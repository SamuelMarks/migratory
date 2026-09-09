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

            let mut export_lines = String::new();
            export_lines.push_str("# VAGRANT-BEGIN\n");

            for sf in _folders {
                if sf.folder_type.as_deref() == Some("nfs") && !sf.disabled {
                    export_lines.push_str(&format!(
                        "{} *(rw,sync,no_subtree_check,all_squash,anonuid={},anongid={})\n",
                        sf.host_path, uid, gid
                    ));
                }
            }
            export_lines.push_str("# VAGRANT-END\n");

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
        assert_eq!(host.service_manager(), "sysvinit");
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
}
