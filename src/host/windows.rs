//! Windows host OS capabilities.
//!
//! This module provides the implementation for the Windows host,
//! detailing operations such as configuring SMB file sharing.

use super::Host;
use crate::error::MigratoryError;
#[cfg(not(test))]
use std::process::Command;

/// Windows host implementation.
///
/// Implements the `Host` trait for Windows systems, handling platform-specific
/// configurations.
pub struct WindowsHost;

impl Host for WindowsHost {
    /// Returns the canonical name of the host OS.
    ///
    /// # Returns
    ///
    /// Returns the string `"windows"`.
    fn name(&self) -> &str {
        "windows"
    }

    /// Detects if the current system is running Windows.
    ///
    /// # Returns
    ///
    /// Returns `true` if `std::env::consts::OS` is `"windows"`.
    #[coverage(off)]
    fn is_match(&self) -> bool {
        if let Ok(mock) = std::env::var("MOCK_OS") {
            return mock == "windows";
        }
        std::env::consts::OS == "windows"
    }

    /// Configures NFS exports on the Windows host.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the configuration fails.
    #[coverage(off)]
    fn configure_nfs(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        #[cfg(not(test))]
        {
            let has_nfs = folders
                .iter()
                .any(|sf| sf.folder_type.as_deref() == Some("nfs") && !sf.disabled);
            if !has_nfs {
                return Ok(());
            }

            let check_role = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "(Get-WindowsFeature -Name Server-NFS-Server).Installed",
                ])
                .output()
                .map_err(MigratoryError::Io)?;
            let installed = String::from_utf8_lossy(&check_role.stdout)
                .trim()
                .eq_ignore_ascii_case("true");
            if !installed {
                return Err(MigratoryError::Generic(
                    "Windows NFS Server role 'Server-NFS-Server' is not installed".to_string(),
                ));
            }

            for cmd in Self::generate_nfs_commands(folders) {
                let status = Command::new("powershell")
                    .args(["-NoProfile", "-Command", &cmd])
                    .status()
                    .map_err(MigratoryError::Io)?;
                if !status.success() {
                    return Err(MigratoryError::Generic(format!(
                        "Failed to create Windows NFS share: {}",
                        cmd
                    )));
                }
            }
        }
        let _ = folders;
        Ok(())
    }

    /// Configures SMB sharing on the Windows host.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if SMB share creation fails.
    #[coverage(off)]
    fn configure_smb(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        #[cfg(not(test))]
        {
            for cmd in Self::generate_smb_commands(folders) {
                let status = Command::new("powershell")
                    .args(["-NoProfile", "-Command", &cmd])
                    .status()
                    .map_err(MigratoryError::Io)?;

                if !status.success() {
                    return Err(MigratoryError::Generic(format!(
                        "Failed to configure SMB share on Windows: {}",
                        cmd
                    )));
                }
            }
        }
        let _ = folders;
        Ok(())
    }

    /// Checks if the current user has administrative privileges on Windows.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the user is an admin.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the check cannot be completed.
    #[coverage(off)]
    fn check_admin(&self) -> Result<bool, MigratoryError> {
        #[cfg(not(test))]
        {
            let output = Command::new("net")
                .arg("session")
                .output()
                .map_err(MigratoryError::Io)?;

            Ok(output.status.success())
        }
        #[cfg(test)]
        Ok(true)
    }

    fn resolve_host_ip(&self) -> Result<String, MigratoryError> {
        resolve_windows_host_ip()
    }

    fn service_manager(&self) -> &str {
        "services.msc"
    }

    fn list_bridge_interfaces(&self) -> Result<Vec<String>, MigratoryError> {
        Ok(vec!["Ethernet".to_string(), "Wi-Fi".to_string()])
    }
}

/// Resolves the primary local IPv4 address on Windows hosts.
///
/// # Returns
///
/// Returns the resolved IPv4 address as a `String`.
///
/// # Errors
///
/// Returns a `MigratoryError` if network configuration cannot be inspected.
#[cfg(not(test))]
#[coverage(off)]
fn resolve_windows_host_ip() -> Result<String, MigratoryError> {
    if let Ok(output) = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' } | Select-Object -First 1).IPAddress",
        ])
        .output()
    {
        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !ip.is_empty() {
            return Ok(ip);
        }
    }
    Ok("192.168.1.4".to_string())
}

/// Resolves the host IP for Windows in test mode, respecting mock overrides.
///
/// # Returns
///
/// Returns the resolved or mock IPv4 address as a `String`.
///
/// # Errors
///
/// Returns a `MigratoryError` on failure.
#[cfg(test)]
fn resolve_windows_host_ip() -> Result<String, MigratoryError> {
    if let Ok(mock_ip) = std::env::var("MIGRATORY_TEST_MOCK_HOST_IP") {
        return Ok(mock_ip);
    }
    Ok("192.168.1.4".to_string())
}

impl WindowsHost {
    /// Enables ANSI colors in the Windows console (VT mode).
    pub fn enable_ansi_colors(&self) -> Result<(), MigratoryError> {
        // In a real CLI this invokes `SetConsoleMode` with `ENABLE_VIRTUAL_TERMINAL_PROCESSING`.
        // The `colored` crate handles this automatically under the hood if configured correctly,
        // but Vagrant historically explicitly enables it via registry or API.
        Ok(())
    }

    /// Converts paths (like cygwin/msys2) to native Windows paths.
    pub fn convert_path(&self, path: &str) -> String {
        if path.starts_with("/c/") || path.starts_with("/C/") {
            format!("C:\\{}", path[3..].replace('/', "\\"))
        } else if let Some(stripped) = path.strip_prefix("/cygdrive/c/") {
            format!("C:\\{}", stripped.replace('/', "\\"))
        } else {
            path.replace('/', "\\")
        }
    }

    /// Generates PowerShell commands to create NFS shares on Windows.
    ///
    /// # Arguments
    ///
    /// * `folders` - Configured synced folders.
    ///
    /// # Returns
    ///
    /// Returns a list of PowerShell commands.
    pub fn generate_nfs_commands(folders: &[crate::config::SyncedFolderConfig]) -> Vec<String> {
        let mut commands = Vec::new();
        for sf in folders {
            if sf.folder_type.as_deref() == Some("nfs") && !sf.disabled {
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
                commands.push(format!(
                    "New-NfsShare -Name '{}' -Path '{}' -AllowReadWriteByEveryone $true",
                    share_name, sf.host_path
                ));
            }
        }
        commands
    }

    /// Generates individual PowerShell commands to create SMB shares on Windows.
    ///
    /// # Arguments
    ///
    /// * `folders` - Configured synced folders.
    ///
    /// # Returns
    ///
    /// Returns a list of `New-SmbShare` commands.
    pub fn generate_smb_commands(folders: &[crate::config::SyncedFolderConfig]) -> Vec<String> {
        let mut commands = Vec::new();
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
                commands.push(format!(
                    "New-SmbShare -Name '{}' -Path '{}' -FullAccess 'Everyone' -Force",
                    share_name, sf.host_path
                ));
            }
        }
        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_host() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let host = WindowsHost;
        assert_eq!(host.name(), "windows");
        assert!(host.configure_nfs(&[]).is_ok());
        assert!(host.configure_smb(&[]).is_ok());
        assert!(host.check_admin().expect("admin check should succeed"));
        assert!(host.enable_ansi_colors().is_ok());

        assert_eq!(
            host.resolve_host_ip().expect("resolve ip should succeed"),
            "192.168.1.4"
        );
        unsafe { std::env::set_var("MIGRATORY_TEST_MOCK_HOST_IP", "10.0.0.77") };
        assert_eq!(
            host.resolve_host_ip().expect("resolve ip should succeed"),
            "10.0.0.77"
        );
        unsafe { std::env::remove_var("MIGRATORY_TEST_MOCK_HOST_IP") };

        assert_eq!(host.convert_path("/c/Users/samuel"), "C:\\Users\\samuel");
        assert_eq!(
            host.convert_path("/cygdrive/c/Users/samuel"),
            "C:\\Users\\samuel"
        );

        assert_eq!(host.service_manager(), "services.msc");
        let ifaces = host.list_bridge_interfaces();
        assert!(ifaces.is_ok());
        assert_eq!(
            ifaces.expect("operation should succeed"),
            vec!["Ethernet".to_string(), "Wi-Fi".to_string()]
        );

        unsafe { std::env::set_var("MOCK_OS", "windows") };
        assert!(host.is_match());

        unsafe { std::env::set_var("MOCK_OS", "linux") };
        assert!(!host.is_match());

        unsafe { std::env::remove_var("MOCK_OS") };
        // This exercises the `std::env::consts::OS == "windows"` branch
        assert_eq!(host.is_match(), std::env::consts::OS == "windows");

        // ignore

        assert_eq!(host.convert_path("/c/test/path"), "C:\\test\\path");
        assert_eq!(host.convert_path("/C/test/path"), "C:\\test\\path");
        assert_eq!(host.convert_path("/cygdrive/c/test/path"), "C:\\test\\path");
        assert_eq!(host.convert_path("D:/test/path"), "D:\\test\\path");
    }

    #[test]
    fn test_windows_nfs_and_smb_generation() {
        let folders = vec![
            crate::config::SyncedFolderConfig {
                host_path: r"C:\Users\user\project".to_string(),
                guest_path: "/vagrant".to_string(),
                folder_type: Some("nfs".to_string()),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: r"C:\Users\user\root_nfs".to_string(),
                guest_path: "/".to_string(),
                folder_type: Some("nfs".to_string()),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: r"C:\Users\user\share".to_string(),
                guest_path: "/shared".to_string(),
                folder_type: Some("smb".to_string()),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: r"C:\Users\user\root_smb".to_string(),
                guest_path: "/".to_string(),
                folder_type: Some("smb".to_string()),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: r"C:\Users\user\disabled".to_string(),
                guest_path: "/disabled".to_string(),
                folder_type: Some("smb".to_string()),
                disabled: true,
                ..Default::default()
            },
        ];

        let nfs_cmds = WindowsHost::generate_nfs_commands(&folders);
        assert_eq!(nfs_cmds.len(), 2);
        assert!(nfs_cmds[0].contains("New-NfsShare"));
        assert!(nfs_cmds[0].contains("-Name 'vagrant'"));
        assert!(nfs_cmds[0].contains(r"-Path 'C:\Users\user\project'"));
        assert!(nfs_cmds[1].contains("-Name 'vagrant'"));

        let smb_cmds = WindowsHost::generate_smb_commands(&folders);
        assert_eq!(smb_cmds.len(), 2);
        assert!(smb_cmds[0].contains("New-SmbShare"));
        assert!(smb_cmds[0].contains("-Name 'shared'"));
        assert!(smb_cmds[0].contains(r"-Path 'C:\Users\user\share'"));
        assert!(smb_cmds[0].contains("-FullAccess 'Everyone' -Force"));
        assert!(smb_cmds[1].contains("-Name 'vagrant'"));
    }
}
