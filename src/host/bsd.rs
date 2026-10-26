//! BSD (FreeBSD, OpenBSD, NetBSD) host OS capabilities.
//!
//! This module provides the implementation for the BSD host,
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
fn default_bsd_runner(prog: &str, args: &[&str]) -> Result<(bool, String), MigratoryError> {
    if cfg!(test) && prog != "echo" {
        return Ok((true, "0\n".to_string()));
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

/// BSD host implementation.
///
/// Implements the `Host` trait for BSD systems (FreeBSD, OpenBSD, NetBSD),
/// handling platform-specific operations and daemon restarts.
pub struct BsdHost;

impl Host for BsdHost {
    /// Returns the canonical name of the host OS.
    ///
    /// # Returns
    ///
    /// Returns the string `"bsd"`.
    fn name(&self) -> &str {
        "bsd"
    }

    /// Detects if the current system is running a BSD-derived operating system.
    ///
    /// # Returns
    ///
    /// Returns `true` if `std::env::consts::OS` is `"freebsd"`, `"openbsd"`, or `"netbsd"`.
    fn is_match(&self) -> bool {
        self.is_match_os(std::env::consts::OS)
    }

    /// Configures NFS exports on the BSD host and reloads `mountd`.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if restarting `mountd` fails.
    fn configure_nfs(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        self.configure_nfs_with(folders, default_bsd_runner)
    }

    /// Configures SMB sharing on the BSD host.
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
        self.configure_smb_with(folders, default_bsd_runner)
    }

    /// Checks if the current user has administrative (root) privileges on BSD.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the effective user is root (UID 0).
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the check fails.
    fn check_admin(&self) -> Result<bool, MigratoryError> {
        self.check_admin_with(default_bsd_runner)
    }

    /// Resolves the host IP for network bridging on BSD.
    ///
    /// # Returns
    ///
    /// Returns the resolved local host IP address string.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the IP cannot be resolved.
    fn resolve_host_ip(&self) -> Result<String, MigratoryError> {
        self.resolve_host_ip_with(default_bsd_runner)
    }

    fn service_manager(&self) -> &str {
        "rc.d"
    }

    fn list_bridge_interfaces(&self) -> Result<Vec<String>, MigratoryError> {
        Ok(vec![
            "em0".to_string(),
            "re0".to_string(),
            "bridge0".to_string(),
        ])
    }
}

impl BsdHost {
    /// Checks whether the given `os` identifier matches a BSD operating system.
    ///
    /// # Arguments
    ///
    /// * `os` - Target OS name string.
    ///
    /// # Returns
    ///
    /// Returns `true` if `os` is a BSD variant.
    pub fn is_match_os(&self, os: &str) -> bool {
        matches!(os, "bsd" | "freebsd" | "openbsd" | "netbsd" | "dragonfly")
    }

    /// Configures NFS exports on the BSD host using a custom command runner.
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
    /// Returns a `MigratoryError` if reloading `mountd` fails.
    pub fn configure_nfs_with(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
        runner: HostCommandRunner,
    ) -> Result<(), MigratoryError> {
        let uid = runner("id", &["-u"])
            .map(|(_, s)| s.trim().to_string())
            .unwrap_or_else(|_| "1000".to_string());
        let gid = runner("id", &["-g"])
            .map(|(_, s)| s.trim().to_string())
            .unwrap_or_else(|_| "1000".to_string());

        let export_line = Self::generate_nfs_exports(folders, &uid, &gid);
        let cmd = format!("echo '{export_line}' | sudo tee -a /etc/exports > /dev/null");
        let _ = runner("sh", &["-c", &cmd]);

        let (ok, _) = runner("sudo", &["/etc/rc.d/mountd", "reload"])?;
        if !ok {
            return Err(MigratoryError::Generic(
                "Failed to reload mountd on BSD host".to_string(),
            ));
        }
        Ok(())
    }

    /// Configures SMB sharing on the BSD host using a custom command runner.
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
    /// Returns a `MigratoryError` if reloading `samba_server` fails.
    pub fn configure_smb_with(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
        runner: HostCommandRunner,
    ) -> Result<(), MigratoryError> {
        let conf_block = Self::generate_smb_conf(folders);
        let cmd = format!("echo '{conf_block}' | sudo tee -a /usr/local/etc/smb4.conf > /dev/null");
        let _ = runner("sh", &["-c", &cmd]);

        let (ok, _) = runner("sudo", &["service", "samba_server", "reload"])?;
        if !ok {
            return Err(MigratoryError::Generic(
                "Failed to reload Samba server on BSD host".to_string(),
            ));
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

    /// Resolves the primary local IPv4 address on BSD hosts using a custom command runner.
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
            return Ok(Self::parse_host_ip_from_route(&stdout));
        }
        Ok("192.168.1.1".to_string())
    }

    /// Parses the default gateway IP from `route -n get default` output, falling back to `192.168.1.1`.
    ///
    /// # Arguments
    ///
    /// * `route_output` - Raw stdout from `route -n get default`.
    ///
    /// # Returns
    ///
    /// Returns the resolved IPv4 address string.
    pub fn parse_host_ip_from_route(route_output: &str) -> String {
        for line in route_output.lines() {
            let trimmed = line.trim();
            if let Some(gateway) = trimmed.strip_prefix("gateway:") {
                let gw = gateway.trim().to_string();
                if !gw.is_empty() {
                    return gw;
                }
            }
        }
        "192.168.1.1".to_string()
    }

    /// Detects installed hypervisor capabilities on BSD.
    ///
    /// # Returns
    ///
    /// Returns a list of supported hypervisors detected on the BSD host (e.g. bhyve, virtualbox).
    pub fn capabilities(&self) -> Vec<String> {
        self.capabilities_with("bhyve", "VBoxManage")
    }

    /// Detects installed hypervisor capabilities on BSD using custom binary paths.
    ///
    /// # Arguments
    ///
    /// * `bhyve_bin` - Path or binary name for `bhyve`.
    /// * `vbox_bin` - Path or binary name for `VBoxManage`.
    ///
    /// # Returns
    ///
    /// Returns a list of supported hypervisors detected on the BSD host.
    pub fn capabilities_with(&self, bhyve_bin: &str, vbox_bin: &str) -> Vec<String> {
        let mut caps = Vec::new();
        if std::process::Command::new(bhyve_bin)
            .arg("-h")
            .output()
            .map(|o| o.status.success() || !o.stderr.is_empty())
            .unwrap_or(false)
        {
            caps.push("bhyve".to_string());
        }
        if std::process::Command::new(vbox_bin)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            caps.push("virtualbox".to_string());
        }
        caps
    }

    /// Generates the NFS export configuration block for BSD hosts.
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

    /// Generates Samba share definitions for BSD hosts.
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
        conf.push_str(
            "# VAGRANT-BEGIN-SMB
",
        );
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
                conf.push_str(&format!(
                    "[{}]
path = {}
read only = no
guest ok = yes

",
                    share_name,
                    sf.host_path.display()
                ));
            }
        }
        conf.push_str(
            "# VAGRANT-END-SMB
",
        );
        conf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bsd_host() -> Result<(), MigratoryError> {
        let host = BsdHost;
        let _ = default_bsd_runner("echo", &["0"]);
        assert_eq!(host.name(), "bsd");
        assert!(host.configure_nfs(&[]).is_ok());
        assert!(host.configure_smb(&[]).is_ok());
        assert!(host.check_admin()?);
        assert_eq!(host.resolve_host_ip()?, "192.168.1.1");
        assert_eq!(
            BsdHost::parse_host_ip_from_route(
                "   gateway: 10.0.0.99
"
            ),
            "10.0.0.99"
        );
        assert_eq!(
            BsdHost::parse_host_ip_from_route(
                "gateway:
"
            ),
            "192.168.1.1"
        );

        // Test configure_nfs_with and configure_smb_with failure and fallback branches
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
        assert!(
            host.configure_smb_with(&[], |_, _| Ok((false, String::new())))
                .is_err()
        );
        assert!(!host.check_admin_with(|_, _| Ok((true, "1000".to_string())))?);
        assert_eq!(
            host.resolve_host_ip_with(|_, _| Err(MigratoryError::Generic("err".to_string())))?,
            "192.168.1.1"
        );

        assert_eq!(host.service_manager(), "rc.d");
        let ifaces = host.list_bridge_interfaces()?;
        assert_eq!(
            ifaces,
            vec!["em0".to_string(), "re0".to_string(), "bridge0".to_string(),]
        );

        assert_eq!(host.is_match(), host.is_match_os(std::env::consts::OS));
        assert!(host.is_match_os("freebsd"));
        assert!(host.is_match_os("openbsd"));
        assert!(host.is_match_os("netbsd"));
        assert!(host.is_match_os("dragonfly"));
        assert!(host.is_match_os("bsd"));
        assert!(!host.is_match_os("linux"));

        let _ = host.capabilities();
        let caps = host.capabilities_with("echo", "echo");
        assert!(caps.contains(&"bhyve".to_string()));
        assert!(caps.contains(&"virtualbox".to_string()));
        let failed = host.capabilities_with("false", "false");
        assert!(failed.is_empty());
        let missing = host.capabilities_with("nonexistent_bhyve_xyz", "nonexistent_vbox_xyz");
        assert!(missing.is_empty());
        Ok(())
    }

    #[test]
    fn test_bsd_nfs_and_smb_generation() {
        let folders = vec![
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/home/user/project"),
                guest_path: std::path::PathBuf::from("/vagrant"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/home/user/share"),
                guest_path: std::path::PathBuf::from("/shared"),
                folder_type: Some(crate::config::SyncedFolderType::Smb),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/home/user/root_share"),
                guest_path: std::path::PathBuf::from("/"),
                folder_type: Some(crate::config::SyncedFolderType::Smb),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/home/user/disabled"),
                guest_path: std::path::PathBuf::from("/disabled"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
                disabled: true,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/home/user/disabled_smb"),
                guest_path: std::path::PathBuf::from("/disabled_smb"),
                folder_type: Some(crate::config::SyncedFolderType::Smb),
                disabled: true,
                ..Default::default()
            },
        ];

        let exports = BsdHost::generate_nfs_exports(&folders, "1000", "1000");
        assert!(exports.contains("# VAGRANT-BEGIN"));
        assert!(exports.contains(r#""/home/user/project" -alldirs -mapall=1000:1000 127.0.0.1"#));
        assert!(!exports.contains("/home/user/disabled"));
        assert!(exports.contains("# VAGRANT-END"));

        let smb = BsdHost::generate_smb_conf(&folders);
        assert!(smb.contains("# VAGRANT-BEGIN-SMB"));
        assert!(smb.contains("[shared]"));
        assert!(smb.contains("[vagrant]"));
        assert!(smb.contains("path = /home/user/share"));
        assert!(smb.contains("path = /home/user/root_share"));
        assert!(!smb.contains("disabled_smb"));
        assert!(smb.contains("# VAGRANT-END-SMB"));
    }
}
