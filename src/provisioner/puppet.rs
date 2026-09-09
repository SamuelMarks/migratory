//! Puppet provisioner implementation.

use super::Provisioner;
use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::collections::HashMap;

/// Puppet provisioner.
pub struct PuppetProvisioner {
    manifest_file: Option<String>,
    manifests_path: Option<String>,
    module_path: Option<String>,
    options: Option<String>,
    puppet_server: Option<String>,
    hiera_config_path: Option<String>,
}

impl PuppetProvisioner {
    /// Creates a new puppet provisioner.
    pub fn new() -> Self {
        Self {
            manifest_file: None,
            manifests_path: None,
            module_path: None,
            options: None,
            puppet_server: None,
            hiera_config_path: None,
        }
    }
}

impl Default for PuppetProvisioner {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to package and upload a directory to the guest.
///
/// # Arguments
///
/// * `comm` - Communicator instance.
/// * `local_dir` - Local directory to archive and upload.
/// * `remote_dir` - Remote destination directory.
///
/// # Errors
///
/// Returns a `MigratoryError` if archive creation, upload, or remote extraction fails.
#[coverage(off)]
fn upload_dir(
    comm: &dyn Communicator,
    local_dir: &str,
    remote_dir: &str,
) -> Result<(), MigratoryError> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let tar_path = std::env::temp_dir().join(format!("migratory_upload_{}.tar", ts));

    let file = std::fs::File::create(&tar_path).map_err(MigratoryError::Io)?;
    let mut builder = tar::Builder::new(file);
    builder
        .append_dir_all(".", local_dir)
        .map_err(MigratoryError::Io)?;
    builder.into_inner().map_err(MigratoryError::Io)?;

    let remote_tar_path = format!("/tmp/migratory_puppet_{}.tar", ts);

    comm.upload(&tar_path, &remote_tar_path)?;

    comm.execute(&format!(
        "mkdir -p '{}' && tar -xf '{}' -C '{}'",
        remote_dir, remote_tar_path, remote_dir
    ))?;
    let _ = comm.execute(&format!("rm -f '{}'", remote_tar_path));
    let _ = std::fs::remove_file(&tar_path);

    Ok(())
}

impl Provisioner for PuppetProvisioner {
    fn name(&self) -> &str {
        "puppet"
    }

    fn prepare(&mut self, config: &HashMap<String, String>) -> Result<(), MigratoryError> {
        self.manifest_file = config.get("manifest_file").cloned();
        self.manifests_path = config.get("manifests_path").cloned();
        self.module_path = config.get("module_path").cloned();
        self.options = config.get("options").cloned();
        self.puppet_server = config.get("puppet_server").cloned();
        self.hiera_config_path = config.get("hiera_config_path").cloned();
        Ok(())
    }

    fn provision(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        if let Some(server) = &self.puppet_server {
            let mut command = format!(
                "sudo puppet agent -t --server '{}'",
                server.replace('\'', "'\\''")
            );
            if let Some(options) = &self.options {
                command.push_str(&format!(" {}", options));
            }
            let _ = comm.execute(&command);
            return Ok(());
        }

        let manifest_file = match &self.manifest_file {
            Some(mf) => mf,
            None => {
                return Err(MigratoryError::Validation(
                    "Puppet provisioner requires a 'manifest_file' when not in agent mode"
                        .to_string(),
                ));
            }
        };

        let remote_manifests_path = "/tmp/vagrant-puppet/manifests";
        let remote_modules_path = "/tmp/vagrant-puppet/modules";

        if let Some(manifests_path) = &self.manifests_path
            && std::path::Path::new(manifests_path).exists()
        {
            upload_dir(comm, manifests_path, remote_manifests_path)?;
        }

        let mut command = "sudo puppet apply".to_string();

        if let Some(module_path) = &self.module_path {
            if std::path::Path::new(module_path).exists() {
                upload_dir(comm, module_path, remote_modules_path)?;
                command.push_str(&format!(
                    " --modulepath '{}'",
                    remote_modules_path.replace('\'', "'\\''")
                ));
            } else {
                command.push_str(&format!(
                    " --modulepath '{}'",
                    module_path.replace('\'', "'\\''")
                ));
            }
        }

        if let Some(hiera) = &self.hiera_config_path {
            let hp = std::path::Path::new(hiera);
            if hp.exists() {
                let _ = comm.execute("mkdir -p /tmp/vagrant-puppet");
                comm.upload(hp, "/tmp/vagrant-puppet/hiera.yaml")?;
                command.push_str(" --hiera_config='/tmp/vagrant-puppet/hiera.yaml'");
            }
        }

        if let Some(options) = &self.options {
            command.push_str(&format!(" {}", options));
        }

        let full_manifest_path = if self.manifests_path.is_some() {
            format!("{}/{}", remote_manifests_path, manifest_file)
        } else {
            manifest_file.clone()
        };

        command.push_str(&format!(" '{}'", full_manifest_path.replace('\'', "'\\''")));

        comm.execute(&command)?;
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
    use std::path::Path;
    use std::time::Duration;

    struct MockComm;

    impl Communicator for MockComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            Ok("".to_string())
        }
        #[coverage(off)]
        fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
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
    fn test_puppet_provisioner_success() {
        let mut prov = PuppetProvisioner::default();
        let mut config = HashMap::new();
        config.insert("manifest_file".to_string(), "default.pp".to_string());
        config.insert("manifests_path".to_string(), "/tmp/manifests".to_string());
        config.insert("module_path".to_string(), "/tmp/modules".to_string());
        config.insert("options".to_string(), "--verbose".to_string());

        assert_eq!(prov.name(), "puppet");
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
        assert!(prov.cleanup().is_ok());
    }

    #[test]
    fn test_puppet_provisioner_no_manifests_path() {
        let mut prov = PuppetProvisioner::default();
        let mut config = HashMap::new();
        config.insert("manifest_file".to_string(), "site.pp".to_string());
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_puppet_provisioner_missing_manifest_file() {
        let mut prov = PuppetProvisioner::default();
        let config = HashMap::new();

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_err());
    }

    #[test]
    fn test_puppet_provisioner_agent_mode() {
        let mut prov = PuppetProvisioner::default();
        let mut config = HashMap::new();
        config.insert(
            "puppet_server".to_string(),
            "puppet.example.com".to_string(),
        );
        config.insert("options".to_string(), "--debug".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        // Should succeed even without manifest_file
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_puppet_provisioner_agent_mode_no_options() {
        let mut prov = PuppetProvisioner::default();
        let mut config = HashMap::new();
        config.insert(
            "puppet_server".to_string(),
            "puppet.example.com".to_string(),
        );

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        // Should succeed
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_puppet_provisioner_upload_paths() {
        let temp_dir = tempfile::tempdir().expect("tempdir failed");
        let manifests_path = temp_dir.path().join("manifests");
        std::fs::create_dir_all(&manifests_path).expect("create_dir failed");
        let manifest_file = manifests_path.join("default.pp");
        std::fs::write(&manifest_file, "notify { 'test': }").expect("write failed");

        let modules_path = temp_dir.path().join("modules");
        std::fs::create_dir_all(&modules_path).expect("create_dir failed");

        let hiera_file = temp_dir.path().join("hiera.yaml");
        std::fs::write(&hiera_file, "---\n:backends:\n  - yaml").expect("write failed");

        let mut prov = PuppetProvisioner::default();
        let mut config = HashMap::new();
        config.insert("manifest_file".to_string(), "default.pp".to_string());
        config.insert(
            "manifests_path".to_string(),
            manifests_path.to_string_lossy().to_string(),
        );
        config.insert(
            "module_path".to_string(),
            modules_path.to_string_lossy().to_string(),
        );
        config.insert(
            "hiera_config_path".to_string(),
            hiera_file.to_string_lossy().to_string(),
        );
        config.insert("options".to_string(), "--verbose".to_string());

        assert!(prov.prepare(&config).is_ok());

        struct TrackUploadsComm;
        impl Communicator for TrackUploadsComm {
            fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
                Ok("".to_string())
            }
            fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn download(
                &self,
                _remote_path: &str,
                _local_path: &Path,
            ) -> Result<(), MigratoryError> {
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

        let comm = TrackUploadsComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_puppet_provisioner_error_paths() {
        struct FailingComm {
            fail_upload: bool,
            fail_execute: bool,
        }
        impl Communicator for FailingComm {
            #[coverage(off)]
            fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
                if self.fail_execute {
                    Err(MigratoryError::Generic("execute failed".to_string()))
                } else {
                    Ok("".to_string())
                }
            }
            #[coverage(off)]
            fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
                if self.fail_upload {
                    Err(MigratoryError::Generic("upload failed".to_string()))
                } else {
                    Ok(())
                }
            }
            #[coverage(off)]
            fn download(
                &self,
                _remote_path: &str,
                _local_path: &Path,
            ) -> Result<(), MigratoryError> {
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

        let temp_dir = tempfile::tempdir().expect("tempdir failed");
        let manifests_path = temp_dir.path().join("manifests");
        std::fs::create_dir_all(&manifests_path).expect("create_dir failed");
        let modules_path = temp_dir.path().join("modules");
        std::fs::create_dir_all(&modules_path).expect("create_dir failed");
        let hiera_file = temp_dir.path().join("hiera.yaml");
        std::fs::write(&hiera_file, "---\n").expect("write failed");

        // 1. manifests_path upload fails
        let mut prov = PuppetProvisioner::default();
        let mut config = HashMap::new();
        config.insert("manifest_file".to_string(), "default.pp".to_string());
        config.insert(
            "manifests_path".to_string(),
            manifests_path.to_string_lossy().to_string(),
        );
        assert!(prov.prepare(&config).is_ok());
        let comm_upload_fail = FailingComm {
            fail_upload: true,
            fail_execute: false,
        };
        assert!(prov.provision(&comm_upload_fail).is_err());

        // 2. module_path upload fails (without manifests_path)
        let mut prov2 = PuppetProvisioner::default();
        let mut config2 = HashMap::new();
        config2.insert("manifest_file".to_string(), "default.pp".to_string());
        config2.insert(
            "module_path".to_string(),
            modules_path.to_string_lossy().to_string(),
        );
        assert!(prov2.prepare(&config2).is_ok());
        assert!(prov2.provision(&comm_upload_fail).is_err());

        // 3. hiera_config_path upload fails
        let mut prov3 = PuppetProvisioner::default();
        let mut config3 = HashMap::new();
        config3.insert("manifest_file".to_string(), "default.pp".to_string());
        config3.insert(
            "hiera_config_path".to_string(),
            hiera_file.to_string_lossy().to_string(),
        );
        assert!(prov3.prepare(&config3).is_ok());
        assert!(prov3.provision(&comm_upload_fail).is_err());

        // 4. hiera_config_path does not exist
        let mut prov4 = PuppetProvisioner::default();
        let mut config4 = HashMap::new();
        config4.insert("manifest_file".to_string(), "default.pp".to_string());
        config4.insert(
            "hiera_config_path".to_string(),
            "/nonexistent/hiera.yaml".to_string(),
        );
        assert!(prov4.prepare(&config4).is_ok());
        let comm_ok = FailingComm {
            fail_upload: false,
            fail_execute: false,
        };
        assert!(prov4.provision(&comm_ok).is_ok());

        // 5. comm.execute fails
        let comm_exec_fail = FailingComm {
            fail_upload: false,
            fail_execute: true,
        };
        assert!(prov4.provision(&comm_exec_fail).is_err());
    }
}
