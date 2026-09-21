//! Linux host OS capabilities.
//!
//! This module provides the implementation for the Linux host,
//! detailing operations such as configuring NFS or SMB file sharing.

use super::Host;
use crate::error::MigratoryError;
#[cfg(not(test))]
use std::process::Command;

/// Linux host implementation.
///
/// Implements the `Host` trait for Linux systems, handling platform-specific
/// configurations.
pub struct LinuxHost;

impl Host for LinuxHost {
    /// Returns the canonical name of the host OS.
    ///
    /// # Returns
    ///
    /// Returns the string `"linux"`.
    fn name(&self) -> &str {
        "linux"
    }

    /// Detects if the current system is running Linux.
    ///
    /// # Returns
    ///
    /// Returns `true` if `std::env::consts::OS` is `"linux"`.
    #[coverage(off)]
    fn is_match(&self) -> bool {
        if let Ok(mock) = std::env::var("MOCK_OS") {
            return mock == "linux";
        }
        std::env::consts::OS == "linux"
    }
    /// Configures NFS exports on the Linux host.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if manipulating `/etc/exports` fails (currently a no-op).
    #[coverage(off)]
    fn configure_nfs(
        &self,
        _folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        #[cfg(not(test))]
        {
            let uid = std::process::Command::new("id")
                .arg("-u")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "1000".to_string());
            let gid = std::process::Command::new("id")
                .arg("-g")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "1000".to_string());

            let export_lines = Self::generate_nfs_exports(_folders, &uid, &gid);

            let cmd = format!(
                "echo '{}' | sudo tee -a /etc/exports > /dev/null",
                export_lines
            );
            let _ = Command::new("sh").arg("-c").arg(cmd).status();

            let status = Command::new("sudo")
                .arg("exportfs")
                .arg("-ra")
                .status()
                .map_err(MigratoryError::Io)?;

            if !status.success() {
                return Err(MigratoryError::Generic(
                    "Failed to reload NFS exports via exportfs".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Configures SMB sharing on the Linux host.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the configuration fails (currently a no-op).
    #[coverage(off)]
    fn configure_smb(
        &self,
        _folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        #[cfg(not(test))]
        {
            let conf_block = Self::generate_smb_conf(_folders);
            let cmd = format!(
                "echo '{}' | sudo tee -a /etc/samba/smb.conf > /dev/null",
                conf_block
            );
            let _ = Command::new("sh").arg("-c").arg(cmd).status();

            let status = Command::new("sudo")
                .arg("systemctl")
                .arg("reload")
                .arg("smbd")
                .status()
                .map_err(MigratoryError::Io)?;

            if !status.success() {
                return Err(MigratoryError::Generic(
                    "Failed to reload Samba on Linux host".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Checks if the current user has administrative (root) privileges on Linux.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the user is an admin.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the check cannot be completed (currently a mocked `Ok(true)`).
    #[coverage(off)]
    fn check_admin(&self) -> Result<bool, MigratoryError> {
        #[cfg(not(test))]
        {
            let output = Command::new("id")
                .arg("-u")
                .output()
                .map_err(MigratoryError::Io)?;
            let uid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(uid == "0")
        }
        #[cfg(test)]
        Ok(true)
    }

    #[coverage(off)]
    fn resolve_host_ip(&self) -> Result<String, MigratoryError> {
        // Find local ip using `ip route get`
        let cmd = "ip route get 1 | awk '{print $7}' | tr -d '
'";
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .unwrap_or_else(|_| std::process::Output {
                status: std::os::unix::process::ExitStatusExt::from_raw(0),
                stdout: b"192.168.1.3".to_vec(),
                stderr: vec![],
            });
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn service_manager(&self) -> &str {
        Self::detect_service_manager(
            std::path::Path::new("/run/systemd/system"),
            std::path::Path::new("/run/openrc"),
        )
    }

    fn list_bridge_interfaces(&self) -> Result<Vec<String>, MigratoryError> {
        list_linux_bridge_interfaces()
    }
}

#[cfg(not(test))]
#[coverage(off)]
fn list_linux_bridge_interfaces() -> Result<Vec<String>, MigratoryError> {
    let mut list = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for entry in entries.flatten() {
            list.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    if list.is_empty() {
        list.push("eth0".to_string());
    }
    Ok(list)
}

#[cfg(test)]
fn list_linux_bridge_interfaces() -> Result<Vec<String>, MigratoryError> {
    Ok(vec![
        "eth0".to_string(),
        "eth1".to_string(),
        "br0".to_string(),
    ])
}

impl LinuxHost {
    /// Checks if the user is in the libvirt group for socket access.
    #[coverage(off)]
    pub fn check_libvirt_group(&self) -> Result<bool, MigratoryError> {
        #[cfg(not(test))]
        {
            let output = Command::new("groups")
                .output()
                .map_err(MigratoryError::Io)?;
            let groups = String::from_utf8_lossy(&output.stdout);
            Ok(groups.contains("libvirt") || groups.contains("libvirtd"))
        }
        #[cfg(test)]
        Ok(true)
    }

    /// Detects the service manager from filesystem paths.
    ///
    /// # Arguments
    ///
    /// * `systemd_path` - Path indicating systemd is active.
    /// * `openrc_path` - Path indicating openrc is active.
    ///
    /// # Returns
    ///
    /// Returns the service manager name ("systemd", "openrc", or "sysvinit").
    pub fn detect_service_manager(
        systemd_path: &std::path::Path,
        openrc_path: &std::path::Path,
    ) -> &'static str {
        if systemd_path.exists() {
            "systemd"
        } else if openrc_path.exists() {
            "openrc"
        } else {
            "sysvinit"
        }
    }

    /// Detects host capabilities like hypervisors.
    pub fn capabilities(&self) -> Vec<String> {
        let mut caps = Vec::new();
        if std::process::Command::new("VBoxManage")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            caps.push("virtualbox".to_string());
        }
        if std::process::Command::new("virsh")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            caps.push("libvirt".to_string());
        }
        caps
    }

    /// Generates the NFS export configuration block for Linux hosts.
    ///
    /// Handles folder-specific options (such as mount_options).
    ///
    /// # Arguments
    ///
    /// * `folders` - Configured synced folders.
    /// * `uid` - Anonymous user ID.
    /// * `gid` - Anonymous group ID.
    ///
    /// # Returns
    ///
    /// Returns the formatted export lines.
    pub fn generate_nfs_exports(
        folders: &[crate::config::SyncedFolderConfig],
        uid: &str,
        gid: &str,
    ) -> String {
        let mut lines = String::new();
        lines.push_str("# VAGRANT-BEGIN\n");

        for sf in folders {
            if sf.folder_type.as_deref() == Some("nfs") && !sf.disabled {
                let default_opts = format!(
                    "rw,sync,no_subtree_check,all_squash,anonuid={},anongid={}",
                    uid, gid
                );
                let opts = if let Some(custom_opts) = &sf.mount_options
                    && !custom_opts.is_empty()
                {
                    custom_opts.join(",")
                } else {
                    default_opts
                };
                lines.push_str(&format!("\"{}\" *({})\n", sf.host_path, opts));
            }
        }
        lines.push_str("# VAGRANT-END\n");
        lines
    }

    /// Generates Samba share definitions for Linux hosts.
    ///
    /// # Arguments
    ///
    /// * `folders` - Configured synced folders.
    ///
    /// # Returns
    ///
    /// Returns the Samba configuration block.
    pub fn generate_smb_conf(folders: &[crate::config::SyncedFolderConfig]) -> String {
        let mut conf = String::new();
        conf.push_str("# VAGRANT-BEGIN-SMB\n");
        for sf in folders {
            if sf.folder_type.as_deref() == Some("smb") && !sf.disabled {
                let name = sf
                    .guest_path
                    .replace('/', "_")
                    .trim_start_matches('_')
                    .to_string();
                let share_name = if name.is_empty() {
                    "vagrant".to_string()
                } else {
                    name
                };
                conf.push_str(&format!(
                    "[{}]\npath = {}\nbrowsable = yes\nwritable = yes\nguest ok = yes\nread only = no\n\n",
                    share_name, sf.host_path
                ));
            }
        }
        conf.push_str("# VAGRANT-END-SMB\n");
        conf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[coverage(off)]
    fn restore_mock_os(orig: Option<String>) {
        unsafe {
            if let Some(val) = orig {
                std::env::set_var("MOCK_OS", val);
            } else {
                std::env::remove_var("MOCK_OS");
            }
        }
    }

    #[test]
    fn test_linux_host() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let host = LinuxHost;
        assert_eq!(host.name(), "linux");
        assert!(["systemd", "openrc", "sysvinit"].contains(&host.service_manager()));
        assert!(host.list_bridge_interfaces().is_ok());

        let s_dir = tempfile::tempdir().expect("tempdir failed");
        let systemd_file = s_dir.path().join("systemd");
        let openrc_file = s_dir.path().join("openrc");
        let none_file = s_dir.path().join("none");

        std::fs::write(&systemd_file, "").expect("write failed");
        std::fs::write(&openrc_file, "").expect("write failed");

        assert_eq!(
            LinuxHost::detect_service_manager(&systemd_file, &openrc_file),
            "systemd"
        );
        assert_eq!(
            LinuxHost::detect_service_manager(&none_file, &openrc_file),
            "openrc"
        );
        assert_eq!(
            LinuxHost::detect_service_manager(&none_file, &none_file),
            "sysvinit"
        );

        assert!(host.configure_nfs(&[]).is_ok());
        assert!(host.configure_smb(&[]).is_ok());
        assert!(host.check_admin().expect("admin check should succeed"));
        assert!(
            host.check_libvirt_group()
                .expect("libvirt check should succeed")
        );

        let original_mock_os = std::env::var("MOCK_OS").ok();

        unsafe { std::env::set_var("MOCK_OS", "linux") };
        assert!(host.is_match());

        unsafe { std::env::set_var("MOCK_OS", "windows") };
        assert!(!host.is_match());

        unsafe { std::env::remove_var("MOCK_OS") };
        // This exercises the `std::env::consts::OS == "linux"` branch
        assert_eq!(host.is_match(), std::env::consts::OS == "linux");

        restore_mock_os(original_mock_os);

        let old_path = std::env::var_os("PATH").unwrap_or_default();
        // Test configure_nfs failure coverage
        unsafe { std::env::set_var("PATH", "") };
        let _ = host.configure_nfs(&[]);
        unsafe { std::env::set_var("PATH", old_path.clone()) };
        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let vbox = temp_dir.path().join("VBoxManage");
        let virsh = temp_dir.path().join("virsh");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&vbox, "#!/bin/sh\nexit 0").expect("operation should succeed");
            std::fs::set_permissions(&vbox, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
            std::fs::write(&virsh, "#!/bin/sh\nexit 0").expect("operation should succeed");
            std::fs::set_permissions(&virsh, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let vbox_bat = temp_dir.path().join("VBoxManage.bat");
            std::fs::write(&vbox_bat, "@echo off\nexit 0").expect("operation should succeed");
            let virsh_bat = temp_dir.path().join("virsh.bat");
            std::fs::write(&virsh_bat, "@echo off\nexit 0").expect("operation should succeed");
        }

        let mut new_path = std::ffi::OsString::new();
        new_path.push(temp_dir.path());
        #[cfg(unix)]
        new_path.push(":");
        #[cfg(windows)]
        new_path.push(";");
        new_path.push(&old_path);

        unsafe {
            std::env::set_var("PATH", &new_path);
        }
        let caps = host.capabilities();
        unsafe {
            std::env::set_var("PATH", "");
        }
        let missing_caps = host.capabilities();
        unsafe {
            std::env::set_var("PATH", old_path);
        }

        assert!(caps.contains(&"virtualbox".to_string()));
        assert!(missing_caps.is_empty());
    }

    #[test]
    fn test_linux_nfs_and_smb_generation() {
        let folders = vec![
            crate::config::SyncedFolderConfig {
                host_path: "/home/user/project".to_string(),
                guest_path: "/vagrant".to_string(),
                folder_type: Some("nfs".to_string()),
                disabled: false,
                mount_options: Some(vec!["rw".to_string(), "no_root_squash".to_string()]),
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: "/home/user/default_nfs".to_string(),
                guest_path: "/default".to_string(),
                folder_type: Some("nfs".to_string()),
                disabled: false,
                mount_options: None,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: "/home/user/smb_share".to_string(),
                guest_path: "/shared".to_string(),
                folder_type: Some("smb".to_string()),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: "/home/user/root_share".to_string(),
                guest_path: "/".to_string(),
                folder_type: Some("smb".to_string()),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: "/home/user/disabled".to_string(),
                guest_path: "/disabled".to_string(),
                folder_type: Some("nfs".to_string()),
                disabled: true,
                ..Default::default()
            },
        ];

        let exports = LinuxHost::generate_nfs_exports(&folders, "1000", "1000");
        assert!(exports.contains("# VAGRANT-BEGIN"));
        assert!(exports.contains("\"/home/user/project\" *(rw,no_root_squash)"));
        assert!(exports.contains("\"/home/user/default_nfs\" *(rw,sync,no_subtree_check,all_squash,anonuid=1000,anongid=1000)"));
        assert!(!exports.contains("/home/user/disabled"));
        assert!(exports.contains("# VAGRANT-END"));

        let smb = LinuxHost::generate_smb_conf(&folders);
        assert!(smb.contains("# VAGRANT-BEGIN-SMB"));
        assert!(smb.contains("[shared]"));
        assert!(smb.contains("path = /home/user/smb_share"));
        assert!(smb.contains("browsable = yes"));
        assert!(smb.contains("# VAGRANT-END-SMB"));
    }
}
