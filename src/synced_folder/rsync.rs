//! Rsync Synced Folders.

use super::{SyncedFolder, SyncedFolderOptions};
use crate::communicator::Communicator;
use crate::error::MigratoryError;
#[cfg(not(test))]
use std::process::Command;

/// Rsync Shared Folder.
pub struct RsyncSyncedFolder;

impl SyncedFolder for RsyncSyncedFolder {
    /// Prepares the Rsync synced folder.
    fn prepare(&self, _options: &SyncedFolderOptions) -> Result<(), MigratoryError> {
        // Rsync doesn't require daemon prep on the host, just client execution.
        Ok(())
    }

    /// Mounts the Rsync synced folder in the guest.
    fn mount(
        &self,
        options: &SyncedFolderOptions,
        comm: &dyn Communicator,
    ) -> Result<(), MigratoryError> {
        let host_path = std::path::Path::new(&options.host_path);
        if !host_path.exists() {
            return Err(MigratoryError::Validation(format!(
                "Host path does not exist for Rsync: {}",
                options.host_path
            )));
        }

        if options.guest_path.is_empty() {
            return Err(MigratoryError::Validation(
                "Guest path cannot be empty for Rsync".to_string(),
            ));
        }

        let mkdir_cmd = format!(
            "sudo mkdir -p '{}'",
            options.guest_path.replace('\'', "'\\''")
        );
        let _ = comm.execute(&mkdir_cmd)?;

        let should_chown = !options
            .mount_options
            .iter()
            .any(|o| o == "chown=false" || o == "rsync__chown=false" || o == "no_chown");

        if should_chown {
            let owner = options.owner.as_deref().unwrap_or("vagrant");
            let group = options.group.as_deref().unwrap_or("vagrant");
            let chown_cmd = format!(
                "sudo chown -R {}:{} '{}'",
                owner,
                group,
                options.guest_path.replace('\'', "'\\''")
            );
            let _ = comm.execute(&chown_cmd)?;
        }

        self.execute_mount_command(options)
    }
}

impl RsyncSyncedFolder {
    /// Builds the rsync command string with safe SSH arguments, excludes, and path normalization.
    ///
    /// # Arguments
    ///
    /// * `options` - Options containing host path, guest path, and mount options.
    ///
    /// # Returns
    ///
    /// Returns the built rsync shell command.
    pub fn build_rsync_command(options: &SyncedFolderOptions) -> String {
        // Handle Windows-to-Linux path conversions for rsync.exe
        let mut host_path = options.host_path.clone();
        #[cfg(windows)]
        {
            // Convert C:\path\to\something -> /c/path/to/something for msys2/cygwin rsync
            host_path = host_path.replace('\\', "/");
            if host_path.len() > 1
                && host_path.chars().nth(1) == Some(':')
                && let Some(c) = host_path.chars().next()
            {
                let drive_letter = c.to_lowercase().to_string();
                host_path = format!("/{}{}", drive_letter, &host_path[2..]);
            }
        }

        // Ensure host path ends with a trailing slash so rsync syncs contents
        if !host_path.ends_with('/') {
            host_path.push('/');
        }

        let mut ssh_host = "127.0.0.1".to_string();
        let mut ssh_port = "2222".to_string();
        let mut ssh_user = "vagrant".to_string();
        let mut ssh_key = None;
        let mut extra_rsync_args = Vec::new();
        let mut excludes = vec![".vagrant/".to_string(), ".git/".to_string()];
        let mut includes = Vec::new();
        let mut delete = true;

        for opt in &options.mount_options {
            if let Some(val) = opt.strip_prefix("ssh_host=") {
                ssh_host = val.to_string();
            } else if let Some(val) = opt.strip_prefix("ssh_port=") {
                ssh_port = val.to_string();
            } else if let Some(val) = opt.strip_prefix("ssh_user=") {
                ssh_user = val.to_string();
            } else if let Some(val) = opt.strip_prefix("ssh_key=") {
                ssh_key = Some(val.to_string());
            } else if let Some(val) = opt.strip_prefix("exclude=") {
                excludes.push(val.to_string());
            } else if let Some(val) = opt.strip_prefix("rsync__exclude=") {
                excludes.push(val.to_string());
            } else if let Some(val) = opt.strip_prefix("include=") {
                includes.push(val.to_string());
            } else if let Some(val) = opt.strip_prefix("rsync__args=") {
                extra_rsync_args.push(val.to_string());
            } else if opt == "no_delete" || opt == "delete=false" {
                delete = false;
            }
        }

        let mut ssh_cmd = format!(
            "ssh -p {} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null",
            ssh_port
        );
        if let Some(key) = ssh_key {
            ssh_cmd.push_str(&format!(" -i '{}'", key.replace('\'', "'\\''")));
        }

        let mut cmd = "rsync -az".to_string();
        if delete {
            cmd.push_str(" --delete");
        }
        for exc in &excludes {
            cmd.push_str(&format!(" --exclude '{}'", exc.replace('\'', "'\\''")));
        }
        for inc in &includes {
            cmd.push_str(&format!(" --include '{}'", inc.replace('\'', "'\\''")));
        }
        for arg in &extra_rsync_args {
            cmd.push_str(&format!(" {}", arg));
        }

        cmd.push_str(&format!(
            " -e '{}' '{}' {}@{}:'{}'",
            ssh_cmd,
            host_path,
            ssh_user,
            ssh_host,
            options.guest_path.replace('\'', "'\\''")
        ));

        cmd
    }

    /// Executes the mount command on the host.
    #[cfg(not(test))]
    #[coverage(off)]
    fn execute_mount_command(&self, options: &SyncedFolderOptions) -> Result<(), MigratoryError> {
        let cmd_str = Self::build_rsync_command(options);

        let status = Command::new("sh")
            .arg("-c")
            .arg(&cmd_str)
            .status()
            .map_err(MigratoryError::Io)?;

        if !status.success() {
            return Err(MigratoryError::Generic("Failed to run rsync".to_string()));
        }

        Ok(())
    }

    /// Executes the mount command on the host (test mock).
    #[cfg(test)]
    #[coverage(off)]
    fn execute_mount_command(&self, _options: &SyncedFolderOptions) -> Result<(), MigratoryError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    #[derive(Default)]
    struct MockComm {
        fail_first_execute: bool,
        fail_second_execute: bool,
        execute_count: Cell<usize>,
    }

    impl Communicator for MockComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            let count = self.execute_count.get();
            self.execute_count.set(count + 1);
            if count == 0 && self.fail_first_execute {
                return Err(MigratoryError::Generic("mkdir error".to_string()));
            }
            if count == 1 && self.fail_second_execute {
                return Err(MigratoryError::Generic("chown error".to_string()));
            }
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
    fn test_rsync_folder_mount_comm_failure() {
        let dir = tempdir().expect("operation should succeed");
        let folder = RsyncSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = MockComm {
            fail_first_execute: true,
            ..Default::default()
        };
        assert!(folder.mount(&opts, &comm).is_err());
    }

    #[test]
    fn test_rsync_folder_chown_failure() {
        let dir = tempdir().expect("operation should succeed");
        let folder = RsyncSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = MockComm {
            fail_second_execute: true,
            ..Default::default()
        };
        assert!(folder.mount(&opts, &comm).is_err());
    }

    #[test]
    fn test_rsync_folder_success_with_chown_defaults() {
        let dir = tempdir().expect("operation should succeed");
        let folder = RsyncSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = MockComm::default();
        assert!(folder.prepare(&opts).is_ok());
        assert!(folder.mount(&opts, &comm).is_ok());
    }

    #[test]
    fn test_rsync_folder_success_with_chown_custom() {
        let dir = tempdir().expect("operation should succeed");
        let folder = RsyncSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            owner: Some("appuser".to_string()),
            group: Some("appgroup".to_string()),
            ..Default::default()
        };
        let comm = MockComm::default();
        assert!(folder.mount(&opts, &comm).is_ok());
    }

    #[test]
    fn test_rsync_folder_chown_disabled_variations() {
        let dir = tempdir().expect("operation should succeed");
        let folder = RsyncSyncedFolder;

        for disabled_opt in ["chown=false", "rsync__chown=false", "no_chown"] {
            let opts = SyncedFolderOptions {
                guest_path: "/vagrant".to_string(),
                host_path: dir.path().to_string_lossy().to_string(),
                mount_options: vec![disabled_opt.to_string()],
                ..Default::default()
            };
            let comm = MockComm::default();
            assert!(folder.mount(&opts, &comm).is_ok());
            assert_eq!(comm.execute_count.get(), 1);
        }
    }

    #[test]
    fn test_build_rsync_command_full() {
        let opts = SyncedFolderOptions {
            guest_path: "/var/www".to_string(),
            host_path: "/home/user/app".to_string(),
            mount_options: vec![
                "ssh_host=192.168.56.10".to_string(),
                "ssh_port=2200".to_string(),
                "ssh_user=appuser".to_string(),
                "ssh_key=/path/to/key.pem".to_string(),
                "exclude=*.log".to_string(),
                "rsync__exclude=node_modules/".to_string(),
                "include=*.rs".to_string(),
                "rsync__args=--bwlimit=1000".to_string(),
                "delete=false".to_string(),
                "ignored_option".to_string(),
            ],
            ..Default::default()
        };

        let cmd = RsyncSyncedFolder::build_rsync_command(&opts);
        assert!(cmd.contains("rsync -az"));
        assert!(!cmd.contains("--delete"));
        assert!(cmd.contains("--exclude '*.log'"));
        assert!(cmd.contains("--exclude 'node_modules/'"));
        assert!(cmd.contains("--include '*.rs'"));
        assert!(cmd.contains("--bwlimit=1000"));
        assert!(cmd.contains("ssh -p 2200"));
        assert!(cmd.contains("-i '/path/to/key.pem'"));
        assert!(cmd.contains("appuser@192.168.56.10:'/var/www'"));
    }

    #[test]
    fn test_build_rsync_command_minimal() {
        let opts = SyncedFolderOptions {
            guest_path: "/var/www".to_string(),
            host_path: "/home/user/app/".to_string(),
            mount_options: Vec::new(),
            ..Default::default()
        };

        let cmd = RsyncSyncedFolder::build_rsync_command(&opts);
        assert!(cmd.contains("rsync -az"));
        assert!(cmd.contains("--delete"));
        assert!(!cmd.contains("-i '"));
        assert!(cmd.contains("vagrant@127.0.0.1:'/var/www'"));
    }

    #[test]
    fn test_build_rsync_command_no_delete() {
        let opts = SyncedFolderOptions {
            guest_path: "/var/www".to_string(),
            host_path: "/home/user/app/".to_string(),
            mount_options: vec!["no_delete".to_string()],
            ..Default::default()
        };

        let cmd = RsyncSyncedFolder::build_rsync_command(&opts);
        assert!(cmd.contains("rsync -az"));
        assert!(!cmd.contains("--delete"));
    }

    #[test]
    fn test_rsync_folder_missing_host_path() {
        let folder = RsyncSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: "/tmp/nonexistent_migratory_path_rsync_123".to_string(),
            ..Default::default()
        };
        let comm = MockComm::default();
        assert!(folder.mount(&opts, &comm).is_err());
    }

    #[test]
    fn test_rsync_folder_empty_guest_path() {
        let dir = tempdir().expect("operation should succeed");
        let folder = RsyncSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = MockComm::default();
        assert!(folder.mount(&opts, &comm).is_err());
    }
}
