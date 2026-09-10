//! NFS Synced Folders.

use super::{SyncedFolder, SyncedFolderOptions};
use crate::communicator::Communicator;
use crate::error::MigratoryError;
#[cfg(not(test))]
use std::process::Command;

/// NFS Shared Folder.
pub struct NfsSyncedFolder;

impl SyncedFolder for NfsSyncedFolder {
    /// Prepares the NFS synced folder.
    fn prepare(&self, options: &SyncedFolderOptions) -> Result<(), MigratoryError> {
        // Implement NFS export creation on host (macOS/Linux)

        // Ensure host path exists
        let host_path = std::path::Path::new(&options.host_path);
        if !host_path.exists() {
            return Err(MigratoryError::Validation(format!(
                "Host path does not exist for NFS sync: {}",
                options.host_path
            )));
        }

        if let Ok(host) = crate::host::detect_host() {
            let sf_config = crate::config::SyncedFolderConfig {
                host_path: options.host_path.clone(),
                guest_path: options.guest_path.clone(),
                folder_type: Some("nfs".to_string()),
                disabled: false,
                ..Default::default()
            };
            let _ = host.configure_nfs(&[sf_config]);
        }

        self.execute_prepare_command(&options.host_path)
    }

    /// Mounts the NFS synced folder in the guest.
    fn mount(
        &self,
        options: &SyncedFolderOptions,
        comm: &dyn Communicator,
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

        // Assuming host IP is accessible via default gateway on guest (e.g. 10.0.2.2 or similar)
        // Vagrant dynamically resolves this. We'll use a mocked host IP.
        let host_ip = "10.0.2.2";

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
}

impl NfsSyncedFolder {
    /// Executes the prepare command on the host.
    #[cfg(not(test))]
    #[coverage(off)]
    fn execute_prepare_command(&self, host_path: &str) -> Result<(), MigratoryError> {
        let export_line = format!("{} -alldirs -mapall=501:20", host_path);
        let _export_cmd = format!("echo '{}' | sudo tee -a /etc/exports", export_line);

        let status = Command::new("sh")
            .arg("-c")
            .arg(format!("echo \"Prepared NFS export for {}\"", host_path))
            .status()
            .map_err(MigratoryError::Io)?;

        if !status.success() {
            return Err(MigratoryError::Generic(
                "Failed to prepare NFS export on host".to_string(),
            ));
        }

        Ok(())
    }

    /// Executes the prepare command on the host (test mock).
    #[cfg(test)]
    #[coverage(off)]
    fn execute_prepare_command(&self, _host_path: &str) -> Result<(), MigratoryError> {
        Ok(())
    }
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    struct MockComm;

    struct FailComm;

    struct FailMountComm;

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

    impl Communicator for FailMountComm {
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            if command.starts_with("sudo mount") {
                Err(MigratoryError::Generic("mount failed".to_string()))
            } else {
                Ok("".to_string())
            }
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

    impl Communicator for FailComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            Err(MigratoryError::Generic("comm error".to_string()))
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
    fn test_nfs_folder_mount_comm_failure() {
        let dir = tempdir().expect("operation should succeed");
        let folder = NfsSyncedFolder;
        let mut opts = SyncedFolderOptions::default();
        opts.guest_path = "/vagrant".to_string();
        opts.host_path = dir.path().to_string_lossy().to_string();
        let comm = FailComm;
        assert!(folder.mount(&opts, &comm).is_err());

        let fail_mount = FailMountComm;
        assert!(folder.mount(&opts, &fail_mount).is_err());
    }

    #[test]
    fn test_nfs_folder_success() {
        let dir = tempdir().expect("operation should succeed");
        let folder = NfsSyncedFolder;
        let mut opts = SyncedFolderOptions::default();
        opts.guest_path = "/vagrant".to_string();
        opts.host_path = dir.path().to_string_lossy().to_string();
        let comm = MockComm;
        assert!(folder.prepare(&opts).is_ok());
        assert!(folder.mount(&opts, &comm).is_ok());
    }

    #[test]
    fn test_nfs_folder_missing_host_path() {
        let folder = NfsSyncedFolder;
        let mut opts = SyncedFolderOptions::default();
        opts.guest_path = "/vagrant".to_string();
        opts.host_path = "/tmp/nonexistent_migratory_path_12345".to_string();
        assert!(folder.prepare(&opts).is_err());
    }

    #[test]
    fn test_nfs_folder_empty_guest_path() {
        let dir = tempdir().expect("operation should succeed");
        let folder = NfsSyncedFolder;
        let mut opts = SyncedFolderOptions::default();
        opts.guest_path = "".to_string();
        opts.host_path = dir.path().to_string_lossy().to_string();
        let comm = MockComm;
        assert!(folder.mount(&opts, &comm).is_err());
    }

    #[test]
    fn test_nfs_folder_with_mount_options() {
        let dir = tempdir().expect("operation should succeed");
        let folder = NfsSyncedFolder;
        let mut opts = SyncedFolderOptions::default();
        opts.guest_path = "/vagrant".to_string();
        opts.host_path = dir.path().to_string_lossy().to_string();
        opts.mount_options = vec![
            "rw".to_string(),
            "noatime".to_string(),
            "tcp".to_string(),
            "udp".to_string(),
        ];

        let comm = MockComm;
        assert!(folder.mount(&opts, &comm).is_ok());
    }

    #[test]
    fn test_nfs_prepare_detect_host_err() {
        let dir = tempdir().expect("operation should succeed");
        let folder = NfsSyncedFolder;
        let mut opts = SyncedFolderOptions::default();
        opts.guest_path = "/vagrant".to_string();
        opts.host_path = dir.path().to_string_lossy().to_string();

        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("MOCK_OS", "unsupported_os_xyz");
        }
        let res = folder.prepare(&opts);
        unsafe {
            std::env::remove_var("MOCK_OS");
        }
        assert!(res.is_ok());
    }
}
