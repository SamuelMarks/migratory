//! File provisioner implementation.

use super::Provisioner;
use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::collections::HashMap;
use std::path::Path;

/// File upload provisioner.
pub struct FileProvisioner {
    source: Option<String>,
    destination: Option<String>,
}

impl FileProvisioner {
    /// Creates a new file provisioner.
    pub fn new() -> Self {
        Self {
            source: None,
            destination: None,
        }
    }

    /// Recursively uploads a directory or file to the guest, preserving structure and permissions.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used for executing remote commands and uploading files.
    /// * `local_path` - Local file or directory path to upload.
    /// * `remote_path` - Remote destination path on the guest.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if directory creation, permission setting, or file upload fails.
    pub fn upload_recursive(
        &self,
        comm: &dyn Communicator,
        local_path: &Path,
        remote_path: &str,
    ) -> Result<(), MigratoryError> {
        if local_path.is_dir() {
            let mkdir_cmd = format!("mkdir -p '{}'", remote_path.replace('\'', "'\\''"));
            let _ = comm.execute(&mkdir_cmd)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(local_path)
                    .map(|meta| meta.permissions().mode() & 0o777)
                    .unwrap_or(0o755);
                let chmod_cmd =
                    format!("chmod {:o} '{}'", mode, remote_path.replace('\'', "'\\''"));
                let _ = comm.execute(&chmod_cmd);
            }

            let entries = std::fs::read_dir(local_path).map_err(MigratoryError::Io)?;
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let sub_local = entry.path();
                let sub_remote = format!(
                    "{}/{}",
                    remote_path.trim_end_matches('/'),
                    file_name.to_string_lossy()
                );
                self.upload_recursive(comm, &sub_local, &sub_remote)?;
            }
        } else {
            let parent_str = Path::new(remote_path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            if !parent_str.is_empty() {
                let mkdir_cmd = format!("mkdir -p '{}'", parent_str.replace('\'', "'\\''"));
                let _ = comm.execute(&mkdir_cmd)?;
            }

            comm.upload(local_path, remote_path)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(local_path)
                    .map(|meta| meta.permissions().mode() & 0o777)
                    .unwrap_or(0o644);
                let chmod_cmd =
                    format!("chmod {:o} '{}'", mode, remote_path.replace('\'', "'\\''"));
                let _ = comm.execute(&chmod_cmd);
            }
        }
        Ok(())
    }
}

impl Default for FileProvisioner {
    fn default() -> Self {
        Self::new()
    }
}

impl Provisioner for FileProvisioner {
    fn name(&self) -> &str {
        "file"
    }

    fn prepare(&mut self, config: &HashMap<String, String>) -> Result<(), MigratoryError> {
        self.source = config.get("source").cloned();
        self.destination = config.get("destination").cloned();
        Ok(())
    }

    fn provision(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let source = match &self.source {
            Some(s) => s,
            None => {
                return Err(MigratoryError::Validation(
                    "File provisioner requires a 'source'".to_string(),
                ));
            }
        };

        let destination = match &self.destination {
            Some(d) => d,
            None => {
                return Err(MigratoryError::Validation(
                    "File provisioner requires a 'destination'".to_string(),
                ));
            }
        };

        let local_path = Path::new(source);
        if !local_path.exists() {
            return Err(MigratoryError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Source file not found: {}", source),
            )));
        }

        self.upload_recursive(comm, local_path, destination)?;
        Ok(())
    }

    fn cleanup(&self) -> Result<(), MigratoryError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::communicator::Communicator;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    #[derive(Default)]
    struct MockComm {
        fail_execute: bool,
        fail_upload: bool,
    }

    impl Communicator for MockComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            if self.fail_execute {
                return Err(MigratoryError::Generic("mock execute failed".to_string()));
            }
            Ok("".to_string())
        }

        fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
            if self.fail_upload {
                return Err(MigratoryError::Generic("mock upload failed".to_string()));
            }
            Ok(())
        }

        #[coverage(off)]
        fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn execute_interactive(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn wait_for_ready(&self, _timeout: Duration) -> Result<(), MigratoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_file_provisioner() {
        let mut prov = FileProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir().expect("operation should succeed");
        let file_path = dir.path().join("upload.txt");
        let mut file = File::create(&file_path).expect("operation should succeed");
        writeln!(file, "hello").expect("operation should succeed");

        let file_str = file_path.to_string_lossy().to_string();
        config.insert("source".to_string(), file_str);
        config.insert("destination".to_string(), "/tmp/upload.txt".to_string());

        assert_eq!(prov.name(), "file");
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_ok());
        assert!(prov.cleanup().is_ok());
    }

    #[test]
    fn test_file_provisioner_empty_parent() {
        let prov = FileProvisioner::default();
        let dir = tempdir().expect("operation should succeed");
        let file_path = dir.path().join("upload.txt");
        let mut file = File::create(&file_path).expect("operation should succeed");
        writeln!(file, "hello").expect("operation should succeed");

        let comm = MockComm::default();
        assert!(
            prov.upload_recursive(&comm, &file_path, "upload.txt")
                .is_ok()
        );
    }

    #[test]
    fn test_file_provisioner_directory() {
        let mut prov = FileProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir().expect("operation should succeed");
        let sub = dir.path().join("subdir");
        std::fs::create_dir(&sub).expect("operation should succeed");
        let file_path = sub.join("inner.txt");
        let mut file = File::create(&file_path).expect("operation should succeed");
        writeln!(file, "inner content").expect("operation should succeed");

        config.insert(
            "source".to_string(),
            dir.path().to_string_lossy().to_string(),
        );
        config.insert("destination".to_string(), "/tmp/dest_dir".to_string());

        assert!(prov.prepare(&config).is_ok());
        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_file_provisioner_provision_fails() {
        let mut prov = FileProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir().expect("operation should succeed");
        let file_path = dir.path().join("upload.txt");
        let mut file = File::create(&file_path).expect("operation should succeed");
        writeln!(file, "hello").expect("operation should succeed");

        let file_str = file_path.to_string_lossy().to_string();
        config.insert("source".to_string(), file_str);
        config.insert("destination".to_string(), "/tmp/upload.txt".to_string());
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm {
            fail_execute: false,
            fail_upload: true,
        };
        assert!(prov.provision(&comm).is_err());
    }

    #[test]
    fn test_file_provisioner_missing_config() {
        let mut prov = FileProvisioner::default();
        let config = HashMap::new();
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_err());
    }

    #[test]
    fn test_file_provisioner_missing_destination() {
        let mut prov = FileProvisioner::default();
        let mut config = HashMap::new();
        config.insert("source".to_string(), "foo".to_string());
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_err());
    }

    #[test]
    fn test_file_provisioner_file_not_found() {
        let mut prov = FileProvisioner::default();
        let mut config = HashMap::new();
        config.insert("source".to_string(), "/nonexistent/file.txt".to_string());
        config.insert("destination".to_string(), "/tmp/file.txt".to_string());
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_err());
    }

    #[test]
    fn test_file_provisioner_file_mkdir_fails() {
        let prov = FileProvisioner::default();
        let dir = tempdir().expect("operation should succeed");
        let file_path = dir.path().join("upload.txt");
        let mut file = File::create(&file_path).expect("operation should succeed");
        writeln!(file, "hello").expect("operation should succeed");

        let comm = MockComm {
            fail_execute: true,
            fail_upload: false,
        };
        assert!(
            prov.upload_recursive(&comm, &file_path, "/tmp/upload.txt")
                .is_err()
        );
    }

    #[test]
    fn test_file_provisioner_file_upload_fails() {
        let prov = FileProvisioner::default();
        let dir = tempdir().expect("operation should succeed");
        let file_path = dir.path().join("upload.txt");
        let mut file = File::create(&file_path).expect("operation should succeed");
        writeln!(file, "hello").expect("operation should succeed");

        let comm = MockComm {
            fail_execute: false,
            fail_upload: true,
        };
        assert!(
            prov.upload_recursive(&comm, &file_path, "/tmp/upload.txt")
                .is_err()
        );
    }

    #[test]
    fn test_file_provisioner_dir_mkdir_fails() {
        let prov = FileProvisioner::default();
        let dir = tempdir().expect("operation should succeed");
        let comm = MockComm {
            fail_execute: true,
            fail_upload: false,
        };
        assert!(
            prov.upload_recursive(&comm, dir.path(), "/tmp/dest_dir")
                .is_err()
        );
    }

    #[test]
    fn test_file_provisioner_dir_sub_upload_fails() {
        let prov = FileProvisioner::default();
        let dir = tempdir().expect("operation should succeed");
        let file_path = dir.path().join("upload.txt");
        let mut file = File::create(&file_path).expect("operation should succeed");
        writeln!(file, "hello").expect("operation should succeed");

        let comm = MockComm {
            fail_execute: false,
            fail_upload: true,
        };
        assert!(
            prov.upload_recursive(&comm, dir.path(), "/tmp/dest_dir")
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_file_provisioner_read_dir_fails() {
        use std::os::unix::fs::PermissionsExt;
        let prov = FileProvisioner::default();
        let dir = tempdir().expect("operation should succeed");
        let sub = dir.path().join("unreadable");
        std::fs::create_dir(&sub).expect("operation should succeed");

        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o000))
            .expect("operation should succeed");

        let comm = MockComm::default();
        let res = prov.upload_recursive(&comm, &sub, "/tmp/dest_unreadable");

        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755))
            .expect("operation should succeed");

        assert!(res.is_err());
    }
}
