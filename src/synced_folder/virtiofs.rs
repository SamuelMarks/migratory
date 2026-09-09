//! VirtIO-FS and 9p Synced Folders for QEMU / KVM / libvirt.

use super::{SyncedFolder, SyncedFolderOptions};
use crate::communicator::Communicator;
use crate::error::MigratoryError;

/// Filesystem backend driver type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VirtioFsType {
    /// VirtIO-FS filesystem driver (high performance).
    #[default]
    VirtioFs,
    /// 9p filesystem driver (Plan 9 protocol over VirtIO).
    Plan9,
}

/// VirtIO-FS and 9p Synced Folder implementation.
pub struct VirtioFsSyncedFolder {
    /// Type of filesystem driver (VirtioFs or Plan9).
    pub fs_type: VirtioFsType,
}

impl VirtioFsSyncedFolder {
    /// Creates a new VirtioFsSyncedFolder with the specified driver type.
    pub fn new(fs_type: VirtioFsType) -> Self {
        Self { fs_type }
    }

    /// Derives the mount tag for a synced folder.
    pub fn get_mount_tag(options: &SyncedFolderOptions) -> String {
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

    /// Generates the XML device snippet to inject into libvirt domain XML.
    pub fn generate_domain_xml_snippet(
        options: &SyncedFolderOptions,
        fs_type: VirtioFsType,
    ) -> String {
        let tag = Self::get_mount_tag(options);
        match fs_type {
            VirtioFsType::VirtioFs => format!(
                "<filesystem type='mount' accessmode='passthrough'>
  <driver type='virtiofs'/>
  <source dir='{}'/>
  <target dir='{}'/>
</filesystem>",
                options.host_path, tag
            ),
            VirtioFsType::Plan9 => format!(
                "<filesystem type='mount' accessmode='mapped'>
  <driver type='path' wrpolicy='immediate'/>
  <source dir='{}'/>
  <target dir='{}'/>
</filesystem>",
                options.host_path, tag
            ),
        }
    }
}

impl Default for VirtioFsSyncedFolder {
    fn default() -> Self {
        Self::new(VirtioFsType::VirtioFs)
    }
}

impl SyncedFolder for VirtioFsSyncedFolder {
    /// Prepares the host for VirtIO-FS or 9p syncing by verifying host path.
    fn prepare(&self, options: &SyncedFolderOptions) -> Result<(), MigratoryError> {
        let host_path = std::path::Path::new(&options.host_path);
        if !host_path.exists() {
            return Err(MigratoryError::Validation(format!(
                "Host path does not exist for VirtIO-FS sync: {}",
                options.host_path
            )));
        }
        Ok(())
    }

    /// Mounts the VirtIO-FS or 9p folder on the guest.
    fn mount(
        &self,
        options: &SyncedFolderOptions,
        comm: &dyn Communicator,
    ) -> Result<(), MigratoryError> {
        if options.guest_path.is_empty() {
            return Err(MigratoryError::Validation(
                "Guest path cannot be empty for VirtIO-FS mount".to_string(),
            ));
        }

        let mkdir_cmd = format!(
            "sudo mkdir -p '{}'",
            options.guest_path.replace('\'', "'\\''")
        );
        let _ = comm.execute(&mkdir_cmd)?;

        let tag = Self::get_mount_tag(options);

        let mount_cmd = match self.fs_type {
            VirtioFsType::VirtioFs => {
                let mut opts = Vec::new();
                for opt in &options.mount_options {
                    opts.push(opt.as_str());
                }
                if opts.is_empty() {
                    format!(
                        "sudo mount -t virtiofs '{}' '{}'",
                        tag,
                        options.guest_path.replace('\'', "'\\''")
                    )
                } else {
                    format!(
                        "sudo mount -t virtiofs -o {} '{}' '{}'",
                        opts.join(","),
                        tag,
                        options.guest_path.replace('\'', "'\\''")
                    )
                }
            }
            VirtioFsType::Plan9 => {
                let mut opts = vec!["trans=virtio", "version=9p2000.L", "rw"];
                for opt in &options.mount_options {
                    opts.push(opt.as_str());
                }
                format!(
                    "sudo mount -t 9p -o {} '{}' '{}'",
                    opts.join(","),
                    tag,
                    options.guest_path.replace('\'', "'\\''")
                )
            }
        };

        comm.execute(&mount_cmd)?;

        // Persist mount across reboots in /etc/fstab if not transient
        if !options.transient {
            let fstab_fs_type = match self.fs_type {
                VirtioFsType::VirtioFs => "virtiofs",
                VirtioFsType::Plan9 => "9p",
            };
            let fstab_opts = match self.fs_type {
                VirtioFsType::VirtioFs => "defaults".to_string(),
                VirtioFsType::Plan9 => "trans=virtio,version=9p2000.L,rw".to_string(),
            };
            let fstab_line = format!(
                "{} {} {} {} 0 0",
                tag, options.guest_path, fstab_fs_type, fstab_opts
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
        #[coverage(off)]
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

    struct SubstringFailComm(&'static str);
    impl Communicator for SubstringFailComm {
        #[coverage(off)]
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            if command.contains(self.0) {
                Err(MigratoryError::Generic("fail".to_string()))
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

    #[test]
    fn test_virtiofs_mount_success() {
        let dir = tempdir().expect("operation should succeed");
        let folder = VirtioFsSyncedFolder::default();
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            mount_options: vec!["ro".to_string()],
            ..Default::default()
        };
        let comm = MockComm;
        assert!(folder.prepare(&opts).is_ok());
        assert!(folder.mount(&opts, &comm).is_ok());
    }

    /// Tests virtiofs mount without custom mount options and with fstab persistence.
    #[test]
    fn test_virtiofs_mount_no_opts_and_fstab() {
        let dir = tempdir().expect("operation should succeed");
        let folder = VirtioFsSyncedFolder::default();
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            mount_options: vec![],
            transient: false,
            ..Default::default()
        };
        let comm = MockComm;
        assert!(folder.prepare(&opts).is_ok());
        assert!(folder.mount(&opts, &comm).is_ok());
    }

    #[test]
    fn test_plan9_mount_success_and_fstab() {
        let dir = tempdir().expect("operation should succeed");
        let folder = VirtioFsSyncedFolder::new(VirtioFsType::Plan9);
        let opts = SyncedFolderOptions {
            guest_path: "/shared".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            mount_options: vec!["debug".to_string()],
            transient: false,
            ..Default::default()
        };
        let comm = MockComm;
        assert!(folder.prepare(&opts).is_ok());
        assert!(folder.mount(&opts, &comm).is_ok());
    }

    #[test]
    fn test_virtiofs_xml_snippet() {
        let opts = SyncedFolderOptions {
            name: Some("data_share".to_string()),
            guest_path: "/data".to_string(),
            host_path: "/home/user/data".to_string(),
            ..Default::default()
        };

        let xml_fs =
            VirtioFsSyncedFolder::generate_domain_xml_snippet(&opts, VirtioFsType::VirtioFs);
        assert!(xml_fs.contains("<driver type='virtiofs'/>"));
        assert!(xml_fs.contains("<target dir='data_share'/>"));

        let xml_9p = VirtioFsSyncedFolder::generate_domain_xml_snippet(&opts, VirtioFsType::Plan9);
        assert!(xml_9p.contains("<driver type='path' wrpolicy='immediate'/>"));
        assert!(xml_9p.contains("<target dir='data_share'/>"));
    }

    #[test]
    fn test_virtiofs_get_mount_tag() {
        let opts_default = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            ..Default::default()
        };
        assert_eq!(
            VirtioFsSyncedFolder::get_mount_tag(&opts_default),
            "vagrant"
        );

        let opts_path = SyncedFolderOptions {
            guest_path: "/var/app".to_string(),
            ..Default::default()
        };
        assert_eq!(VirtioFsSyncedFolder::get_mount_tag(&opts_path), "var_app");

        let opts_root = SyncedFolderOptions {
            guest_path: "/".to_string(),
            ..Default::default()
        };
        assert_eq!(VirtioFsSyncedFolder::get_mount_tag(&opts_root), "vagrant");
    }

    #[test]
    fn test_virtiofs_missing_host_path() {
        let folder = VirtioFsSyncedFolder::default();
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: "/nonexistent/virtiofs/path".to_string(),
            ..Default::default()
        };
        assert!(folder.prepare(&opts).is_err());
    }

    #[test]
    fn test_virtiofs_empty_guest_path() {
        let dir = tempdir().expect("operation should succeed");
        let folder = VirtioFsSyncedFolder::default();
        let opts = SyncedFolderOptions {
            guest_path: "".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = MockComm;
        assert!(folder.mount(&opts, &comm).is_err());
    }

    #[test]
    fn test_virtiofs_mount_comm_failure() {
        let dir = tempdir().expect("operation should succeed");
        let folder = VirtioFsSyncedFolder::default();
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = FailComm;
        assert!(folder.mount(&opts, &comm).is_err());
    }

    /// Tests virtiofs mount command failure when mkdir succeeds.
    #[test]
    fn test_virtiofs_mount_cmd_failure() {
        let dir = tempdir().expect("operation should succeed");
        let folder = VirtioFsSyncedFolder::default();
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };
        let comm = SubstringFailComm("mount");
        assert!(folder.mount(&opts, &comm).is_err());
    }

    /// Tests virtiofs fstab persistence command failure when mount succeeds.
    #[test]
    fn test_virtiofs_fstab_cmd_failure() {
        let dir = tempdir().expect("operation should succeed");
        let folder = VirtioFsSyncedFolder::default();
        let opts = SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: dir.path().to_string_lossy().to_string(),
            transient: false,
            ..Default::default()
        };
        let comm = SubstringFailComm("fstab");
        assert!(folder.mount(&opts, &comm).is_err());
    }
}
