//! VirtualBox Shared Folders.

use super::{SyncedFolder, SyncedFolderOptions};
use crate::communicator::Communicator;
use crate::error::MigratoryError;
#[cfg(not(test))]
use crate::provider::virtualbox::execute_vboxmanage;

/// VirtualBox Shared Folder.
pub struct VboxSyncedFolder {
    machine_id: Option<String>,
}

impl VboxSyncedFolder {
    /// Creates a new VirtualBox synced folder instance.
    pub fn new(machine_id: Option<String>) -> Self {
        Self { machine_id }
    }

    /// Helper to get the ID, returning an error if not present.
    fn require_id(&self) -> Result<&str, MigratoryError> {
        self.machine_id.as_deref().ok_or_else(|| {
            MigratoryError::Generic("Machine not created or ID not found".to_string())
        })
    }

    /// Determines the share name for a synced folder.
    pub fn get_share_name(options: &SyncedFolderOptions) -> String {
        if let Some(name) = &options.name {
            name.clone()
        } else if options.guest_path == "/vagrant" {
            "vagrant".to_string()
        } else {
            let sanitized = options.guest_path.trim_matches('/').replace('/', "_");
            if sanitized.is_empty() {
                "vagrant".to_string()
            } else {
                sanitized
            }
        }
    }

    /// Executes the prepare command on the host.
    #[cfg(not(test))]
    #[coverage(off)]
    fn execute_prepare_command(
        &self,
        id: &str,
        share_name: &str,
        host_path: &str,
        transient: bool,
    ) -> Result<(), MigratoryError> {
        // Remove existing just in case (ignoring errors if it doesn't exist)
        let _ = execute_vboxmanage(&["sharedfolder", "remove", id, "--name", share_name]);

        let mut args = vec![
            "sharedfolder",
            "add",
            id,
            "--name",
            share_name,
            "--hostpath",
            host_path,
        ];
        if transient {
            args.push("--transient");
        }
        execute_vboxmanage(&args)?;

        let symlink_key = format!(
            "VBoxInternal2/SharedFoldersEnableSymlinksCreate/{}",
            share_name
        );
        let _ = execute_vboxmanage(&["setextradata", id, &symlink_key, "1"]);

        Ok(())
    }

    /// Executes the prepare command on the host (test mock).
    #[cfg(test)]
    #[coverage(off)]
    fn execute_prepare_command(
        &self,
        _id: &str,
        _share_name: &str,
        _host_path: &str,
        _transient: bool,
    ) -> Result<(), MigratoryError> {
        Ok(())
    }
}

impl SyncedFolder for VboxSyncedFolder {
    /// Prepares the VBox synced folder.
    fn prepare(&self, options: &SyncedFolderOptions) -> Result<(), MigratoryError> {
        let host_path = std::path::Path::new(&options.host_path);
        if !host_path.exists() {
            return Err(MigratoryError::Validation(format!(
                "Host path does not exist for VBox sync: {}",
                options.host_path
            )));
        }

        let id = self.require_id()?;
        let share_name = Self::get_share_name(options);

        self.execute_prepare_command(id, &share_name, &options.host_path, options.transient)
    }

    /// Mounts the VBox synced folder in the guest.
    fn mount(
        &self,
        options: &SyncedFolderOptions,
        comm: &dyn Communicator,
    ) -> Result<(), MigratoryError> {
        if options.guest_path.is_empty() {
            return Err(MigratoryError::Validation(
                "Guest path cannot be empty for VBox mount".to_string(),
            ));
        }

        // Verify vboxsf kernel module presence inside the guest
        comm.execute(
            "if ! lsmod | grep -q vboxsf; then sudo modprobe vboxsf 2>/dev/null || true; fi",
        )?;

        let owner = options.owner.as_deref().unwrap_or("vagrant");
        let group = options.group.as_deref().unwrap_or("vagrant");

        let mkdir_cmd = format!(
            "sudo mkdir -p '{}'",
            options.guest_path.replace('\'', "'\\''")
        );
        let _ = comm.execute(&mkdir_cmd)?;

        // Combine options
        let mut mount_opts = format!(
            "uid=`id -u {}`,gid=`getent group {} | cut -d: -f3`",
            owner, group
        );
        if let Some(dmode) = &options.dmode {
            mount_opts.push_str(&format!(",dmode={}", dmode));
        }
        if let Some(fmode) = &options.fmode {
            mount_opts.push_str(&format!(",fmode={}", fmode));
        }
        for opt in &options.mount_options {
            mount_opts.push_str(&format!(",{}", opt));
        }

        let share_name = Self::get_share_name(options);

        let mount_cmd = format!(
            "sudo mount -t vboxsf -o {} '{}' '{}'",
            mount_opts,
            share_name,
            options.guest_path.replace('\'', "'\\''")
        );
        let _ = comm.execute(&mount_cmd)?;

        // Persist mount across reboots in /etc/fstab if not transient
        if !options.transient {
            let fstab_line = format!(
                "{} {} vboxsf {} 0 0",
                share_name, options.guest_path, mount_opts
            );
            let fstab_cmd = format!(
                "if ! grep -qs '{}' /etc/fstab; then echo '{}' | sudo tee -a /etc/fstab; fi",
                options.guest_path.replace('\'', "'\\''"),
                fstab_line.replace('\'', "'\\''")
            );
            let _ = comm.execute(&fstab_cmd)?;
        }

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

    struct StepFailComm {
        fail_at: std::sync::atomic::AtomicUsize,
        counter: std::sync::atomic::AtomicUsize,
    }
    impl StepFailComm {
        fn new(fail_at: usize) -> Self {
            Self {
                fail_at: std::sync::atomic::AtomicUsize::new(fail_at),
                counter: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }
    impl Communicator for StepFailComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            let current = self
                .counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if current == self.fail_at.load(std::sync::atomic::Ordering::SeqCst) {
                Err(MigratoryError::Generic("step comm failure".to_string()))
            } else {
                Ok("ok".to_string())
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

    #[test]
    fn test_vbox_folder_owner_group_and_step_failures() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let folder = VboxSyncedFolder::new(Some("test-id".to_string()));
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            owner: Some("custom_owner".to_string()),
            group: Some("custom_group".to_string()),
            transient: false,
            ..Default::default()
        };

        let comm = MockComm;
        assert!(folder.mount(&opts, &comm).is_ok());

        // Fail at step 1 (mkdir)
        let fail_step1 = StepFailComm::new(1);
        assert!(folder.mount(&opts, &fail_step1).is_err());

        // Fail at step 2 (mount)
        let fail_step2 = StepFailComm::new(2);
        assert!(folder.mount(&opts, &fail_step2).is_err());

        // Fail at step 3 (fstab)
        let fail_step3 = StepFailComm::new(3);
        assert!(folder.mount(&opts, &fail_step3).is_err());

        Ok(())
    }

    #[test]
    fn test_vbox_folder_mount_comm_failure() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let folder = VboxSyncedFolder::new(Some("test-id".to_string()));
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = FailComm;
        assert!(folder.mount(&opts, &comm).is_err());
        Ok(())
    }

    #[test]
    fn test_vbox_folder_success() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let folder = VboxSyncedFolder::new(Some("test-id".to_string()));
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            dmode: Some("0755".to_string()),
            fmode: Some("0644".to_string()),
            mount_options: vec!["dmask=0777".to_string()],
            ..Default::default()
        };

        let comm = MockComm;
        assert!(folder.prepare(&opts).is_ok());
        assert!(folder.mount(&opts, &comm).is_ok());
        Ok(())
    }

    #[test]
    fn test_vbox_folder_non_transient() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let folder = VboxSyncedFolder::new(Some("test-id".to_string()));
        let opts = SyncedFolderOptions {
            guest_path: "/var/www".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            transient: false,
            name: Some("custom_www".to_string()),
            ..Default::default()
        };

        let comm = MockComm;
        assert!(folder.prepare(&opts).is_ok());
        assert!(folder.mount(&opts, &comm).is_ok());
        Ok(())
    }

    #[test]
    fn test_vbox_get_share_name() {
        let opts_custom = SyncedFolderOptions {
            name: Some("my_share".to_string()),
            guest_path: "/data".to_string(),
            ..Default::default()
        };
        assert_eq!(VboxSyncedFolder::get_share_name(&opts_custom), "my_share");

        let opts_default = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            ..Default::default()
        };
        assert_eq!(VboxSyncedFolder::get_share_name(&opts_default), "vagrant");

        let opts_path = SyncedFolderOptions {
            guest_path: "/opt/app/src".to_string(),
            ..Default::default()
        };
        assert_eq!(VboxSyncedFolder::get_share_name(&opts_path), "opt_app_src");

        let opts_root = SyncedFolderOptions {
            guest_path: "/".to_string(),
            ..Default::default()
        };
        assert_eq!(VboxSyncedFolder::get_share_name(&opts_root), "vagrant");
    }

    #[test]
    fn test_vbox_folder_missing_machine_id() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let folder = VboxSyncedFolder::new(None);
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        assert!(folder.prepare(&opts).is_err());
        Ok(())
    }

    #[test]
    fn test_vbox_folder_missing_host_path() {
        let folder = VboxSyncedFolder::new(Some("test-id".to_string()));
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: "/tmp/nonexistent_migratory_path_vbox_123".to_string(),
            ..Default::default()
        };
        assert!(folder.prepare(&opts).is_err());
    }

    #[test]
    fn test_vbox_folder_empty_guest_path() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let folder = VboxSyncedFolder::new(Some("test-id".to_string()));
        let opts = SyncedFolderOptions {
            guest_path: "".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = MockComm;
        assert!(folder.mount(&opts, &comm).is_err());
        Ok(())
    }
}
