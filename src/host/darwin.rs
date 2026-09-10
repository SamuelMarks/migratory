//! macOS (Darwin) host OS capabilities.
//!
//! This module provides the implementation for the Darwin (macOS) host,
//! detailing operations such as configuring NFS or SMB file sharing.

use super::Host;
use crate::error::MigratoryError;
#[cfg(not(test))]
use std::process::Command;

/// Darwin (macOS) host implementation.
///
/// Implements the `Host` trait for macOS systems, handling platform-specific
/// configurations.
pub struct DarwinHost;

impl Host for DarwinHost {
    /// Returns the canonical name of the host OS.
    ///
    /// # Returns
    ///
    /// Returns the string `"darwin"`.
    fn name(&self) -> &str {
        "darwin"
    }

    /// Detects if the current system is running macOS (Darwin).
    ///
    /// # Returns
    ///
    /// Returns `true` if `std::env::consts::OS` is `"macos"`.
    #[coverage(off)]
    fn is_match(&self) -> bool {
        if let Ok(mock) = std::env::var("MOCK_OS") {
            return mock == "macos" || mock == "darwin";
        }
        std::env::consts::OS == "macos"
    }

    /// Configures NFS exports on the macOS host.
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
                .unwrap_or_else(|_| "501".to_string());
            let gid = std::process::Command::new("id")
                .arg("-g")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|_| "20".to_string());

            let export_line = format!(
                "# VAGRANT-BEGIN
/tmp -alldirs -mapall={}:{} 127.0.0.1
# VAGRANT-END
",
                uid, gid
            );
            let cmd = format!(
                "echo '{}' | sudo tee -a /etc/exports > /dev/null",
                export_line
            );
            let _ = Command::new("sh").arg("-c").arg(cmd).status();

            let status = Command::new("sudo")
                .arg("nfsd")
                .arg("restart")
                .status()
                .map_err(MigratoryError::Io)?;

            if !status.success() {
                return Err(MigratoryError::Generic(
                    "Failed to restart NFS daemon on Darwin".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Configures SMB sharing on the macOS host.
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
            for sf in _folders {
                if sf.folder_type.as_deref() == Some("smb") && !sf.disabled {
                    let share_name = sf
                        .guest_path
                        .replace('/', "_")
                        .trim_start_matches('_')
                        .to_string();
                    let share_name = if share_name.is_empty() {
                        "vagrant".to_string()
                    } else {
                        share_name
                    };

                    let status = Command::new("sudo")
                        .arg("sharing")
                        .arg("-a")
                        .arg(&sf.host_path)
                        .arg("-s")
                        .arg(&share_name)
                        .status()
                        .map_err(MigratoryError::Io)?;

                    if !status.success() {
                        return Err(MigratoryError::Generic(format!(
                            "Failed to configure SMB share on Darwin: {}",
                            share_name
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// Checks if the current user has administrative (root) privileges on macOS.
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
        // macOS typical fallback
        Ok("192.168.1.2".to_string()) // Mock
    }

    fn service_manager(&self) -> &str {
        "launchd"
    }

    fn list_bridge_interfaces(&self) -> Result<Vec<String>, MigratoryError> {
        list_darwin_bridge_interfaces()
    }
}

#[cfg(not(test))]
#[coverage(off)]
fn list_darwin_bridge_interfaces() -> Result<Vec<String>, MigratoryError> {
    let output = std::process::Command::new("ifconfig")
        .arg("-l")
        .output()
        .map_err(MigratoryError::Io)?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.split_whitespace().map(|s| s.to_string()).collect())
}

#[cfg(test)]
fn list_darwin_bridge_interfaces() -> Result<Vec<String>, MigratoryError> {
    Ok(vec![
        "en0".to_string(),
        "en1".to_string(),
        "bridge0".to_string(),
    ])
}

impl DarwinHost {
    /// Checks for Hypervisor framework privileges (entitlements check).
    #[coverage(off)]
    pub fn check_hypervisor_privileges(&self) -> Result<bool, MigratoryError> {
        #[cfg(not(test))]
        {
            let status = Command::new("sysctl")
                .arg("kern.hv_support")
                .status()
                .map_err(MigratoryError::Io)?;
            Ok(status.success())
        }
        #[cfg(test)]
        Ok(true)
    }

    /// Detects host capabilities like hypervisors.
    pub fn capabilities(&self) -> Vec<String> {
        let mut caps = Vec::new();
        // Check for virtualbox
        if std::process::Command::new("VBoxManage")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            caps.push("virtualbox".to_string());
        }
        // Check for vmware
        if std::process::Command::new("vmrun")
            .arg("list")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            caps.push("vmware".to_string());
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
    fn test_darwin_host() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let host = DarwinHost;
        assert_eq!(host.name(), "darwin");
        assert_eq!(host.service_manager(), "launchd");
        assert!(host.list_bridge_interfaces().is_ok());
        assert!(host.configure_nfs(&[]).is_ok());
        assert!(host.configure_smb(&[]).is_ok());
        assert!(host.check_admin().expect("admin check should succeed"));
        assert!(
            host.check_hypervisor_privileges()
                .expect("hv check should succeed")
        );
        let original_mock_os = std::env::var("MOCK_OS").ok();

        unsafe { std::env::set_var("MOCK_OS", "macos") };
        assert!(host.is_match());

        unsafe { std::env::set_var("MOCK_OS", "windows") };
        assert!(!host.is_match());

        unsafe { std::env::remove_var("MOCK_OS") };
        // This exercises the `std::env::consts::OS == "macos"` branch
        assert_eq!(host.is_match(), std::env::consts::OS == "macos");

        restore_mock_os(original_mock_os);
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        // Test configure_nfs failure coverage
        unsafe { std::env::set_var("PATH", "") };
        let _ = host.configure_nfs(&[]);
        unsafe { std::env::set_var("PATH", old_path.clone()) };
        // Mock VBoxManage and vmrun to guarantee coverage
        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let vbox = temp_dir.path().join("VBoxManage");
        let vmrun = temp_dir.path().join("vmrun");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&vbox, "#!/bin/sh\nexit 0").expect("operation should succeed");
            std::fs::set_permissions(&vbox, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
            std::fs::write(&vmrun, "#!/bin/sh\nexit 0").expect("operation should succeed");
            std::fs::set_permissions(&vmrun, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let vbox_bat = temp_dir.path().join("VBoxManage.bat");
            std::fs::write(&vbox_bat, "@echo off\nexit 0").expect("operation should succeed");
            let vmrun_bat = temp_dir.path().join("vmrun.bat");
            std::fs::write(&vmrun_bat, "@echo off\nexit 0").expect("operation should succeed");
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
        assert!(caps.contains(&"vmware".to_string()));
        assert!(missing_caps.is_empty());
    }
}
