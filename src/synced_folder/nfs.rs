//! NFS Synced Folders.

use super::{SyncedFolder, SyncedFolderOptions};
use crate::communicator::Communicator;
use crate::error::MigratoryError;

/// NFS Shared Folder.
pub struct NfsSyncedFolder;

impl SyncedFolder for NfsSyncedFolder {
    /// Prepares the NFS synced folder.
    fn prepare(&self, options: &SyncedFolderOptions) -> Result<(), MigratoryError> {
        // Ensure host path exists
        let host_path = std::path::Path::new(&options.host_path);
        if !host_path.exists() {
            return Err(MigratoryError::Validation(format!(
                "Host path does not exist for NFS sync: {}",
                options.host_path
            )));
        }

        self.execute_prepare_command(&options.host_path)
    }

    /// Mounts the NFS synced folder in the guest.
    fn mount(
        &self,
        options: &SyncedFolderOptions,
        comm: &dyn Communicator,
    ) -> Result<(), MigratoryError> {
        self.mount_with_host(options, comm, crate::host::detect_host().ok().as_deref())
    }
}

impl NfsSyncedFolder {
    /// Mounts the NFS synced folder in the guest using an optional injected [`crate::host::Host`].
    ///
    /// # Arguments
    ///
    /// * `options` - Synced folder options.
    /// * `comm` - Guest communicator.
    /// * `host` - Optional host implementation used to resolve fallback host IP.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if mounting fails.
    pub fn mount_with_host(
        &self,
        options: &SyncedFolderOptions,
        comm: &dyn Communicator,
        host: Option<&dyn crate::host::Host>,
    ) -> Result<(), MigratoryError> {
        if options.guest_path.is_empty() {
            return Err(MigratoryError::Validation(
                "Guest path cannot be empty for NFS mount".to_string(),
            ));
        }

        // Install NFS guest client utilities if needed
        let install_nfs_client = "which mount.nfs >/dev/null 2>&1 || (which apt-get >/dev/null 2>&1 && sudo apt-get update -y && sudo apt-get install -y nfs-common) || (which yum >/dev/null 2>&1 && sudo yum install -y nfs-utils) || true";
        let _ = comm.execute(install_nfs_client);

        let mkdir_cmd = format!(
            "sudo mkdir -p '{}'",
            options.guest_path.replace('\'', "'\\''")
        );
        let _ = comm.execute(&mkdir_cmd)?;

        // Dynamically resolve host IP from guest routing table or host resolver
        let host_ip = match comm.execute("ip route show default | awk '{print $3}' 2>/dev/null || route -n get default | awk '/gateway/{print $2}'") {
            Ok(output) if !output.trim().is_empty() => output.trim().to_string(),
            _ => host
                .and_then(|h| h.resolve_host_ip().ok())
                .unwrap_or_else(|| "10.0.2.2".to_string()),
        };

        let proto = if options.mount_options.iter().any(|o| o.contains("tcp")) {
            "tcp"
        } else {
            "udp"
        };

        let mut mount_opts = format!("vers=3,{},nolock,rw", proto); // Typical defaults
        for opt in &options.mount_options {
            if opt != "tcp" && opt != "udp" {
                mount_opts.push_str(&format!(",{}", opt));
            }
        }

        let mount_cmd = format!(
            "sudo mount -t nfs -o {} {}:'{}' '{}'",
            mount_opts,
            host_ip,
            options.host_path.replace('\'', "'\\''"),
            options.guest_path.replace('\'', "'\\''")
        );

        let _ = comm.execute(&mount_cmd)?;

        Ok(())
    }

    /// Executes the prepare command on the host by delegating to the detected host implementation.
    ///
    /// # Arguments
    ///
    /// * `host_path` - Local host directory to export via NFS.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if host detection or NFS export configuration fails.
    pub fn execute_prepare_command(&self, host_path: &str) -> Result<(), MigratoryError> {
        let host = crate::host::detect_host()?;
        self.execute_prepare_command_with_host(host_path, host.as_ref())
    }

    /// Executes the NFS prepare command on the provided [`crate::host::Host`] implementation.
    ///
    /// # Arguments
    ///
    /// * `host_path` - Local host directory to export via NFS.
    /// * `host` - Host implementation to configure NFS exports on.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if NFS export configuration fails.
    pub fn execute_prepare_command_with_host(
        &self,
        host_path: &str,
        host: &dyn crate::host::Host,
    ) -> Result<(), MigratoryError> {
        let sf_config = crate::config::SyncedFolderConfig {
            host_path: std::path::PathBuf::from(host_path),
            guest_path: std::path::PathBuf::from("/vagrant"),
            folder_type: Some(crate::config::SyncedFolderType::Nfs),
            disabled: false,
            ..Default::default()
        };
        host.configure_nfs(&[sf_config])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    /// No-op mock communicator for NFS synced folder tests.
    struct MockComm;

    /// Failing mock communicator for NFS synced folder error tests.
    struct FailComm;

    /// Mock communicator that fails only `sudo mount` commands.
    struct FailMountComm;

    /// Mock communicator returning a dynamic gateway IP for `awk` commands.
    struct DynamicGatewayComm;

    impl Communicator for DynamicGatewayComm {
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            if command.contains("awk") {
                Ok("192.168.56.1".to_string())
            } else {
                Ok(String::new())
            }
        }
    }

    impl Communicator for MockComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            Ok(String::new())
        }
    }

    impl Communicator for FailMountComm {
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            if command.starts_with("sudo mount") {
                Err(MigratoryError::Generic("mount failed".to_string()))
            } else {
                Ok(String::new())
            }
        }
    }

    impl Communicator for FailComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            Err(MigratoryError::Generic("comm error".to_string()))
        }
    }

    #[test]
    fn test_nfs_folder_mount_comm_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let folder = NfsSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = FailComm;
        assert!(folder.mount(&opts, &comm).is_err());

        let fail_mount = FailMountComm;
        assert!(folder.mount(&opts, &fail_mount).is_err());
        Ok(())
    }

    #[test]
    fn test_nfs_folder_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let folder = NfsSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = MockComm;
        assert!(folder.prepare(&opts).is_ok());
        assert!(folder.mount(&opts, &comm).is_ok());
        Ok(())
    }

    #[test]
    fn test_nfs_folder_dynamic_gateway() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let folder = NfsSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = DynamicGatewayComm;
        assert!(folder.mount(&opts, &comm).is_ok());
        Ok(())
    }

    #[test]
    fn test_nfs_folder_missing_host_path() {
        let folder = NfsSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: "/tmp/nonexistent_migratory_path_12345".to_string(),
            ..Default::default()
        };
        assert!(folder.prepare(&opts).is_err());
    }

    #[test]
    fn test_nfs_folder_empty_guest_path() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let folder = NfsSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: String::new(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = MockComm;
        assert!(folder.mount(&opts, &comm).is_err());
        Ok(())
    }

    #[test]
    fn test_nfs_folder_with_mount_options() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let folder = NfsSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            mount_options: vec![
                "rw".to_string(),
                "noatime".to_string(),
                "tcp".to_string(),
                "udp".to_string(),
            ],
            ..Default::default()
        };

        let comm = MockComm;
        assert!(folder.mount(&opts, &comm).is_ok());
        Ok(())
    }

    /// Failing mock host used to test NFS preparation and IP resolution failures.
    struct FailingHost;
    impl crate::host::Host for FailingHost {
        fn configure_nfs(
            &self,
            _folders: &[crate::config::SyncedFolderConfig],
        ) -> Result<(), MigratoryError> {
            Err(MigratoryError::Generic("Mock prepare error".to_string()))
        }
        fn resolve_host_ip(&self) -> Result<String, MigratoryError> {
            Err(MigratoryError::Generic("no ip".to_string()))
        }
    }

    #[test]
    fn test_nfs_prepare_detect_host_err() {
        let folder = NfsSyncedFolder;
        let failing_host = FailingHost;
        assert!(
            folder
                .execute_prepare_command_with_host("/tmp", &failing_host)
                .is_err()
        );
    }

    #[test]
    fn test_nfs_execute_prepare_command_mock_err() {
        let folder = NfsSyncedFolder;
        let failing_host = FailingHost;
        let res = folder.execute_prepare_command_with_host("/tmp", &failing_host);
        assert!(res.is_err());
    }

    #[test]
    fn test_nfs_mount_unsupported_host_fallback() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let folder = NfsSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let comm = MockComm;
        let res = folder.mount_with_host(&opts, &comm, None);
        assert!(res.is_ok());

        let failing_host = FailingHost;
        let res2 = folder.mount_with_host(&opts, &comm, Some(&failing_host));
        assert!(res2.is_ok());
        Ok(())
    }
}
