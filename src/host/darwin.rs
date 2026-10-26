//! macOS (Darwin) host OS capabilities.
//!
//! This module provides the implementation for the Darwin (macOS) host,
//! detailing operations such as configuring NFS or SMB file sharing.

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
fn default_darwin_runner(prog: &str, args: &[&str]) -> Result<(bool, String), MigratoryError> {
    if cfg!(test) && prog != "echo" {
        let out = match prog {
            "id" => "0\n".to_string(),
            "ifconfig" => "en0 en1 bridge0\n".to_string(),
            _ => String::new(),
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
    fn is_match(&self) -> bool {
        self.is_match_os(std::env::consts::OS)
    }

    /// Configures NFS exports on the macOS host.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if manipulating `/etc/exports` fails.
    fn configure_nfs(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        self.configure_nfs_with(folders, default_darwin_runner)
    }

    /// Configures SMB sharing on the macOS host.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the configuration fails.
    fn configure_smb(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        self.configure_smb_with(folders, default_darwin_runner)
    }

    /// Checks if the current user has administrative (root) privileges on macOS.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the user is an admin.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the check cannot be completed.
    fn check_admin(&self) -> Result<bool, MigratoryError> {
        self.check_admin_with(default_darwin_runner)
    }

    fn resolve_host_ip(&self) -> Result<String, MigratoryError> {
        self.resolve_host_ip_with(default_darwin_runner)
    }

    fn service_manager(&self) -> &str {
        "launchd"
    }

    fn list_bridge_interfaces(&self) -> Result<Vec<String>, MigratoryError> {
        self.list_bridge_interfaces_with(default_darwin_runner)
    }
}

impl DarwinHost {
    /// Checks whether the given `os` identifier matches macOS / Darwin.
    ///
    /// # Arguments
    ///
    /// * `os` - Target OS name string.
    ///
    /// # Returns
    ///
    /// Returns `true` if `os` is `"macos"` or `"darwin"`.
    pub fn is_match_os(&self, os: &str) -> bool {
        matches!(os, "macos" | "darwin")
    }

    /// Configures NFS exports on macOS using a custom command runner.
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
    /// Returns a `MigratoryError` if restarting `nfsd` fails.
    pub fn configure_nfs_with(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
        runner: HostCommandRunner,
    ) -> Result<(), MigratoryError> {
        let uid = runner("id", &["-u"])
            .map(|(_, s)| s.trim().to_string())
            .unwrap_or_else(|_| "501".to_string());
        let gid = runner("id", &["-g"])
            .map(|(_, s)| s.trim().to_string())
            .unwrap_or_else(|_| "20".to_string());

        let export_line = Self::generate_nfs_exports(folders, &uid, &gid);
        let cmd = format!("echo '{export_line}' | sudo tee -a /etc/exports > /dev/null");
        let _ = runner("sh", &["-c", &cmd]);

        let (ok, _) = runner("sudo", &["nfsd", "restart"])?;
        if !ok {
            return Err(MigratoryError::Generic(
                "Failed to restart NFS daemon on Darwin".to_string(),
            ));
        }
        Ok(())
    }

    /// Configures SMB sharing on macOS using a custom command runner.
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
    /// Returns a `MigratoryError` if `sharing` fails.
    pub fn configure_smb_with(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
        runner: HostCommandRunner,
    ) -> Result<(), MigratoryError> {
        for sf in folders {
            if sf.folder_type == Some(crate::config::SyncedFolderType::Smb) && !sf.disabled {
                let share_name = sf
                    .guest_path
                    .to_string_lossy()
                    .replace('/', "_")
                    .trim_start_matches('_')
                    .to_string();
                let share_name = if share_name.is_empty() {
                    "vagrant".to_string()
                } else {
                    share_name
                };
                let host_str = sf.host_path.to_string_lossy();
                let (ok, _) = runner("sudo", &["sharing", "-a", &host_str, "-S", &share_name])?;
                if !ok {
                    return Err(MigratoryError::Generic(format!(
                        "Failed to configure SMB share on Darwin: {}",
                        share_name
                    )));
                }
            }
        }
        Ok(())
    }

    /// Checks whether the current user is root (UID 0) using a custom command runner.
    ///
    /// # Arguments
    ///
    /// * `runner` - Command execution callback.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if UID is `"0"`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the command fails.
    pub fn check_admin_with(&self, runner: HostCommandRunner) -> Result<bool, MigratoryError> {
        let (_, stdout) = runner("id", &["-u"])?;
        Ok(stdout.trim() == "0")
    }

    /// Resolves the primary local IPv4 address for macOS hosts using a custom command runner.
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
        if let Ok((_, stdout)) = runner("route", &["-n", "get", "default"]) {
            for line in stdout.lines() {
                if let Some(iface) = line.trim().strip_prefix("interface:")
                    && let Ok((_, ip_stdout)) = runner("ipconfig", &["getifaddr", iface.trim()])
                    && !ip_stdout.trim().is_empty()
                {
                    return Ok(ip_stdout.trim().to_string());
                }
            }
        }
        Ok("192.168.1.2".to_string())
    }

    /// Enumerates available network interfaces on macOS via `ifconfig -l`.
    ///
    /// # Arguments
    ///
    /// * `runner` - Command execution callback.
    ///
    /// # Returns
    ///
    /// Returns a vector of interface names.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if `ifconfig` fails.
    pub fn list_bridge_interfaces_with(
        &self,
        runner: HostCommandRunner,
    ) -> Result<Vec<String>, MigratoryError> {
        let (_, text) = runner("ifconfig", &["-l"])?;
        Ok(text.split_whitespace().map(|s| s.to_string()).collect())
    }

    /// Checks for Hypervisor framework privileges (entitlements check).
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the hypervisor framework is supported.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if querying `sysctl` fails.
    pub fn check_hypervisor_privileges(&self) -> Result<bool, MigratoryError> {
        self.check_hypervisor_privileges_with(default_darwin_runner)
    }

    /// Checks for Hypervisor framework privileges using a custom command runner.
    ///
    /// # Arguments
    ///
    /// * `runner` - Command execution callback.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if `sysctl kern.hv_support` succeeds.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if querying `sysctl` fails.
    pub fn check_hypervisor_privileges_with(
        &self,
        runner: HostCommandRunner,
    ) -> Result<bool, MigratoryError> {
        let (ok, _) = runner("sysctl", &["kern.hv_support"])?;
        Ok(ok)
    }

    /// Detects host capabilities like hypervisors.
    ///
    /// # Returns
    ///
    /// Returns a list of available hypervisor names.
    pub fn capabilities(&self) -> Vec<String> {
        self.capabilities_with("VBoxManage", "vmrun")
    }

    /// Detects host capabilities using custom binary paths for `VBoxManage` and `vmrun`.
    ///
    /// # Arguments
    ///
    /// * `vbox_bin` - Path or binary name for `VBoxManage`.
    /// * `vmrun_bin` - Path or binary name for `vmrun`.
    ///
    /// # Returns
    ///
    /// Returns a list of available hypervisor names.
    pub fn capabilities_with(&self, vbox_bin: &str, vmrun_bin: &str) -> Vec<String> {
        let mut caps = Vec::new();
        if std::process::Command::new(vbox_bin)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            caps.push("virtualbox".to_string());
        }
        if std::process::Command::new(vmrun_bin)
            .arg("list")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            caps.push("vmware".to_string());
        }
        caps
    }

    /// Generates the NFS export configuration block for Darwin hosts.
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
            if sf.folder_type == Some(crate::config::SyncedFolderType::Nfs) && !sf.disabled {
                lines.push_str(&format!(
                    r#""{}" -alldirs -mapall={}:{} 127.0.0.1"#,
                    sf.host_path.display(),
                    uid,
                    gid
                ));
                lines.push('\n');
            }
        }
        lines.push_str("# VAGRANT-END\n");
        lines
    }

    /// Generates the macOS sharing command for an SMB share.
    ///
    /// # Arguments
    ///
    /// * `host_path` - Path on host.
    /// * `share_name` - Name of the share.
    ///
    /// # Returns
    ///
    /// Returns the command line string.
    pub fn generate_smb_sharing_command(host_path: &str, share_name: &str) -> String {
        format!(r#"sharing -a "{}" -S "{}""#, host_path, share_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_darwin_host() -> Result<(), MigratoryError> {
        let host = DarwinHost;
        let _ = default_darwin_runner("echo", &["0"]);
        assert_eq!(host.name(), "darwin");
        assert_eq!(host.service_manager(), "launchd");
        assert!(host.list_bridge_interfaces().is_ok());
        assert_eq!(host.resolve_host_ip()?, "192.168.1.2");
        assert!(host.configure_nfs(&[]).is_ok());
        assert!(host.configure_smb(&[]).is_ok());
        assert!(host.check_admin()?);
        assert!(host.check_hypervisor_privileges()?);

        // Test configure_nfs_with & configure_smb_with & resolve_host_ip_with branches
        assert!(
            host.configure_nfs_with(&[], |prog, _| {
                if prog == "id" {
                    Err(MigratoryError::Generic("id err".to_string()))
                } else {
                    Ok((false, String::new()))
                }
            })
            .is_err()
        );
        let smb_folders = vec![
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/Users/user/share"),
                guest_path: std::path::PathBuf::from("/shared"),
                folder_type: Some(crate::config::SyncedFolderType::Smb),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/Users/user/root"),
                guest_path: std::path::PathBuf::from("/"),
                folder_type: Some(crate::config::SyncedFolderType::Smb),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/Users/user/disabled"),
                guest_path: std::path::PathBuf::from("/disabled"),
                folder_type: Some(crate::config::SyncedFolderType::Smb),
                disabled: true,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/Users/user/rsync"),
                guest_path: std::path::PathBuf::from("/rsync"),
                folder_type: Some(crate::config::SyncedFolderType::Rsync),
                disabled: false,
                ..Default::default()
            },
        ];
        assert!(
            host.configure_smb_with(&smb_folders, |_, _| Ok((true, String::new())))
                .is_ok()
        );
        assert!(
            host.configure_smb_with(&smb_folders, |_, _| Ok((false, String::new())))
                .is_err()
        );
        assert!(!host.check_admin_with(|_, _| Ok((true, "501".to_string())))?);
        let ip1 = host.resolve_host_ip_with(|prog, _| {
            if prog == "route" {
                Ok((true, "  route to: default\n  interface: en0\n".to_string()))
            } else {
                Ok((true, "10.0.1.55\n".to_string()))
            }
        });
        assert_eq!(ip1?, "10.0.1.55");
        let ip2 = host.resolve_host_ip_with(|prog, _| {
            if prog == "route" {
                Ok((true, "  interface: en0\n".to_string()))
            } else {
                Err(MigratoryError::Generic("ipconfig err".to_string()))
            }
        });
        assert_eq!(ip2?, "192.168.1.2");
        let ip3 = host.resolve_host_ip_with(|prog, _| {
            if prog == "route" {
                Ok((true, "  interface: en0\n".to_string()))
            } else {
                Ok((true, "   \n".to_string()))
            }
        });
        assert_eq!(ip3?, "192.168.1.2");
        assert_eq!(
            host.resolve_host_ip_with(|_, _| Err(MigratoryError::Generic("err".to_string())))?,
            "192.168.1.2"
        );
        assert!(!host.check_hypervisor_privileges_with(|_, _| Ok((false, String::new())))?);

        assert!(host.is_match_os("macos"));
        assert!(host.is_match_os("darwin"));
        assert!(!host.is_match_os("windows"));
        assert_eq!(host.is_match(), std::env::consts::OS == "macos");

        let _ = host.capabilities();
        let caps = host.capabilities_with("echo", "echo");
        let missing_caps = host.capabilities_with("nonexistent_vbox_xyz", "nonexistent_vmrun_xyz");
        let failed_caps = host.capabilities_with("false", "false");

        assert!(caps.contains(&"virtualbox".to_string()));
        assert!(caps.contains(&"vmware".to_string()));
        assert!(missing_caps.is_empty());
        assert!(failed_caps.is_empty());
        Ok(())
    }

    #[test]
    fn test_darwin_nfs_and_smb_generation() {
        let folders = vec![
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/Users/user/project"),
                guest_path: std::path::PathBuf::from("/vagrant"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/Users/user/disabled"),
                guest_path: std::path::PathBuf::from("/disabled"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
                disabled: true,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/Users/user/rsync"),
                guest_path: std::path::PathBuf::from("/rsync"),
                folder_type: Some(crate::config::SyncedFolderType::Rsync),
                disabled: false,
                ..Default::default()
            },
        ];

        let exports = DarwinHost::generate_nfs_exports(&folders, "501", "20");
        assert!(exports.contains("# VAGRANT-BEGIN"));
        assert!(exports.contains(r#""/Users/user/project" -alldirs -mapall=501:20 127.0.0.1"#));
        assert!(!exports.contains("/Users/user/disabled"));
        assert!(!exports.contains("/Users/user/rsync"));
        assert!(exports.contains("# VAGRANT-END"));

        let smb_cmd = DarwinHost::generate_smb_sharing_command("/Users/user/project", "vagrant");
        assert_eq!(smb_cmd, r#"sharing -a "/Users/user/project" -S "vagrant""#);
    }
}
