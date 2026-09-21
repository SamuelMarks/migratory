//! SMB Synced Folders.

use super::{SyncedFolder, SyncedFolderOptions};
use crate::communicator::Communicator;
use crate::error::MigratoryError;

/// SMB Shared Folder.
pub struct SmbSyncedFolder;

impl SyncedFolder for SmbSyncedFolder {
    /// Prepares the SMB synced folder.
    fn prepare(&self, options: &SyncedFolderOptions) -> Result<(), MigratoryError> {
        let host_path = std::path::Path::new(&options.host_path);
        if !host_path.exists() {
            return Err(MigratoryError::Validation(format!(
                "Host path does not exist for SMB sync: {}",
                options.host_path
            )));
        }

        let share_name = Self::get_share_name(options);
        self.execute_prepare_command(&options.host_path, &share_name)
    }

    /// Mounts the SMB synced folder in the guest.
    fn mount(
        &self,
        options: &SyncedFolderOptions,
        comm: &dyn Communicator,
    ) -> Result<(), MigratoryError> {
        if options.guest_path.is_empty() {
            return Err(MigratoryError::Validation(
                "Guest path cannot be empty for SMB mount".to_string(),
            ));
        }

        // Verify cifs client tools on guest
        comm.execute(
            "if ! which mount.cifs >/dev/null 2>&1; then if which apt-get >/dev/null 2>&1; then sudo apt-get update -y && sudo apt-get install -y cifs-utils; elif which yum >/dev/null 2>&1; then sudo yum install -y cifs-utils; fi; fi",
        )?;

        let mkdir_cmd = format!(
            "sudo mkdir -p '{}'",
            options.guest_path.replace('\'', "'\\''")
        );
        let _ = comm.execute(&mkdir_cmd)?;

        // Default host IP is accessible via default gateway on guest
        let mut host_ip = "10.0.2.2".to_string();
        for opt in &options.mount_options {
            if let Some(ip) = opt.strip_prefix("host_ip=") {
                host_ip = ip.to_string();
            }
        }

        let share_name = Self::get_share_name(options);
        let mount_opts = Self::build_mount_options(options);

        let mount_cmd = format!(
            "sudo mount -t cifs -o {} //{}/{} '{}'",
            mount_opts,
            host_ip,
            share_name,
            options.guest_path.replace('\'', "'\\''")
        );

        let _ = comm.execute(&mount_cmd)?;

        Ok(())
    }
}

impl SmbSyncedFolder {
    /// Determines the SMB share name.
    pub fn get_share_name(options: &SyncedFolderOptions) -> String {
        if let Some(name) = &options.name {
            name.clone()
        } else {
            let sanitized = options.guest_path.trim_matches('/').replace('/', "_");
            if sanitized.is_empty() {
                "vgt-share".to_string()
            } else {
                format!("vgt-{}", sanitized)
            }
        }
    }

    /// Extracts SMB username and password from options or environment.
    pub fn get_credentials(options: &SyncedFolderOptions) -> (String, String) {
        let mut username =
            std::env::var("VAGRANT_SMB_USERNAME").unwrap_or_else(|_| "vagrant".to_string());
        let mut password =
            std::env::var("VAGRANT_SMB_PASSWORD").unwrap_or_else(|_| "vagrant".to_string());

        for opt in &options.mount_options {
            if let Some(u) = opt.strip_prefix("smb_username=") {
                username = u.to_string();
            } else if let Some(u) = opt.strip_prefix("username=") {
                username = u.to_string();
            } else if let Some(p) = opt.strip_prefix("smb_password=") {
                password = p.to_string();
            } else if let Some(p) = opt.strip_prefix("password=") {
                password = p.to_string();
            }
        }

        (username, password)
    }

    /// Constructs the CIFS mount options with NTLMSSP security negotiation.
    pub fn build_mount_options(options: &SyncedFolderOptions) -> String {
        let owner = options.owner.as_deref().unwrap_or("vagrant");
        let group = options.group.as_deref().unwrap_or("vagrant");
        let (username, password) = Self::get_credentials(options);

        let mut mount_opts = format!(
            "uid=`id -u {}`,gid=`getent group {} | cut -d: -f3`,username={},password={},sec=ntlmssp",
            owner, group, username, password
        );

        if let Some(dmode) = &options.dmode {
            mount_opts.push_str(&format!(",dir_mode={}", dmode));
        }
        if let Some(fmode) = &options.fmode {
            mount_opts.push_str(&format!(",file_mode={}", fmode));
        }

        for opt in &options.mount_options {
            if !opt.starts_with("smb_username=")
                && !opt.starts_with("username=")
                && !opt.starts_with("smb_password=")
                && !opt.starts_with("password=")
                && !opt.starts_with("host_ip=")
            {
                mount_opts.push_str(&format!(",{}", opt));
            }
        }

        mount_opts
    }

    /// Executes the prepare command on the host by delegating to the detected host implementation.
    ///
    /// # Arguments
    ///
    /// * `host_path` - Local host directory to share via SMB.
    /// * `share_name` - Name of the SMB share.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if host detection or SMB configuration fails.
    #[coverage(off)]
    pub fn execute_prepare_command(
        &self,
        host_path: &str,
        share_name: &str,
    ) -> Result<(), MigratoryError> {
        #[cfg(test)]
        if std::env::var("MIGRATORY_TEST_MOCK_PREPARE_ERR").is_ok() {
            return Err(MigratoryError::Generic("Mock prepare error".to_string()));
        }

        let host = crate::host::detect_host()?;
        let sf_config = crate::config::SyncedFolderConfig {
            host_path: host_path.to_string(),
            guest_path: format!("/{}", share_name),
            folder_type: Some("smb".to_string()),
            disabled: false,
            ..Default::default()
        };
        host.configure_smb(&[sf_config])
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

    struct FailMkdirComm;
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

    impl Communicator for FailMkdirComm {
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            if command.starts_with("sudo mkdir") {
                Err(MigratoryError::Generic("mkdir failed".to_string()))
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
    fn test_smb_folder_mount_comm_failure() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let folder = SmbSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = FailComm;
        assert!(folder.mount(&opts, &comm).is_err());

        let comm_mkdir = FailMkdirComm;
        assert!(folder.mount(&opts, &comm_mkdir).is_err());

        let comm_mount = FailMountComm;
        assert!(folder.mount(&opts, &comm_mount).is_err());
        Ok(())
    }

    #[test]
    fn test_smb_folder_success() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let folder = SmbSyncedFolder;
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
    fn test_smb_share_name() {
        let opts_custom = SyncedFolderOptions {
            name: Some("my_custom_share".to_string()),
            guest_path: "/data".to_string(),
            ..Default::default()
        };
        assert_eq!(
            SmbSyncedFolder::get_share_name(&opts_custom),
            "my_custom_share"
        );

        let opts_path = SyncedFolderOptions {
            guest_path: "/var/www/html".to_string(),
            ..Default::default()
        };
        assert_eq!(
            SmbSyncedFolder::get_share_name(&opts_path),
            "vgt-var_www_html"
        );

        let opts_root = SyncedFolderOptions {
            guest_path: "/".to_string(),
            ..Default::default()
        };
        assert_eq!(SmbSyncedFolder::get_share_name(&opts_root), "vgt-share");
    }

    #[test]
    fn test_smb_credentials_and_mount_options() {
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: "/tmp".to_string(),
            dmode: Some("0755".to_string()),
            fmode: Some("0644".to_string()),
            mount_options: vec![
                "smb_username=testuser".to_string(),
                "smb_password=secret".to_string(),
                "host_ip=192.168.1.50".to_string(),
                "vers=3.0".to_string(),
            ],
            ..Default::default()
        };

        let (user, pass) = SmbSyncedFolder::get_credentials(&opts);
        assert_eq!(user, "testuser");
        assert_eq!(pass, "secret");

        let mount_opts = SmbSyncedFolder::build_mount_options(&opts);
        assert!(mount_opts.contains("username=testuser"));
        assert!(mount_opts.contains("password=secret"));
        assert!(mount_opts.contains("sec=ntlmssp"));
        assert!(mount_opts.contains("dir_mode=0755"));
        assert!(mount_opts.contains("file_mode=0644"));
        assert!(mount_opts.contains("vers=3.0"));
        assert!(!mount_opts.contains("host_ip="));
    }

    #[test]
    fn test_smb_folder_missing_host_path() {
        let folder = SmbSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: "/tmp/nonexistent_migratory_path_smb_123".to_string(),
            ..Default::default()
        };
        assert!(folder.prepare(&opts).is_err());
    }

    #[test]
    fn test_smb_folder_empty_guest_path() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let folder = SmbSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = MockComm;
        assert!(folder.mount(&opts, &comm).is_err());
        Ok(())
    }

    #[test]
    fn test_smb_folder_with_mount_options() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let folder = SmbSyncedFolder;
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            mount_options: vec![
                "rw".to_string(),
                "noatime".to_string(),
                "host_ip=192.168.1.50".to_string(),
                "username=alternateuser".to_string(),
                "password=alternatepass".to_string(),
            ],
            ..Default::default()
        };

        let (user, pass) = SmbSyncedFolder::get_credentials(&opts);
        assert_eq!(user, "alternateuser");
        assert_eq!(pass, "alternatepass");

        let comm = MockComm;
        assert!(folder.mount(&opts, &comm).is_ok());
        Ok(())
    }

    #[test]
    fn test_smb_execute_prepare_command() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let folder = SmbSyncedFolder;
        let dir = tempdir().expect("tempdir failed");
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        assert!(folder.prepare(&opts).is_ok());
        assert!(
            folder
                .execute_prepare_command(&opts.host_path, "vagrant")
                .is_ok()
        );
    }

    #[test]
    fn test_smb_execute_prepare_command_mock_err() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let folder = SmbSyncedFolder;
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PREPARE_ERR", "1");
        }
        let res = folder.execute_prepare_command("/tmp", "vagrant");
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PREPARE_ERR");
        }
        assert!(res.is_err());
    }
}
