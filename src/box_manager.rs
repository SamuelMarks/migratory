//! Box management module.
//!
//! This module provides functionality for managing Vagrant-compatible boxes,
//! such as unpacking them into target directories, downloading, and caching.

use crate::error::MigratoryError;
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use std::fs::{self, File};
#[cfg(test)]
use std::io::Write;
use std::path::{Path, PathBuf};
use tar::Archive;

/// Represents semantic version components (major, minor, patch).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// Major version number.
    pub major: u64,
    /// Minor version number.
    pub minor: u64,
    /// Patch version number.
    pub patch: u64,
}

impl Version {
    /// Parses a version string (e.g. "1.2.3", "v1.2", "2").
    pub fn parse(s: &str) -> Option<Self> {
        let clean = s.trim().trim_start_matches('v');
        if clean.is_empty() {
            return None;
        }
        let base = if let Some((head, _)) = clean.split_once(['-', '+']) {
            head
        } else {
            clean
        };
        let (major_str, rest) = match base.split_once('.') {
            Some((m, r)) => (m, Some(r)),
            None => (base, None),
        };
        let major: u64 = match major_str.parse() {
            Ok(n) => n,
            Err(_) => return None,
        };
        let (minor, patch) = if let Some(rest_str) = rest {
            if let Some((min_str, pat_str)) = rest_str.split_once('.') {
                (min_str.parse().unwrap_or(0), pat_str.parse().unwrap_or(0))
            } else {
                (rest_str.parse().unwrap_or(0), 0)
            }
        } else {
            (0, 0)
        };
        Some(Version {
            major,
            minor,
            patch,
        })
    }
}

/// Evaluates if a version matches a given constraint string.
/// Supports `~>`, `>=`, `<=`, `>`, `<`, `=`, `!=`, and compound `,` separated constraints.
pub fn matches_version_constraint(version_str: &str, constraint_str: &str) -> bool {
    let ver = match Version::parse(version_str) {
        Some(v) => v,
        None => return false,
    };

    let constraints = constraint_str.split(',');
    for single in constraints {
        let single = single.trim();
        if single.is_empty() {
            continue;
        }

        if let Some(target) = single.strip_prefix("~>") {
            let target_str = target.trim();
            let target_ver = match Version::parse(target_str) {
                Some(v) => v,
                None => return false,
            };
            let upper = if target_str.split('.').count() <= 2 {
                Version {
                    major: target_ver.major + 1,
                    minor: 0,
                    patch: 0,
                }
            } else {
                Version {
                    major: target_ver.major,
                    minor: target_ver.minor + 1,
                    patch: 0,
                }
            };
            if ver < target_ver || ver >= upper {
                return false;
            }
        } else if let Some(target) = single.strip_prefix(">=") {
            let target_ver = match Version::parse(target.trim()) {
                Some(v) => v,
                None => return false,
            };
            if ver < target_ver {
                return false;
            }
        } else if let Some(target) = single.strip_prefix("<=") {
            let target_ver = match Version::parse(target.trim()) {
                Some(v) => v,
                None => return false,
            };
            if ver > target_ver {
                return false;
            }
        } else if let Some(target) = single.strip_prefix('>') {
            let target_ver = match Version::parse(target.trim()) {
                Some(v) => v,
                None => return false,
            };
            if ver <= target_ver {
                return false;
            }
        } else if let Some(target) = single.strip_prefix('<') {
            let target_ver = match Version::parse(target.trim()) {
                Some(v) => v,
                None => return false,
            };
            if ver >= target_ver {
                return false;
            }
        } else if let Some(target) = single.strip_prefix("!=") {
            let target_ver = match Version::parse(target.trim()) {
                Some(v) => v,
                None => return false,
            };
            if ver == target_ver {
                return false;
            }
        } else {
            let target_str = single.strip_prefix('=').unwrap_or(single).trim();
            let target_ver = match Version::parse(target_str) {
                Some(v) => v,
                None => return false,
            };
            if ver != target_ver {
                return false;
            }
        }
    }

    true
}

/// Finds the highest version satisfying the given version constraint.
pub fn resolve_latest_matching_version<'a>(
    versions: &[&'a str],
    constraint: &str,
) -> Option<&'a str> {
    let mut matching: Vec<(&'a str, Version)> = versions
        .iter()
        .filter_map(|&v| {
            if matches_version_constraint(v, constraint) {
                Version::parse(v).map(|parsed| (v, parsed))
            } else {
                None
            }
        })
        .collect();

    matching.sort_by(|a, b| a.1.cmp(&b.1));
    matching.last().map(|(v, _)| *v)
}

/// Box manager.
///
/// Handles global box caching, downloading, and extraction.
pub struct BoxManager {
    /// The global directory where boxes are cached (e.g. `~/.vagrant.d/boxes`).
    pub global_boxes_dir: PathBuf,
}

impl BoxManager {
    /// Creates a new `BoxManager`.
    ///
    /// # Arguments
    ///
    /// * `global_dir` - The path to the global vagrant directory (e.g. `~/.vagrant.d`).
    pub fn new(global_dir: &Path) -> Self {
        Self {
            global_boxes_dir: global_dir.join("boxes"),
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Verifies that a downloaded or copied file matches an expected checksum.
fn verify_checksum(
    dest_path: &Path,
    checksum_type: Option<&str>,
    expected_checksum: Option<&str>,
) -> Result<(), MigratoryError> {
    if let (Some(ctype), Some(expected)) = (checksum_type, expected_checksum) {
        use md5::Md5;
        use sha1::Sha1;
        use sha2::{Digest, Sha256, Sha512};

        let bytes = fs::read(dest_path)?;
        let result = match ctype {
            "sha256" => to_hex(&Sha256::digest(&bytes)),
            "sha512" => to_hex(&Sha512::digest(&bytes)),
            "sha1" => to_hex(&Sha1::digest(&bytes)),
            "md5" => to_hex(&Md5::digest(&bytes)),
            _ => String::new(),
        };

        if !result.is_empty() && !crate::constant_time_compare(&result, expected) {
            let _ = fs::remove_file(dest_path);
            return Err(MigratoryError::Generic(format!(
                "Checksum mismatch: expected {}, got {}",
                expected, result
            )));
        }
    }
    Ok(())
}

impl BoxManager {
    /// Downloads or copies a `.box` file from a URL or local path.
    ///
    /// # Arguments
    ///
    /// * `url` - The download URL or local file path.
    /// * `dest_path` - The path to save the downloaded file.
    /// * `expected_checksum` - Optional expected checksum.
    /// * `checksum_type` - Optional checksum type (currently only "sha256" is supported).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on network failure, IO error, or checksum mismatch.
    pub fn download_box(
        &self,
        url: &str,
        dest_path: &Path,
        expected_checksum: Option<&str>,
        checksum_type: Option<&str>,
    ) -> Result<(), MigratoryError> {
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let is_http = url.starts_with("http://") || url.starts_with("https://");

        if is_http {
            let client = Client::new();
            let mut response = client
                .get(url)
                .send()
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;

            if !response.status().is_success() {
                return Err(MigratoryError::Generic(format!(
                    "Failed to download box: {}",
                    response.status()
                )));
            }

            let mut file = File::create(dest_path)?;
            std::io::copy(&mut response, &mut file)?;
            verify_checksum(dest_path, checksum_type, expected_checksum)?;
        } else {
            // Handle local file or file:// URI
            let path_str = if url.starts_with("file://") {
                url.trim_start_matches("file://")
            } else {
                url
            };

            let src_path = Path::new(path_str);
            if !src_path.exists() {
                return Err(MigratoryError::NotFound(format!(
                    "Box file not found at: {}",
                    path_str
                )));
            }

            fs::copy(src_path, dest_path)?;
            verify_checksum(dest_path, checksum_type, expected_checksum)?;
        }

        Ok(())
    }

    /// Adds a .box file to the global cache.
    ///
    /// # Arguments
    ///
    /// * name - The box name.
    /// * version - The version string.
    /// * provider - The provider name.
    /// * box_file - The local path to the box file.
    ///
    /// # Returns
    ///
    /// Returns Ok(()) on success.
    pub fn add_box(
        &self,
        name: &str,
        version: &str,
        provider: &str,
        box_file: &Path,
    ) -> Result<(), MigratoryError> {
        // Replace slashes in name for directory structure, e.g. hashicorp/bionic64 -> hashicorp-VAGRANTSLASH-bionic64
        // Vagrant uses `-VAGRANTSLASH-` for namespaced boxes.
        let safe_name = name.replace('/', "-VAGRANTSLASH-");
        let dest_dir = self
            .global_boxes_dir
            .join(safe_name)
            .join(version)
            .join(provider);

        fs::create_dir_all(&dest_dir)?;

        // Mock signature verification
        if let Ok(sig_path) = std::env::var("MIGRATORY_MOCK_SIGNATURE") {
            let _ =
                fs::read_to_string(sig_path).map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }

        unpack_box(box_file, &dest_dir)
    }

    /// Prunes old versions of boxes.
    ///
    /// # Arguments
    ///
    /// * `provider_filter` - Optional provider to limit pruning to.
    /// * `dry_run` - If true, only print what would be deleted.
    /// * `keep_active` - If true, keep boxes that are in use (not fully implemented).
    /// * `ui` - The UI to print information to.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if reading directories fails.
    pub fn prune(
        &self,
        provider_filter: Option<&str>,
        dry_run: bool,
        _keep_active: bool,
        ui: &impl crate::ui::Ui,
    ) -> Result<(), MigratoryError> {
        if !self.global_boxes_dir.exists() {
            return Ok(());
        }

        let box_entries = fs::read_dir(&self.global_boxes_dir)?;
        for box_entry in box_entries.flatten() {
            let box_path = box_entry.path();
            if !box_path.is_dir() {
                continue;
            }

            let box_name = box_entry
                .file_name()
                .to_string_lossy()
                .replace("-VAGRANTSLASH-", "/");

            let mut versions: Vec<PathBuf> = Vec::new();
            if let Ok(ver_entries) = fs::read_dir(&box_path) {
                for ver_entry in ver_entries.flatten() {
                    if ver_entry.path().is_dir() {
                        versions.push(ver_entry.path());
                    }
                }
            }

            // Sort versions (naive sort by string, ideally parse semantic versions)
            versions.sort();

            if versions.len() > 1 {
                // Keep the latest version
                let keep_version = versions.pop().unwrap_or_default();
                ui.info(
                    "box",
                    &format!(
                        "Keeping latest version {} for box {}",
                        keep_version
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy(),
                        box_name
                    ),
                );

                for old_ver in versions {
                    let old_ver_str = old_ver.file_name().unwrap_or_default().to_string_lossy();
                    if let Ok(prov_entries) = fs::read_dir(&old_ver) {
                        for prov_entry in prov_entries.flatten() {
                            if prov_entry.path().is_dir() {
                                let prov_name =
                                    prov_entry.file_name().to_string_lossy().to_string();
                                if let Some(f) = provider_filter
                                    && f != prov_name
                                {
                                    continue;
                                }

                                if dry_run {
                                    ui.info(
                                        "box",
                                        &format!(
                                            "Would remove: {} (v{}) provider: {}",
                                            box_name, old_ver_str, prov_name
                                        ),
                                    );
                                } else {
                                    ui.info(
                                        "box",
                                        &format!(
                                            "Removing: {} (v{}) provider: {}",
                                            box_name, old_ver_str, prov_name
                                        ),
                                    );
                                    let _ = fs::remove_dir_all(prov_entry.path());
                                }
                            }
                        }
                    }

                    // Cleanup empty version dir
                    if !dry_run {
                        let _ = fs::remove_dir(&old_ver);
                    }
                }
            }
        }
        Ok(())
    }

    fn check_outdated(
        &self,
        name: &str,
    ) -> Result<Option<crate::cloud::BoxMetadata>, MigratoryError> {
        if name.is_empty() {
            return Err(MigratoryError::Generic("Name cannot be empty".to_string()));
        }

        let safe_name = name.replace('/', "-VAGRANTSLASH-");
        let box_dir = self.global_boxes_dir.join(&safe_name);
        if !box_dir.exists() {
            return Ok(None);
        }

        let mut latest_local = Version {
            major: 0,
            minor: 0,
            patch: 0,
        };
        if let Ok(entries) = std::fs::read_dir(&box_dir) {
            for entry in entries.flatten() {
                if let Some(v) = Version::parse(&entry.file_name().to_string_lossy())
                    && v > latest_local
                {
                    latest_local = v;
                }
            }
        }

        if let Ok(cloud) = crate::cloud::CloudClient::new()
            && let Ok(meta) = cloud.fetch_metadata(name)
        {
            for v in &meta.versions {
                if let Some(remote_ver) = Version::parse(&v.version)
                    && remote_ver > latest_local
                {
                    return Ok(Some(meta));
                }
            }
        }

        Ok(None)
    }

    /// Checks if a local box is outdated compared to Vagrant Cloud.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the box.
    ///
    /// # Returns
    ///
    /// Returns `Ok(bool)` indicating if a newer version is available.
    pub fn outdated(&self, name: &str) -> Result<bool, MigratoryError> {
        Ok(self.check_outdated(name)?.is_some())
    }

    /// Updates a box to the latest version available on Vagrant Cloud.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the box.
    /// * `provider` - Optional provider filter.
    ///
    /// # Returns
    ///
    /// Returns `Ok(bool)` indicating if an update was installed.
    pub fn update(&self, name: &str, provider: Option<&str>) -> Result<bool, MigratoryError> {
        let Some(meta) = self.check_outdated(name)? else {
            return Ok(false);
        };

        let mut latest_v = None;
        for v in &meta.versions {
            if let Some(parsed) = Version::parse(&v.version)
                && latest_v.as_ref().map(|(_, p)| &parsed > p).unwrap_or(true)
            {
                latest_v = Some((v, parsed));
            }
        }

        let target_prov = latest_v.as_ref().and_then(|(target_version, _)| {
            target_version
                .providers
                .iter()
                .find(|p| {
                    if let Some(prov_filter) = provider {
                        p.name == prov_filter
                    } else {
                        true
                    }
                })
                .map(|prov| (*target_version, prov))
        });

        if let Some((target_version, prov)) = target_prov {
            let temp_box = self
                .global_boxes_dir
                .join(format!(".tmp-update-{}.box", uuid::Uuid::new_v4()));

            self.download_box(
                &prov.url,
                &temp_box,
                prov.checksum.as_deref(),
                prov.checksum_type.as_deref(),
            )?;

            self.add_box(name, &target_version.version, &prov.name, &temp_box)?;
            let _ = fs::remove_file(&temp_box);
            return Ok(true);
        }

        Ok(false)
    }

    /// Repackages a box.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the box.
    /// * `provider` - The provider of the box.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    pub fn repackage(&self, name: &str, provider: &str) -> Result<(), MigratoryError> {
        if name.is_empty() {
            return Err(MigratoryError::Generic("Name cannot be empty".to_string()));
        }

        let safe_name = name.replace('/', "-VAGRANTSLASH-");
        let box_dir = self.global_boxes_dir.join(&safe_name);

        if !box_dir.exists() {
            return Err(MigratoryError::NotFound(format!("Box '{}'", name)));
        }

        let mut target_version = String::new();
        if let Ok(entries) = std::fs::read_dir(&box_dir) {
            for entry in entries.flatten() {
                let ver = entry.file_name().to_string_lossy().to_string();
                if ver > target_version {
                    target_version = ver;
                }
            }
        }

        if target_version.is_empty() {
            return Err(MigratoryError::NotFound(format!(
                "No versions found for box '{}'",
                name
            )));
        }

        let provider_dir = box_dir.join(&target_version).join(provider);
        if !provider_dir.exists() {
            return Err(MigratoryError::NotFound(format!(
                "Provider '{}' not found for box '{}' version '{}'",
                provider, name, target_version
            )));
        }

        let package_name = format!("package-{}-{}-{}.box", safe_name, target_version, provider);
        let dest_file = std::env::current_dir()
            .unwrap_or(std::path::PathBuf::from("."))
            .join(&package_name);

        let file = File::create(&dest_file).map_err(MigratoryError::Io)?;
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(enc);

        tar.append_dir_all(".", &provider_dir)
            .map_err(MigratoryError::Io)?;
        let _ = tar.finish();

        println!("Box repackaged successfully to {}", dest_file.display());
        Ok(())
    }
}

/// Unpacks a `.box` file (tar gzip) to a destination directory.
///
/// # Arguments
///
/// * `box_path` - The path to the `.box` (tar.gz) file to unpack.
/// * `dest_dir` - The path to the directory where the contents should be unpacked.
///
/// # Returns
///
/// Returns `Ok(())` on successful unpacking, or a `MigratoryError` if the file
/// cannot be read or unpacked.
pub fn unpack_box(box_path: &Path, dest_dir: &Path) -> Result<(), MigratoryError> {
    let file = File::open(box_path)?;
    let tar = GzDecoder::new(file);
    let mut archive = Archive::new(tar);
    archive.unpack(dest_dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use httpmock::prelude::*;
    use tempfile::tempdir;

    #[test]
    fn test_download_box_no_parent() {
        let server = httpmock::MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/");
            then.status(200).body("box data");
        });

        let manager = BoxManager::new(Path::new(""));
        let result = manager.download_box(&server.url("/"), Path::new(""), None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_box_manager_new() {
        let manager = BoxManager::new(Path::new("/tmp/.vagrant.d"));
        assert_eq!(manager.global_boxes_dir, Path::new("/tmp/.vagrant.d/boxes"));
    }

    #[test]
    fn test_download_box_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/box.box");
            then.status(200).body("box data");
        });

        let dir = tempdir().expect("operation should succeed");
        let dest = dir.path().join("downloaded.box");
        let manager = BoxManager::new(dir.path());

        let url = server.url("/box.box");
        assert!(manager.download_box(&url, &dest, None, None).is_ok());
        assert_eq!(
            fs::read_to_string(&dest).expect("operation should succeed"),
            "box data"
        );
        mock.assert();
    }

    #[test]
    fn test_download_box_checksum_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/box.box");
            then.status(200).body("box data");
        });

        let dir = tempdir().expect("operation should succeed");
        let dest = dir.path().join("downloaded.box");
        let manager = BoxManager::new(dir.path());

        // Checksum for "box data"
        let expected_checksum = "5ea8ba3b58ef23e35ecb2c980ace915e73f10123094cd8b48221ca203fd32168";

        let url = server.url("/box.box");
        assert!(
            manager
                .download_box(&url, &dest, Some(expected_checksum), Some("sha256"))
                .is_ok()
        );
        mock.assert();
    }

    #[test]
    fn test_download_box_checksum_mismatch() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/box.box");
            then.status(200).body("box data");
        });

        let dir = tempdir().expect("operation should succeed");
        let dest = dir.path().join("downloaded.box");
        let manager = BoxManager::new(dir.path());

        let url = server.url("/box.box");
        let result = manager.download_box(&url, &dest, Some("badchecksum"), Some("sha256"));
        assert!(result.is_err());
        assert!(!dest.exists()); // should be cleaned up
        mock.assert();
    }

    #[test]
    fn test_add_box() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());

        let box_path = dir.path().join("test.box");
        let file = File::create(&box_path).expect("operation should succeed");
        let mut encoder = GzEncoder::new(file, Compression::default());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(2);
            header.set_cksum();
            builder
                .append_data(&mut header, "metadata.json", &b"{}"[..])
                .expect("operation should succeed");
            builder.into_inner().expect("operation should succeed");
        }
        encoder.finish().expect("operation should succeed");

        assert!(
            manager
                .add_box("hashicorp/bionic64", "1.0.0", "virtualbox", &box_path)
                .is_ok()
        );

        let expected_dir = dir
            .path()
            .join("boxes")
            .join("hashicorp-VAGRANTSLASH-bionic64")
            .join("1.0.0")
            .join("virtualbox");
        assert!(expected_dir.exists());
        assert!(expected_dir.join("metadata.json").exists());
    }

    #[test]
    fn test_prune_boxes() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let ui = crate::ui::ConsoleUi;

        let box_name = "test-VAGRANTSLASH-box";
        let box_dir = manager.global_boxes_dir.join(box_name);

        let v1_dir = box_dir.join("1.0.0").join("virtualbox");
        let v2_dir = box_dir.join("2.0.0").join("virtualbox");
        let v0_dir = box_dir.join("0.5.0");
        fs::create_dir_all(&v1_dir).expect("operation should succeed");
        fs::create_dir_all(&v2_dir).expect("operation should succeed");
        fs::create_dir(&v0_dir).expect("operation should succeed");

        let unreadable_box = manager.global_boxes_dir.join("unreadable-box");
        fs::create_dir(&unreadable_box).expect("operation should succeed");

        use std::os::unix::fs::PermissionsExt;
        let orig_unreadable = fs::metadata(&unreadable_box)
            .expect("operation should succeed")
            .permissions();
        let orig_v0 = fs::metadata(&v0_dir)
            .expect("operation should succeed")
            .permissions();
        fs::set_permissions(&unreadable_box, fs::Permissions::from_mode(0o000))
            .expect("operation should succeed");
        fs::set_permissions(&v0_dir, fs::Permissions::from_mode(0o000))
            .expect("operation should succeed");

        File::create(box_dir.join("non_dir_entry")).expect("operation should succeed");
        File::create(box_dir.join("1.0.0").join("non_dir_prov")).expect("operation should succeed");

        // Prune older versions
        assert!(manager.prune(None, false, true, &ui).is_ok());

        let _ = fs::set_permissions(&unreadable_box, orig_unreadable);
        let _ = fs::set_permissions(&v0_dir, orig_v0);

        assert!(!v1_dir.exists());
        assert!(v2_dir.exists());
    }

    #[test]
    fn test_unpack_missing_file() {
        let result = unpack_box(Path::new("missing.box"), Path::new("dest"));
        assert!(matches!(result, Err(MigratoryError::Io(_))));
    }

    #[test]
    fn test_unpack_success() {
        let dir = tempdir().expect("operation should succeed");
        let box_path = dir.path().join("test.box");
        let dest_dir = dir.path().join("dest");
        std::fs::create_dir(&dest_dir).expect("operation should succeed");

        // Create a dummy tar.gz file
        let file = File::create(&box_path).expect("operation should succeed");
        let mut encoder = GzEncoder::new(file, Compression::default());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(11);
            header.set_cksum();
            builder
                .append_data(&mut header, "test.txt", &b"hello world"[..])
                .expect("operation should succeed");
            builder.into_inner().expect("operation should succeed");
        }
        encoder.finish().expect("operation should succeed");

        let result = unpack_box(&box_path, &dest_dir);
        assert!(result.is_ok());

        let file_contents =
            std::fs::read_to_string(dest_dir.join("test.txt")).expect("operation should succeed");
        assert_eq!(file_contents, "hello world");
    }

    #[test]
    fn test_download_box_not_found() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(GET).path("/box.box");
            then.status(404);
        });

        let dir = tempdir().expect("operation should succeed");
        let dest = dir.path().join("downloaded.box");
        let manager = BoxManager::new(dir.path());

        let url = server.url("/box.box");
        let result = manager.download_box(&url, &dest, None, None);
        assert!(result.is_err());
        mock.assert();
    }

    #[test]
    fn test_download_box_network_error() {
        let dir = tempdir().expect("operation should succeed");
        let dest = dir.path().join("downloaded.box");
        let manager = BoxManager::new(dir.path());

        // Use totally invalid urls (http and https) to force reqwest errors
        let result_http =
            manager.download_box("http://invalid.localdomain.none/b.box", &dest, None, None);
        assert!(result_http.is_err());
        let result_https =
            manager.download_box("https://invalid.localdomain.none/b.box", &dest, None, None);
        assert!(result_https.is_err());
    }

    #[test]
    fn test_download_box_io_error_during_copy() {
        let dir = tempdir().expect("operation should succeed");
        let dest = dir.path().join("downloaded.box");
        let manager = BoxManager::new(dir.path());

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();

        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 512];
            let _ = stream.read(&mut buf);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\npartial_payload");
            let _ = stream.flush();
        });

        let url = format!("http://127.0.0.1:{}/truncated.box", port);
        let result = manager.download_box(&url, &dest, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_prune_empty() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let ui = crate::ui::ConsoleUi;
        // Directory doesn't exist
        assert!(manager.prune(None, false, true, &ui).is_ok());
    }

    #[test]
    fn test_prune_dry_run_and_provider_filter() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let ui = crate::ui::ConsoleUi;

        let box_name = "test-VAGRANTSLASH-box";
        let box_dir = manager.global_boxes_dir.join(box_name);

        let v1_vb = box_dir.join("1.0.0").join("virtualbox");
        let v1_vm = box_dir.join("1.0.0").join("vmware");
        let v2_vb = box_dir.join("2.0.0").join("virtualbox");
        fs::create_dir_all(&v1_vb).expect("operation should succeed");
        fs::create_dir_all(&v1_vm).expect("operation should succeed");
        fs::create_dir_all(&v2_vb).expect("operation should succeed");

        // Prune with dry run and filter
        assert!(manager.prune(Some("virtualbox"), true, true, &ui).is_ok());

        // Since it was a dry run, nothing should be deleted
        assert!(v1_vb.exists());
        assert!(v1_vm.exists());
        assert!(v2_vb.exists());

        // Now actual prune with filter "virtualbox"
        assert!(manager.prune(Some("virtualbox"), false, true, &ui).is_ok());
        assert!(!v1_vb.exists()); // pruned
        assert!(v1_vm.exists()); // skipped due to filter
        assert!(v2_vb.exists()); // kept because it's latest
    }

    #[test]
    fn test_stubs() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        assert_eq!(
            manager.outdated("box").expect("operation should succeed"),
            false
        );
        assert!(manager.repackage("box", "prov").is_err());
    }

    #[test]
    fn test_unpack_invalid_tar() {
        let dir = tempdir().expect("operation should succeed");
        let box_path = dir.path().join("test.box");
        let dest_dir = dir.path().join("dest");

        // Write invalid data
        let mut file = File::create(&box_path).expect("operation should succeed");
        file.write_all(b"not a tar.gz")
            .expect("operation should succeed");

        let result = unpack_box(&box_path, &dest_dir);
        assert!(matches!(result, Err(MigratoryError::Io(_))));
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;
    use crate::ui::ConsoleUi;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_prune_ignores_files() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        fs::create_dir_all(&manager.global_boxes_dir).expect("operation should succeed");
        // Create a file instead of a dir to test the !box_path.is_dir() continue path
        File::create(manager.global_boxes_dir.join("not_a_dir_box"))
            .expect("operation should succeed");

        // Create a box dir with a file inside it instead of a version dir
        let box_dir = manager.global_boxes_dir.join("real_box");
        fs::create_dir_all(&box_dir).expect("operation should succeed");
        File::create(box_dir.join("not_a_version_dir")).expect("operation should succeed");

        // Create a valid version dir but with a file inside it instead of a provider dir
        let version_dir1 = box_dir.join("1.0.0");
        let version_dir2 = box_dir.join("2.0.0");
        fs::create_dir_all(&version_dir1).expect("operation should succeed");
        fs::create_dir_all(&version_dir2).expect("operation should succeed"); // To make versions.len() > 1
        File::create(version_dir1.join("not_a_provider_dir")).expect("operation should succeed");

        // Box with exactly 1 version to cover `versions.len() == 1` branch
        let box_dir_1ver = manager.global_boxes_dir.join("one_ver_box");
        fs::create_dir_all(&box_dir_1ver).expect("operation should succeed");
        fs::create_dir_all(box_dir_1ver.join("1.0.0")).expect("operation should succeed");

        let ui = ConsoleUi;
        assert!(manager.prune(None, false, false, &ui).is_ok());
    }

    #[test]
    fn test_repackage_box_dir_is_file() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        std::fs::create_dir_all(&manager.global_boxes_dir).expect("operation should succeed");
        File::create(manager.global_boxes_dir.join("file-box")).expect("operation should succeed");
        let result = manager.repackage("file-box", "provider");
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_repackage_empty_box_dir() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        std::fs::create_dir_all(manager.global_boxes_dir.join("empty-box"))
            .expect("operation should succeed");
        let result = manager.repackage("empty-box", "provider");
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }
    #[test]
    fn test_repackage_success() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let box_dir = manager.global_boxes_dir.join("test-box");
        fs::create_dir_all(box_dir.join("0.1.0")).expect("operation should succeed");
        fs::create_dir_all(box_dir.join("0.5.0")).expect("operation should succeed");
        let prov_dir = box_dir.join("2.0.0").join("virtualbox");
        fs::create_dir_all(&prov_dir).expect("operation should succeed");
        fs::create_dir_all(box_dir.join("1.0.0").join("virtualbox"))
            .expect("operation should succeed");

        // Add a dummy file inside the provider dir
        File::create(prov_dir.join("Vagrantfile")).expect("operation should succeed");

        // Should find version 2.0.0 and package it
        let res = manager.repackage("test-box", "virtualbox");
        assert!(res.is_ok(), "{:?}", res);

        let package_name = "package-test-box-2.0.0-virtualbox.box";
        let dest_file = std::env::current_dir()
            .unwrap_or(std::path::PathBuf::from("."))
            .join(package_name);

        assert!(dest_file.exists());
        let _ = fs::remove_file(dest_file); // cleanup
    }

    #[test]
    fn test_repackage_no_versions() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let box_dir = manager.global_boxes_dir.join("test-box");
        fs::create_dir_all(&box_dir).expect("operation should succeed");

        let result = manager.repackage("test-box", "virtualbox");
        assert!(
            matches!(result, Err(MigratoryError::NotFound(_))),
            "{:?}",
            result
        );
    }

    #[test]
    fn test_repackage_missing_provider() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let box_dir = manager.global_boxes_dir.join("test-box");
        fs::create_dir_all(box_dir.join("1.0.0")).expect("operation should succeed"); // Create version dir but no provider dir

        let result = manager.repackage("test-box", "virtualbox");
        assert!(
            matches!(result, Err(MigratoryError::NotFound(_))),
            "{:?}",
            result
        );
    }
}
#[cfg(test)]
mod extra_coverage_tests {
    use super::*;
    use httpmock::MockServer;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_download_box_sha512_http() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/box.box");
            then.status(200).body("box data");
        });

        let dir = tempdir().expect("operation should succeed");
        let dest = dir.path().join("downloaded.box");
        let manager = BoxManager::new(dir.path());

        use sha2::{Digest, Sha512};
        let mut hasher = Sha512::new();
        hasher.update(b"box data");
        let expected_checksum = hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        let url = server.url("/box.box");
        assert!(
            manager
                .download_box(&url, &dest, Some(&expected_checksum), Some("sha512"))
                .is_ok()
        );
    }

    #[test]
    fn test_download_box_sha1_http() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/box.box");
            then.status(200).body("box data");
        });

        let dir = tempdir().expect("operation should succeed");
        let dest = dir.path().join("downloaded.box");
        let manager = BoxManager::new(dir.path());

        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(b"box data");
        let expected_checksum = hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        let url = server.url("/box.box");
        assert!(
            manager
                .download_box(&url, &dest, Some(&expected_checksum), Some("sha1"))
                .is_ok()
        );
    }

    #[test]
    fn test_download_box_md5_http() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/box.box");
            then.status(200).body("box data");
        });

        let dir = tempdir().expect("operation should succeed");
        let dest = dir.path().join("downloaded.box");
        let manager = BoxManager::new(dir.path());

        use md5::{Digest, Md5};
        let mut hasher = Md5::new();
        hasher.update(b"box data");
        let expected_checksum = hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        let url = server.url("/box.box");
        assert!(
            manager
                .download_box(&url, &dest, Some(&expected_checksum), Some("md5"))
                .is_ok()
        );
    }

    #[test]
    fn test_download_box_local_file() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());

        let src = dir.path().join("src.box");
        let dest = dir.path().join("dest.box");

        let mut f = File::create(&src).unwrap();
        f.write_all(b"box data").unwrap();

        let url = format!("file://{}", src.display());
        assert!(manager.download_box(&url, &dest, None, None).is_ok());
    }

    #[test]
    fn test_download_box_local_file_without_file_prefix() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());

        let src = dir.path().join("src.box");
        let dest = dir.path().join("dest.box");

        let mut f = File::create(&src).unwrap();
        f.write_all(b"box data").unwrap();

        let url = src.to_string_lossy().to_string();
        assert!(manager.download_box(&url, &dest, None, None).is_ok());
    }

    #[test]
    fn test_download_box_local_file_not_found() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());

        let src = dir.path().join("src.box");
        let dest = dir.path().join("dest.box");

        let url = format!("file://{}", src.display());
        assert!(manager.download_box(&url, &dest, None, None).is_err());
    }

    #[test]
    fn test_download_box_local_file_checksum_sha256() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());

        let src = dir.path().join("src.box");
        let dest = dir.path().join("dest.box");

        let mut f = File::create(&src).unwrap();
        f.write_all(b"box data").unwrap();

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"box data");
        let expected_checksum = hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        let url = format!("file://{}", src.display());
        assert!(
            manager
                .download_box(&url, &dest, Some(&expected_checksum), Some("sha256"))
                .is_ok()
        );
    }

    #[test]
    fn test_download_box_local_file_checksum_sha512() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let src = dir.path().join("src.box");
        let dest = dir.path().join("dest.box");
        let mut f = File::create(&src).unwrap();
        f.write_all(b"box data").unwrap();
        use sha2::{Digest, Sha512};
        let mut hasher = Sha512::new();
        hasher.update(b"box data");
        let expected_checksum = hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        let url = format!("file://{}", src.display());
        assert!(
            manager
                .download_box(&url, &dest, Some(&expected_checksum), Some("sha512"))
                .is_ok()
        );
    }

    #[test]
    fn test_download_box_local_file_checksum_sha1() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let src = dir.path().join("src.box");
        let dest = dir.path().join("dest.box");
        let mut f = File::create(&src).unwrap();
        f.write_all(b"box data").unwrap();
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(b"box data");
        let expected_checksum = hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        let url = format!("file://{}", src.display());
        assert!(
            manager
                .download_box(&url, &dest, Some(&expected_checksum), Some("sha1"))
                .is_ok()
        );
    }

    #[test]
    fn test_download_box_local_file_checksum_md5() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let src = dir.path().join("src.box");
        let dest = dir.path().join("dest.box");
        let mut f = File::create(&src).unwrap();
        f.write_all(b"box data").unwrap();
        use md5::{Digest, Md5};
        let mut hasher = Md5::new();
        hasher.update(b"box data");
        let expected_checksum = hasher
            .finalize()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        let url = format!("file://{}", src.display());
        assert!(
            manager
                .download_box(&url, &dest, Some(&expected_checksum), Some("md5"))
                .is_ok()
        );
    }

    #[test]
    fn test_download_box_local_file_checksum_mismatch() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let src = dir.path().join("src.box");
        let dest = dir.path().join("dest.box");
        let mut f = File::create(&src).unwrap();
        f.write_all(b"box data").unwrap();

        let expected_checksum = "badchecksum";
        let url = format!("file://{}", src.display());
        assert!(
            manager
                .download_box(&url, &dest, Some(expected_checksum), Some("sha256"))
                .is_err()
        );
    }

    #[test]
    fn test_download_box_local_file_checksum_unknown() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let src = dir.path().join("src.box");
        let dest = dir.path().join("dest.box");
        let mut f = File::create(&src).unwrap();
        f.write_all(b"box data").unwrap();

        let url = format!("file://{}", src.display());
        // A dummy checksum with an unknown checksum_type
        assert!(
            manager
                .download_box(&url, &dest, Some("dummy"), Some("unknown"))
                .is_ok()
        );
    }

    #[test]
    fn test_download_box_http_checksum_unknown() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/box.box");
            then.status(200).body("box data");
        });

        let dir = tempdir().expect("operation should succeed");
        let dest = dir.path().join("downloaded.box");
        let manager = BoxManager::new(dir.path());

        let url = server.url("/box.box");
        assert!(
            manager
                .download_box(&url, &dest, Some("dummy"), Some("unknown"))
                .is_ok()
        );
    }

    #[test]
    fn test_add_box_mock_signature_error() {
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());

        unsafe {
            std::env::set_var("MIGRATORY_MOCK_SIGNATURE", "/path/that/does/not/exist");
        }
        let box_path = dir.path().join("test.box");
        let result = manager.add_box("test/box", "1.0.0", "virtualbox", &box_path);

        unsafe {
            std::env::remove_var("MIGRATORY_MOCK_SIGNATURE");
        }

        assert!(result.is_err());
    }

    #[test]
    fn test_version_parsing_and_constraints() {
        assert_eq!(
            Version::parse("1.2.3"),
            Some(Version {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        assert_eq!(
            Version::parse("v2.0"),
            Some(Version {
                major: 2,
                minor: 0,
                patch: 0
            })
        );
        assert!(Version::parse("invalid").is_none());

        assert!(matches_version_constraint("1.2.3", ">= 1.0.0, < 2.0.0"));
        assert!(!matches_version_constraint("2.0.0", ">= 1.0.0, < 2.0.0"));
        assert!(matches_version_constraint("1.2.5", "~> 1.2"));
        assert!(!matches_version_constraint("2.0.0", "~> 1.2"));
        assert!(!matches_version_constraint("1.0.0", "~> 1.2"));
        assert!(matches_version_constraint("1.2.9", "~> 1.2.3"));
        assert!(!matches_version_constraint("1.3.0", "~> 1.2.3"));
        assert!(!matches_version_constraint("1.1.9", "~> 1.2.0"));
        assert!(matches_version_constraint("2.1.0", "> 2.0.0"));
        assert!(matches_version_constraint("1.9.9", "<= 2.0.0"));
        assert!(matches_version_constraint("1.5.0", "!= 1.0.0"));
        assert!(!matches_version_constraint("1.0.0", "!= 1.0.0"));
        assert!(matches_version_constraint("1.0.0", "= 1.0.0"));

        // Additional constraint branch coverage
        assert_eq!(
            Version::parse("1"),
            Some(Version {
                major: 1,
                minor: 0,
                patch: 0
            })
        );
        assert_eq!(
            Version::parse("1.2.3-alpha"),
            Some(Version {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        assert_eq!(
            Version::parse("1.x.y"),
            Some(Version {
                major: 1,
                minor: 0,
                patch: 0
            })
        );
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse("v"), None);
        assert!(!matches_version_constraint("invalid_ver", ">= 1.0.0"));
        assert!(matches_version_constraint("1.0.0", ", 1.0.0, "));
        assert!(!matches_version_constraint("1.0.0", "~> invalid"));
        assert!(!matches_version_constraint("1.0.0", ">= invalid"));
        assert!(!matches_version_constraint("1.0.0", ">= 2.0.0"));
        assert!(!matches_version_constraint("1.0.0", "<= invalid"));
        assert!(!matches_version_constraint("2.0.0", "<= 1.0.0"));
        assert!(!matches_version_constraint("1.0.0", "> invalid"));
        assert!(!matches_version_constraint("1.0.0", "> 1.0.0"));
        assert!(!matches_version_constraint("1.0.0", "> 2.0.0"));
        assert!(!matches_version_constraint("1.0.0", "< invalid"));
        assert!(!matches_version_constraint("2.0.0", "< 1.0.0"));
        assert!(matches_version_constraint("1.0.0", "< 2.0.0"));
        assert!(!matches_version_constraint("1.0.0", "!= invalid"));
        assert!(!matches_version_constraint("1.0.0", "= invalid"));
        assert!(!matches_version_constraint("1.0.0", "= 2.0.0"));
        assert!(!matches_version_constraint("1.0.0", "invalid"));
        assert!(!matches_version_constraint("1.0.0", "2.0.0"));

        let versions = ["1.0.0", "1.1.0", "1.2.0", "1.2.5", "2.0.0"];
        let resolved = resolve_latest_matching_version(&versions, "~> 1.2.0");
        assert_eq!(resolved, Some("1.2.5"));

        let resolved_none = resolve_latest_matching_version(&versions, ">= 3.0.0");
        assert_eq!(resolved_none, None);

        let empty_versions: [&str; 0] = [];
        assert_eq!(
            resolve_latest_matching_version(&empty_versions, ">= 1.0.0"),
            None
        );
        let unsorted_versions = ["2.0.0", "1.0.0", "1.5.0"];
        assert_eq!(
            resolve_latest_matching_version(&unsorted_versions, ">= 1.0.0"),
            Some("2.0.0")
        );
    }

    fn create_dummy_box_bytes() -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut encoder =
                flate2::write::GzEncoder::new(&mut buf, flate2::Compression::default());
            {
                let mut builder = tar::Builder::new(&mut encoder);
                let mut header = tar::Header::new_gnu();
                header.set_size(2);
                header.set_cksum();
                builder
                    .append_data(&mut header, "metadata.json", &b"{}"[..])
                    .expect("operation should succeed");
                builder.into_inner().expect("operation should succeed");
            }
            encoder.finish().expect("operation should succeed");
        }
        buf
    }

    #[test]
    fn test_box_manager_outdated_and_update() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.base_url());
        }

        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());

        // Test outdated with empty name
        assert!(manager.outdated("").is_err());

        // Test outdated when box_dir does not exist
        assert_eq!(
            manager
                .outdated("my/box")
                .expect("operation should succeed"),
            false
        );

        // Setup local box directories for my/box:
        // "1.0.0", "0.5.0" (lower), and "not-a-ver"
        let safe_name = "my-VAGRANTSLASH-box";
        let box_dir = manager.global_boxes_dir.join(safe_name);
        fs::create_dir_all(box_dir.join("1.0.0/virtualbox")).expect("operation should succeed");
        fs::create_dir_all(box_dir.join("0.5.0/virtualbox")).expect("operation should succeed");
        fs::create_dir_all(box_dir.join("not-a-ver/virtualbox")).expect("operation should succeed");

        let box_bytes = create_dummy_box_bytes();
        let box_checksum = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&box_bytes);
            hasher
                .finalize()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>()
        };

        let _box_mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/box.box");
            then.status(200).body(box_bytes);
        });

        // Test metadata fetch error: outdated returns false
        let mut mock_meta_err = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/boxes/meta-err");
            then.status(500);
        });
        let err_box_dir = manager.global_boxes_dir.join("meta-err");
        fs::create_dir_all(err_box_dir.join("1.0.0/virtualbox")).expect("operation should succeed");
        assert_eq!(
            manager
                .outdated("meta-err")
                .expect("operation should succeed"),
            false
        );
        mock_meta_err.delete();

        // Test metadata with only older versions: outdated returns false
        let mut mock_meta_older = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/boxes/meta-older");
            then.status(200).json_body(serde_json::json!({
                "description_markdown": "Test Box",
                "short_description": "A test box",
                "name": "meta-older",
                "versions": [
                    {
                        "version": "0.5.0",
                        "providers": []
                    }
                ]
            }));
        });
        let older_box_dir = manager.global_boxes_dir.join("meta-older");
        fs::create_dir_all(older_box_dir.join("1.0.0/virtualbox"))
            .expect("operation should succeed");
        assert_eq!(
            manager
                .outdated("meta-older")
                .expect("operation should succeed"),
            false
        );
        mock_meta_older.delete();

        let _metadata_mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/boxes/my/box");
            then.status(200).json_body(serde_json::json!({
                "description_markdown": "Test Box",
                "short_description": "A test box",
                "name": "my/box",
                "versions": [
                    {
                        "version": "invalid-version",
                        "providers": []
                    },
                    {
                        "version": "0.8.0",
                        "providers": []
                    },
                    {
                        "version": "2.0.0",
                        "providers": [
                            {
                                "name": "virtualbox",
                                "url": server.url("/box.box"),
                                "checksum": box_checksum,
                                "checksum_type": "sha256"
                            }
                        ]
                    },
                    {
                        "version": "1.5.0",
                        "providers": []
                    }
                ]
            }));
        });

        // Now outdated should be true
        assert_eq!(
            manager
                .outdated("my/box")
                .expect("operation should succeed"),
            true
        );

        // Test update with provider filter that doesn't match
        let res_no_match = manager.update("my/box", Some("vmware"));
        assert_eq!(res_no_match.expect("operation should succeed"), false);

        // Test update with provider = None (takes any provider)
        let res_none_prov = manager.update("my/box", None);
        assert_eq!(res_none_prov.expect("operation should succeed"), true);

        // Now version 2.0.0 is installed, so outdated is false
        assert_eq!(
            manager
                .outdated("my/box")
                .expect("operation should succeed"),
            false
        );

        // And update returns false because it's not outdated
        assert_eq!(
            manager
                .update("my/box", None)
                .expect("operation should succeed"),
            false
        );

        // Test update when outdated but target version has no providers
        let _meta_no_provs = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/boxes/no-provs");
            then.status(200).json_body(serde_json::json!({
                "description_markdown": "Test Box",
                "short_description": "A test box",
                "name": "no-provs",
                "versions": [
                    {
                        "version": "3.0.0",
                        "providers": []
                    }
                ]
            }));
        });
        let no_provs_dir = manager.global_boxes_dir.join("no-provs");
        fs::create_dir_all(no_provs_dir.join("1.0.0/virtualbox"))
            .expect("operation should succeed");
        assert_eq!(
            manager
                .update("no-provs", None)
                .expect("operation should succeed"),
            false
        );

        // Test update when download_box fails
        let _meta_fail_dl = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/boxes/fail-dl");
            then.status(200).json_body(serde_json::json!({
                "description_markdown": "Test Box",
                "short_description": "A test box",
                "name": "fail-dl",
                "versions": [
                    {
                        "version": "2.0.0",
                        "providers": [
                            {
                                "name": "virtualbox",
                                "url": server.url("/fail-dl.box")
                            }
                        ]
                    }
                ]
            }));
        });
        let _fail_dl_box = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/fail-dl.box");
            then.status(500);
        });
        let fail_dl_dir = manager.global_boxes_dir.join("fail-dl");
        fs::create_dir_all(fail_dl_dir.join("1.0.0/virtualbox")).expect("operation should succeed");
        assert!(manager.update("fail-dl", None).is_err());

        // Test update when add_box fails (corrupt archive bytes)
        let _meta_fail_add = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/boxes/fail-add");
            then.status(200).json_body(serde_json::json!({
                "description_markdown": "Test Box",
                "short_description": "A test box",
                "name": "fail-add",
                "versions": [
                    {
                        "version": "2.0.0",
                        "providers": [
                            {
                                "name": "virtualbox",
                                "url": server.url("/fail-add.box")
                            }
                        ]
                    }
                ]
            }));
        });
        let _fail_add_box = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/fail-add.box");
            then.status(200).body(b"corrupt non-tar bytes");
        });
        let fail_add_dir = manager.global_boxes_dir.join("fail-add");
        fs::create_dir_all(fail_add_dir.join("1.0.0/virtualbox"))
            .expect("operation should succeed");
        assert!(manager.update("fail-add", None).is_err());

        // Test outdated and update when CloudClient::new fails
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_CLIENT_ERROR", "1");
        }
        let err_client_dir = manager.global_boxes_dir.join("err-client");
        fs::create_dir_all(err_client_dir.join("1.0.0/virtualbox"))
            .expect("operation should succeed");
        assert_eq!(
            manager
                .outdated("err-client")
                .expect("operation should succeed"),
            false
        );
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_CLIENT_ERROR");
        }

        // Test outdated when box_dir exists but is a file (read_dir fails)
        let file_box_path = manager.global_boxes_dir.join("file-box");
        fs::write(&file_box_path, b"not a dir").expect("operation should succeed");
        assert_eq!(
            manager
                .outdated("file-box")
                .expect("operation should succeed"),
            false
        );

        // Test update when outdated is true but latest_v is None (empty versions in update)
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("operation should succeed");
        let port = listener
            .local_addr()
            .expect("operation should succeed")
            .port();
        let server_url = format!("http://127.0.0.1:{}", port);
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", &server_url);
        }

        std::thread::spawn(move || {
            let (mut stream1, _) = listener.accept().expect("accept failed");
            use std::io::{Read, Write};
            let mut buf = [0u8; 1024];
            let _ = stream1.read(&mut buf);
            let body1 = serde_json::json!({
                "description_markdown": "desc",
                "short_description": "short",
                "name": "seq-box",
                "versions": [
                    {
                        "version": "2.0.0",
                        "providers": []
                    }
                ]
            })
            .to_string();
            let resp1 = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body1.len(),
                body1
            );
            let _ = stream1.write_all(resp1.as_bytes());
        });

        let seq_dir = manager.global_boxes_dir.join("seq-box");
        fs::create_dir_all(seq_dir.join("1.0.0/virtualbox")).expect("operation should succeed");
        assert_eq!(
            manager
                .update("seq-box", None)
                .expect("operation should succeed"),
            false
        );

        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
        }
    }

    #[test]
    fn test_io_error_coverage() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/test.box");
            then.status(200).body("box data");
        });

        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());

        // 1. download_box where dest_path is an existing directory -> File::create fails
        let dest_is_dir = dir.path().join("is_a_dir");
        fs::create_dir(&dest_is_dir).expect("operation should succeed");
        let url = server.url("/test.box");
        let dl_res = manager.download_box(&url, &dest_is_dir, None, None);
        assert!(dl_res.is_err());

        // 2. download_box_local_file where dest is an invalid file path because parent is a file
        let src_file = dir.path().join("src.box");
        fs::write(&src_file, b"src content").expect("operation should succeed");
        let local_url = format!("file://{}", src_file.display());
        let bad_parent = dir.path().join("file_parent");
        fs::write(&bad_parent, b"not a dir").expect("operation should succeed");
        let bad_dest = bad_parent.join("sub").join("file");
        let local_res = manager.download_box(&local_url, &bad_dest, None, None);
        assert!(local_res.is_err());

        // 3. add_box where dest_dir creation fails because a regular file exists at parent
        let dummy_box = dir.path().join("dummy.box");
        fs::write(&dummy_box, b"dummy").expect("operation should succeed");
        let blocked = manager.global_boxes_dir.join("blocked-VAGRANTSLASH-box");
        fs::create_dir_all(&manager.global_boxes_dir).expect("operation should succeed");
        fs::write(&blocked, b"file blocking dir").expect("operation should succeed");
        let add_res = manager.add_box("blocked/box", "1.0.0", "virtualbox", &dummy_box);
        assert!(add_res.is_err());

        // 4. unpack_box where dest_dir is a regular file
        let tgz_path = dir.path().join("valid.box");
        let file = File::create(&tgz_path).expect("operation should succeed");
        let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(5);
            header.set_cksum();
            builder
                .append_data(&mut header, "entry.txt", &b"hello"[..])
                .expect("operation should succeed");
            builder.into_inner().expect("operation should succeed");
        }
        encoder.finish().expect("operation should succeed");

        let blocked_dest = dir.path().join("dest_file");
        fs::write(&blocked_dest, b"existing file").expect("operation should succeed");
        let unpack_res = unpack_box(&tgz_path, &blocked_dest);
        assert!(unpack_res.is_err());

        // 5. download_box_local_file where dest is an existing directory -> fs::copy fails
        let copy_dest_dir = dir.path().join("copy_dest_dir");
        fs::create_dir(&copy_dest_dir).expect("operation should succeed");
        let copy_res = manager.download_box(&local_url, &copy_dest_dir, None, None);
        assert!(copy_res.is_err());

        // 6. repackage where dest_file is an existing directory -> File::create fails
        let repkg_box_dir = manager
            .global_boxes_dir
            .join("err-box")
            .join("1.0.0")
            .join("virtualbox");
        fs::create_dir_all(&repkg_box_dir).expect("operation should succeed");
        File::create(repkg_box_dir.join("Vagrantfile")).expect("operation should succeed");
        let pkg_dir = std::env::current_dir()
            .unwrap_or(PathBuf::from("."))
            .join("package-err-box-1.0.0-virtualbox.box");
        let _ = fs::remove_file(&pkg_dir);
        let _ = fs::remove_dir_all(&pkg_dir);
        fs::create_dir(&pkg_dir).expect("operation should succeed");
        let repkg_res = manager.repackage("err-box", "virtualbox");
        assert!(repkg_res.is_err());
        let _ = fs::remove_dir_all(&pkg_dir);

        // 7. update where outdated returns an error -> self.outdated(name)?;
        let update_err = manager.update("", None);
        assert!(update_err.is_err());
    }

    #[test]
    fn test_download_checksum_file_open_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let src_file = dir.path().join("source.box");
        fs::write(&src_file, b"content").expect("operation should succeed");
        let local_url = format!("file://{}", src_file.display());

        let dest_file = dir.path().join("dest_unreadable.box");
        fs::write(&dest_file, b"content").expect("operation should succeed");
        let orig_perms = fs::metadata(&dest_file)
            .expect("operation should succeed")
            .permissions();
        fs::set_permissions(&dest_file, fs::Permissions::from_mode(0o000))
            .expect("operation should succeed");

        let res = manager.download_box(&local_url, &dest_file, Some("abc"), Some("sha1"));
        assert!(res.is_err());

        let _ = fs::set_permissions(&dest_file, orig_perms);
    }

    #[test]
    fn test_repackage_unreadable_file_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().expect("operation should succeed");
        let manager = BoxManager::new(dir.path());
        let prov_dir = manager
            .global_boxes_dir
            .join("unreadable-pkg-box")
            .join("1.0.0")
            .join("virtualbox");
        fs::create_dir_all(&prov_dir).expect("operation should succeed");
        let unreadable_file = prov_dir.join("bad_file");
        fs::write(&unreadable_file, b"content").expect("operation should succeed");
        let orig_perms = fs::metadata(&unreadable_file)
            .expect("operation should succeed")
            .permissions();
        fs::set_permissions(&unreadable_file, fs::Permissions::from_mode(0o000))
            .expect("operation should succeed");

        let res = manager.repackage("unreadable-pkg-box", "virtualbox");
        assert!(res.is_err());

        let _ = fs::set_permissions(&unreadable_file, orig_perms);
    }

    #[test]
    fn test_verify_checksum_read_error() {
        let res = verify_checksum(
            Path::new("non_existent_file_xyz.box"),
            Some("sha256"),
            Some("abc"),
        );
        assert!(res.is_err());
    }
}
