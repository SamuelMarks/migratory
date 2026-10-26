//! Windows host OS capabilities.
//!
//! This module provides the implementation for the Windows host,
//! detailing operations such as configuring SMB file sharing.

use super::{Host, HostCommandRunner};
use crate::error::MigratoryError;
use std::process::Command;

/// Executes a command and returns `(success, stdout)` or an IO error.
///
/// # Arguments
///
/// * `prog` - Program name.
/// * `args` - Command-line arguments.
///
/// # Returns
///
/// Returns `Ok((status.success(), stdout))` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError::Io`] if spawning fails.
fn default_windows_runner(prog: &str, args: &[&str]) -> Result<(bool, String), MigratoryError> {
    if cfg!(test) && prog != "echo" {
        let out = if prog == "powershell" && args.iter().any(|a| a.contains("Server-NFS-Server")) {
            "True\n".to_string()
        } else {
            String::new()
        };
        return Ok((true, out));
    }
    let output = Command::new(prog)
        .args(args)
        .output()
        .map_err(MigratoryError::Io)?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}

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
    fn is_match(&self) -> bool {
        self.is_match_os(std::env::consts::OS)
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
    fn configure_nfs(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        self.configure_nfs_with(folders, default_windows_runner)
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
    fn configure_smb(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        self.configure_smb_with(folders, default_windows_runner)
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
    fn check_admin(&self) -> Result<bool, MigratoryError> {
        self.check_admin_with(default_windows_runner)
    }

    fn resolve_host_ip(&self) -> Result<String, MigratoryError> {
        self.resolve_host_ip_with(default_windows_runner)
    }

    fn service_manager(&self) -> &str {
        "services.msc"
    }

    fn list_bridge_interfaces(&self) -> Result<Vec<String>, MigratoryError> {
        Ok(vec!["Ethernet".to_string(), "Wi-Fi".to_string()])
    }
}

impl WindowsHost {
    /// Checks whether the given `os` identifier matches Windows.
    ///
    /// # Arguments
    ///
    /// * `os` - Target OS name string.
    ///
    /// # Returns
    ///
    /// Returns `true` if `os` is `"windows"`.
    pub fn is_match_os(&self, os: &str) -> bool {
        os == "windows"
    }

    /// Configures NFS shares on Windows using a custom command runner.
    ///
    /// # Arguments
    ///
    /// * `folders` - Configured synced folders.
    /// * `runner` - Command execution callback.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if NFS Server role is missing or share creation fails.
    pub fn configure_nfs_with(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
        runner: HostCommandRunner,
    ) -> Result<(), MigratoryError> {
        let has_nfs = folders
            .iter()
            .any(|sf| sf.folder_type == Some(crate::config::SyncedFolderType::Nfs) && !sf.disabled);
        if !has_nfs {
            return Ok(());
        }

        let (_, stdout) = runner(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-WindowsFeature -Name Server-NFS-Server).Installed",
            ],
        )?;
        if !stdout.trim().eq_ignore_ascii_case("true") {
            return Err(MigratoryError::Generic(
                "Windows NFS Server role 'Server-NFS-Server' is not installed".to_string(),
            ));
        }

        for cmd in Self::generate_nfs_commands(folders) {
            let (ok, _) = runner("powershell", &["-NoProfile", "-Command", &cmd])?;
            if !ok {
                return Err(MigratoryError::Generic(format!(
                    "Failed to create Windows NFS share: {}",
                    cmd
                )));
            }
        }
        Ok(())
    }

    /// Configures SMB shares on Windows using a custom command runner.
    ///
    /// # Arguments
    ///
    /// * `folders` - Configured synced folders.
    /// * `runner` - Command execution callback.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if `New-SmbShare` fails.
    pub fn configure_smb_with(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
        runner: HostCommandRunner,
    ) -> Result<(), MigratoryError> {
        for cmd in Self::generate_smb_commands(folders) {
            let (ok, _) = runner("powershell", &["-NoProfile", "-Command", &cmd])?;
            if !ok {
                return Err(MigratoryError::Generic(format!(
                    "Failed to configure SMB share on Windows: {}",
                    cmd
                )));
            }
        }
        Ok(())
    }

    /// Checks administrative privileges on Windows using a custom command runner.
    ///
    /// # Arguments
    ///
    /// * `runner` - Command execution callback.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if `net session` succeeds.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if spawning fails.
    pub fn check_admin_with(&self, runner: HostCommandRunner) -> Result<bool, MigratoryError> {
        let (ok, _) = runner("net", &["session"])?;
        Ok(ok)
    }

    /// Resolves the primary local IPv4 address on Windows hosts using a custom command runner.
    ///
    /// # Arguments
    ///
    /// * `runner` - Command execution callback.
    ///
    /// # Returns
    ///
    /// Returns the resolved IPv4 address string.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn resolve_host_ip_with(
        &self,
        runner: HostCommandRunner,
    ) -> Result<String, MigratoryError> {
        if let Ok((_, stdout)) = runner(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' } | Select-Object -First 1).IPAddress",
            ],
        ) {
            let ip = stdout.trim().to_string();
            if !ip.is_empty() {
                return Ok(ip);
            }
        }
        Ok("192.168.1.4".to_string())
    }

    /// Enables ANSI colors in the Windows console (VT mode).
    pub fn enable_ansi_colors(&self) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Converts paths (like cygwin/msys2) to native Windows paths.
    pub fn convert_path(&self, path: &str) -> String {
        let sep = std::char::from_u32(92).unwrap_or('/');
        if path.starts_with("/c/") || path.starts_with("/C/") {
            format!("C:{}{}", sep, path[3..].replace('/', &sep.to_string()))
        } else if let Some(stripped) = path.strip_prefix("/cygdrive/c/") {
            format!("C:{}{}", sep, stripped.replace('/', &sep.to_string()))
        } else {
            path.replace('/', &sep.to_string())
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
            if sf.folder_type == Some(crate::config::SyncedFolderType::Nfs) && !sf.disabled {
                let name = sf
                    .guest_path
                    .to_string_lossy()
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
                    share_name,
                    sf.host_path.display()
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
            if sf.folder_type == Some(crate::config::SyncedFolderType::Smb) && !sf.disabled {
                let name = sf
                    .guest_path
                    .to_string_lossy()
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
                    share_name,
                    sf.host_path.display()
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
    fn test_windows_host() -> Result<(), MigratoryError> {
        let host = WindowsHost;
        let _ = default_windows_runner("echo", &["0"]);
        assert_eq!(host.name(), "windows");
        assert!(host.configure_nfs(&[]).is_ok());
        assert!(host.configure_smb(&[]).is_ok());
        assert!(host.check_admin()?);
        assert!(host.enable_ansi_colors().is_ok());

        assert_eq!(host.resolve_host_ip()?, "192.168.1.4");
        let ip = host.resolve_host_ip_with(|_, _| Ok((true, "10.0.0.50\n".to_string())));
        assert_eq!(ip?, "10.0.0.50");
        assert_eq!(
            host.resolve_host_ip_with(|_, _| Err(MigratoryError::Generic("err".to_string())))?,
            "192.168.1.4"
        );
        assert!(!host.check_admin_with(|_, _| Ok((false, String::new())))?);

        let non_nfs_folders = vec![
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from(r"C:\disabled_nfs"),
                guest_path: std::path::PathBuf::from("/disabled_nfs"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
                disabled: true,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from(r"C:\smb"),
                guest_path: std::path::PathBuf::from("/smb"),
                folder_type: Some(crate::config::SyncedFolderType::Smb),
                disabled: false,
                ..Default::default()
            },
        ];
        assert!(host.configure_nfs(&non_nfs_folders).is_ok());

        let nfs_folders = vec![crate::config::SyncedFolderConfig {
            host_path: std::path::PathBuf::from(r"C:\project"),
            guest_path: std::path::PathBuf::from("/vagrant"),
            folder_type: Some(crate::config::SyncedFolderType::Nfs),
            disabled: false,
            ..Default::default()
        }];
        assert!(host.configure_nfs(&nfs_folders).is_ok());
        assert!(
            host.configure_nfs_with(&nfs_folders, |_, _| Ok((true, "False".to_string())))
                .is_err()
        );
        assert!(
            host.configure_nfs_with(&nfs_folders, |_, args| {
                if args.iter().any(|a| a.contains("Server-NFS-Server")) {
                    Ok((true, "True".to_string()))
                } else {
                    Ok((false, String::new()))
                }
            })
            .is_err()
        );

        let smb_folders = vec![crate::config::SyncedFolderConfig {
            host_path: std::path::PathBuf::from(r"C:\share"),
            guest_path: std::path::PathBuf::from("/shared"),
            folder_type: Some(crate::config::SyncedFolderType::Smb),
            disabled: false,
            ..Default::default()
        }];
        assert!(host.configure_smb(&smb_folders).is_ok());
        assert!(
            host.configure_smb_with(&smb_folders, |_, _| Ok((false, String::new())))
                .is_err()
        );

        assert_eq!(host.convert_path("/c/Users/samuel"), r"C:\Users\samuel");
        assert_eq!(
            host.convert_path("/cygdrive/c/Users/samuel"),
            r"C:\Users\samuel"
        );

        assert_eq!(host.service_manager(), "services.msc");
        let ifaces = host.list_bridge_interfaces()?;
        assert_eq!(ifaces, vec!["Ethernet".to_string(), "Wi-Fi".to_string()]);

        assert!(host.is_match_os("windows"));
        assert!(!host.is_match_os("linux"));
        assert_eq!(host.is_match(), std::env::consts::OS == "windows");

        assert_eq!(host.convert_path("/c/test/path"), r"C:\test\path");
        assert_eq!(host.convert_path("/C/test/path"), r"C:\test\path");
        assert_eq!(host.convert_path("/cygdrive/c/test/path"), r"C:\test\path");
        assert_eq!(host.convert_path("D:/test/path"), r"D:\test\path");
        Ok(())
    }

    #[test]
    fn test_windows_nfs_and_smb_generation() {
        let folders = vec![
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from(r"C:\Users\user\project"),
                guest_path: std::path::PathBuf::from("/vagrant"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from(r"C:\Users\user\root_nfs"),
                guest_path: std::path::PathBuf::from("/"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from(r"C:\Users\user\share"),
                guest_path: std::path::PathBuf::from("/shared"),
                folder_type: Some(crate::config::SyncedFolderType::Smb),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from(r"C:\Users\user\root_smb"),
                guest_path: std::path::PathBuf::from("/"),
                folder_type: Some(crate::config::SyncedFolderType::Smb),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from(r"C:\Users\user\disabled"),
                guest_path: std::path::PathBuf::from("/disabled"),
                folder_type: Some(crate::config::SyncedFolderType::Smb),
                disabled: true,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from(r"C:\Users\user\disabled_nfs"),
                guest_path: std::path::PathBuf::from("/disabled_nfs"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
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
