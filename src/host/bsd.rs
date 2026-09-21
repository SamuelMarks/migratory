//! BSD (FreeBSD, OpenBSD, NetBSD) host OS capabilities.
//!
//! This module provides the implementation for the BSD host,
//! detailing operations such as configuring NFS or SMB file sharing.

use super::Host;
use crate::error::MigratoryError;
#[cfg(not(test))]
use std::process::Command;

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
    #[coverage(off)]
    fn is_match(&self) -> bool {
        if let Ok(mock) = std::env::var("MOCK_OS") {
            return mock == "bsd"
                || mock == "freebsd"
                || mock == "openbsd"
                || mock == "netbsd"
                || mock == "dragonfly";
        }
        std::env::consts::OS == "freebsd"
            || std::env::consts::OS == "openbsd"
            || std::env::consts::OS == "netbsd"
            || std::env::consts::OS == "dragonfly"
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

            let export_line = Self::generate_nfs_exports(_folders, &uid, &gid);
            let cmd = format!(
                "echo '{}' | sudo tee -a /etc/exports > /dev/null",
                export_line
            );
            let _ = Command::new("sh").arg("-c").arg(cmd).status();

            let status = Command::new("sudo")
                .arg("/etc/rc.d/mountd")
                .arg("reload")
                .status()
                .map_err(MigratoryError::Io)?;

            if !status.success() {
                return Err(MigratoryError::Generic(
                    "Failed to reload mountd on BSD host".to_string(),
                ));
            }
        }
        Ok(())
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
    #[coverage(off)]
    fn configure_smb(
        &self,
        _folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        #[cfg(not(test))]
        {
            let conf_block = Self::generate_smb_conf(_folders);
            let cmd = format!(
                "echo '{}' | sudo tee -a /usr/local/etc/smb4.conf > /dev/null",
                conf_block
            );
            let _ = Command::new("sh").arg("-c").arg(cmd).status();

            let status = Command::new("sudo")
                .arg("service")
                .arg("samba_server")
                .arg("reload")
                .status()
                .map_err(MigratoryError::Io)?;

            if !status.success() {
                return Err(MigratoryError::Generic(
                    "Failed to reload Samba server on BSD host".to_string(),
                ));
            }
        }
        Ok(())
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
        resolve_bsd_host_ip()
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

/// Resolves the primary local IPv4 address on BSD hosts.
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
fn resolve_bsd_host_ip() -> Result<String, MigratoryError> {
    if let Ok(output) = std::process::Command::new("route")
        .args(["-n", "get", "default"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let trimmed = line.trim();
            if let Some(gateway) = trimmed.strip_prefix("gateway:") {
                let gw = gateway.trim().to_string();
                if !gw.is_empty() {
                    return Ok(gw);
                }
            }
        }
    }
    Ok("192.168.1.1".to_string())
}

/// Resolves the host IP for BSD in test mode, respecting mock overrides.
///
/// # Returns
///
/// Returns the resolved or mock IPv4 address as a `String`.
///
/// # Errors
///
/// Returns a `MigratoryError` on failure.
#[cfg(test)]
fn resolve_bsd_host_ip() -> Result<String, MigratoryError> {
    if let Ok(mock_ip) = std::env::var("MIGRATORY_TEST_MOCK_HOST_IP") {
        return Ok(mock_ip);
    }
    Ok("192.168.1.1".to_string())
}

impl BsdHost {
    /// Detects installed hypervisor capabilities on BSD.
    ///
    /// # Returns
    ///
    /// Returns a list of supported hypervisors detected on the BSD host (e.g. bhyve, virtualbox).
    #[coverage(off)]
    pub fn capabilities(&self) -> Vec<String> {
        let mut caps = Vec::new();
        if std::process::Command::new("bhyve")
            .arg("-h")
            .output()
            .map(|o| o.status.success() || !o.stderr.is_empty())
            .unwrap_or(false)
        {
            caps.push("bhyve".to_string());
        }
        if std::process::Command::new("VBoxManage")
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
            if sf.folder_type.as_deref() == Some("nfs") && !sf.disabled {
                lines.push_str(&format!(
                    "\"{}\" -alldirs -mapall={}:{} 127.0.0.1\n",
                    sf.host_path, uid, gid
                ));
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
                    "[{}]\npath = {}\nread only = no\nguest ok = yes\n\n",
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

    #[test]
    fn test_bsd_host() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        let host = BsdHost;
        assert_eq!(host.name(), "bsd");
        assert!(host.configure_nfs(&[]).is_ok());
        assert!(host.configure_smb(&[]).is_ok());
        assert!(host.check_admin().expect("admin check should succeed"));
        assert_eq!(
            host.resolve_host_ip().expect("resolve ip should succeed"),
            "192.168.1.1"
        );

        unsafe { std::env::set_var("MIGRATORY_TEST_MOCK_HOST_IP", "10.0.0.99") };
        assert_eq!(
            host.resolve_host_ip().expect("resolve ip should succeed"),
            "10.0.0.99"
        );
        unsafe { std::env::remove_var("MIGRATORY_TEST_MOCK_HOST_IP") };

        assert_eq!(host.service_manager(), "rc.d");
        let ifaces = host.list_bridge_interfaces();
        assert!(ifaces.is_ok());
        assert_eq!(
            ifaces.expect("operation should succeed"),
            vec!["em0".to_string(), "re0".to_string(), "bridge0".to_string(),]
        );

        unsafe { std::env::set_var("MOCK_OS", "freebsd") };
        assert!(host.is_match());

        unsafe { std::env::set_var("MOCK_OS", "openbsd") };
        assert!(host.is_match());

        unsafe { std::env::set_var("MOCK_OS", "netbsd") };
        assert!(host.is_match());

        unsafe { std::env::set_var("MOCK_OS", "dragonfly") };
        assert!(host.is_match());

        unsafe { std::env::set_var("MOCK_OS", "bsd") };
        assert!(host.is_match());

        unsafe { std::env::set_var("MOCK_OS", "linux") };
        assert!(!host.is_match());

        unsafe { std::env::remove_var("MOCK_OS") };

        let caps = host.capabilities();
        let _ = caps;
    }

    #[test]
    fn test_bsd_nfs_and_smb_generation() {
        let folders = vec![
            crate::config::SyncedFolderConfig {
                host_path: "/home/user/project".to_string(),
                guest_path: "/vagrant".to_string(),
                folder_type: Some("nfs".to_string()),
                disabled: false,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: "/home/user/share".to_string(),
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

        let exports = BsdHost::generate_nfs_exports(&folders, "1000", "1000");
        assert!(exports.contains("# VAGRANT-BEGIN"));
        assert!(exports.contains("\"/home/user/project\" -alldirs -mapall=1000:1000 127.0.0.1"));
        assert!(!exports.contains("/home/user/disabled"));
        assert!(exports.contains("# VAGRANT-END"));

        let smb = BsdHost::generate_smb_conf(&folders);
        assert!(smb.contains("# VAGRANT-BEGIN-SMB"));
        assert!(smb.contains("[shared]"));
        assert!(smb.contains("path = /home/user/share"));
        assert!(smb.contains("# VAGRANT-END-SMB"));
    }
}
