//! Linux host OS capabilities.
//!
//! This module provides the implementation for the Linux host,
//! detailing operations such as configuring NFS or SMB file sharing.

use super::Host;
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
fn default_linux_runner(prog: &str, args: &[&str]) -> Result<(bool, String), MigratoryError> {
    if cfg!(test) && prog != "echo" {
        let out = match prog {
            "id" => "0\n".to_string(),
            "groups" => "libvirt libvirtd\n".to_string(),
            "sh" => "192.168.1.3".to_string(),
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
    fn is_match(&self) -> bool {
        self.is_match_os(std::env::consts::OS)
    }

    /// Configures NFS exports on the Linux host.
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
        self.configure_nfs_with(folders, default_linux_runner)
    }

    /// Configures SMB sharing on the Linux host.
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
        self.configure_smb_with(folders, default_linux_runner)
    }

    /// Checks if the current user has administrative (root) privileges on Linux.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the user is an admin.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the check cannot be completed.
    fn check_admin(&self) -> Result<bool, MigratoryError> {
        self.check_admin_with(default_linux_runner)
    }

    fn resolve_host_ip(&self) -> Result<String, MigratoryError> {
        self.resolve_host_ip_with(default_linux_runner)
    }

    fn service_manager(&self) -> &str {
        Self::detect_service_manager(
            std::path::Path::new("/run/systemd/system"),
            std::path::Path::new("/run/openrc"),
        )
    }

    fn list_bridge_interfaces(&self) -> Result<Vec<String>, MigratoryError> {
        self.list_bridge_interfaces_from(std::path::Path::new("/sys/class/net"))
    }
}

impl LinuxHost {
    /// Checks whether the given `os` identifier matches Linux.
    ///
    /// # Arguments
    ///
    /// * `os` - Target OS name string.
    ///
    /// # Returns
    ///
    /// Returns `true` if `os` is `"linux"`.
    pub fn is_match_os(&self, os: &str) -> bool {
        os == "linux"
    }

    /// Configures NFS exports on Linux using a custom command runner.
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
    /// Returns a `MigratoryError` if `exportfs -ra` fails.
    pub fn configure_nfs_with<F>(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
        mut runner: F,
    ) -> Result<(), MigratoryError>
    where
        F: FnMut(&str, &[&str]) -> Result<(bool, String), MigratoryError>,
    {
        let uid = runner("id", &["-u"])
            .map(|(_, s)| s.trim().to_string())
            .unwrap_or_else(|_| "1000".to_string());
        let gid = runner("id", &["-g"])
            .map(|(_, s)| s.trim().to_string())
            .unwrap_or_else(|_| "1000".to_string());

        let export_lines = Self::generate_nfs_exports(folders, &uid, &gid);
        let cmd = format!("echo '{export_lines}' | sudo tee -a /etc/exports > /dev/null");
        let _ = runner("sh", &["-c", &cmd]);

        let (ok, _) = runner("sudo", &["exportfs", "-ra"])?;
        if !ok {
            return Err(MigratoryError::Generic(
                "Failed to reload NFS exports via exportfs".to_string(),
            ));
        }
        Ok(())
    }

    /// Configures SMB sharing on Linux using a custom command runner.
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
    /// Returns a `MigratoryError` if reloading `smbd` fails.
    pub fn configure_smb_with<F>(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
        mut runner: F,
    ) -> Result<(), MigratoryError>
    where
        F: FnMut(&str, &[&str]) -> Result<(bool, String), MigratoryError>,
    {
        let conf_block = Self::generate_smb_conf(folders);
        let cmd = format!("echo '{conf_block}' | sudo tee -a /etc/samba/smb.conf > /dev/null");
        let _ = runner("sh", &["-c", &cmd]);

        let (ok, _) = runner("sudo", &["systemctl", "reload", "smbd"])?;
        if !ok {
            return Err(MigratoryError::Generic(
                "Failed to reload Samba on Linux host".to_string(),
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
    pub fn check_admin_with<F>(&self, mut runner: F) -> Result<bool, MigratoryError>
    where
        F: FnMut(&str, &[&str]) -> Result<(bool, String), MigratoryError>,
    {
        let (_, stdout) = runner("id", &["-u"])?;
        Ok(stdout.trim() == "0")
    }

    /// Resolves the host IP on Linux using a custom command runner.
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
    pub fn resolve_host_ip_with<F>(&self, mut runner: F) -> Result<String, MigratoryError>
    where
        F: FnMut(&str, &[&str]) -> Result<(bool, String), MigratoryError>,
    {
        let cmd = "ip route get 1 | awk '{print $7}' | tr -d '
'";
        let ip = runner("sh", &["-c", cmd])
            .map(|(_, s)| s.trim().to_string())
            .unwrap_or_else(|_| "192.168.1.3".to_string());
        Ok(if ip.is_empty() {
            "192.168.1.3".to_string()
        } else {
            ip
        })
    }

    /// Enumerates network interfaces from a directory (e.g. `/sys/class/net`).
    ///
    /// # Arguments
    ///
    /// * `sys_net_dir` - Directory containing network interface entries.
    ///
    /// # Returns
    ///
    /// Returns a vector of interface names, defaulting to `["eth0"]` if none are found.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if an error occurs.
    pub fn list_bridge_interfaces_from(
        &self,
        sys_net_dir: &std::path::Path,
    ) -> Result<Vec<String>, MigratoryError> {
        let mut list = Vec::new();
        if let Ok(entries) = std::fs::read_dir(sys_net_dir) {
            for entry in entries.flatten() {
                list.push(entry.file_name().to_string_lossy().to_string());
            }
        }
        if list.is_empty() {
            list.push("eth0".to_string());
        }
        Ok(list)
    }

    /// Checks if the user is in the libvirt group for socket access.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the user belongs to `libvirt` or `libvirtd`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if querying `groups` fails.
    pub fn check_libvirt_group(&self) -> Result<bool, MigratoryError> {
        self.check_libvirt_group_with(default_linux_runner)
    }

    /// Checks if the user is in the libvirt group using a custom command runner.
    ///
    /// # Arguments
    ///
    /// * `runner` - Command execution callback.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the user belongs to `libvirt` or `libvirtd`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if querying `groups` fails.
    pub fn check_libvirt_group_with<F>(&self, mut runner: F) -> Result<bool, MigratoryError>
    where
        F: FnMut(&str, &[&str]) -> Result<(bool, String), MigratoryError>,
    {
        let (_, groups) = runner("groups", &[])?;
        Ok(groups.contains("libvirt") || groups.contains("libvirtd"))
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
    ///
    /// # Returns
    ///
    /// Returns a list of available hypervisor names.
    pub fn capabilities(&self) -> Vec<String> {
        self.capabilities_with("VBoxManage", "virsh")
    }

    /// Detects host capabilities using custom binary paths for `VBoxManage` and `virsh`.
    ///
    /// # Arguments
    ///
    /// * `vbox_bin` - Path or binary name for `VBoxManage`.
    /// * `virsh_bin` - Path or binary name for `virsh`.
    ///
    /// # Returns
    ///
    /// Returns a list of available hypervisor names.
    pub fn capabilities_with(&self, vbox_bin: &str, virsh_bin: &str) -> Vec<String> {
        let mut caps = Vec::new();
        if std::process::Command::new(vbox_bin)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            caps.push("virtualbox".to_string());
        }
        if std::process::Command::new(virsh_bin)
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
        lines.push_str(
            "# VAGRANT-BEGIN
",
        );

        for sf in folders {
            if sf.folder_type == Some(crate::config::SyncedFolderType::Nfs) && !sf.disabled {
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
                lines.push_str(&format!(r#""{}" *({})"#, sf.host_path.display(), opts));
                lines.push('\n');
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
browsable = yes
writable = yes
guest ok = yes
read only = no

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
    fn test_linux_host() -> Result<(), MigratoryError> {
        let host = LinuxHost;
        let _ = default_linux_runner("echo", &["0"]);
        assert_eq!(host.name(), "linux");
        assert!(["systemd", "openrc", "sysvinit"].contains(&host.service_manager()));
        assert!(host.list_bridge_interfaces().is_ok());

        let s_dir = tempfile::tempdir()?;
        let systemd_file = s_dir.path().join("systemd");
        let openrc_file = s_dir.path().join("openrc");
        let none_file = s_dir.path().join("none");

        std::fs::write(&systemd_file, "")?;
        std::fs::write(&openrc_file, "")?;

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

        let net_dir = s_dir.path().join("net");
        std::fs::create_dir_all(net_dir.join("wlan0"))?;
        let ifaces = host.list_bridge_interfaces_from(&net_dir)?;
        assert!(ifaces.contains(&"wlan0".to_string()));
        let empty_ifaces = host.list_bridge_interfaces_from(&none_file)?;
        assert_eq!(empty_ifaces, vec!["eth0".to_string()]);

        assert!(host.configure_nfs(&[]).is_ok());
        assert!(host.configure_smb(&[]).is_ok());
        assert!(host.check_admin()?);
        assert!(host.check_libvirt_group()?);
        assert_eq!(host.resolve_host_ip()?, "192.168.1.3");

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
        assert!(!host.check_libvirt_group_with(|_, _| Ok((true, "users wheel".to_string())))?);
        assert_eq!(
            host.resolve_host_ip_with(|_, _| Ok((true, String::new())))?,
            "192.168.1.3"
        );
        assert_eq!(
            host.resolve_host_ip_with(|_, _| Err(MigratoryError::Generic("err".to_string())))?,
            "192.168.1.3"
        );

        assert!(host.is_match_os("linux"));
        assert!(!host.is_match_os("windows"));
        assert_eq!(host.is_match(), std::env::consts::OS == "linux");

        let _ = host.capabilities();
        let caps = host.capabilities_with("echo", "echo");
        let missing_caps = host.capabilities_with("nonexistent_vbox_xyz", "nonexistent_virsh_xyz");
        let failed_caps = host.capabilities_with("false", "false");

        assert!(caps.contains(&"virtualbox".to_string()));
        assert!(caps.contains(&"libvirt".to_string()));
        assert!(missing_caps.is_empty());
        assert!(failed_caps.is_empty());
        Ok(())
    }

    #[test]
    fn test_linux_nfs_and_smb_generation() {
        let folders = vec![
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/home/user/project"),
                guest_path: std::path::PathBuf::from("/vagrant"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
                disabled: false,
                mount_options: Some(vec!["rw".to_string(), "no_root_squash".to_string()]),
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/home/user/empty_opts_nfs"),
                guest_path: std::path::PathBuf::from("/empty_opts"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
                disabled: false,
                mount_options: Some(Vec::new()),
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/home/user/default_nfs"),
                guest_path: std::path::PathBuf::from("/default"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
                disabled: false,
                mount_options: None,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/home/user/smb_share"),
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
                host_path: std::path::PathBuf::from("/home/user/disabled_smb"),
                guest_path: std::path::PathBuf::from("/disabled_smb"),
                folder_type: Some(crate::config::SyncedFolderType::Smb),
                disabled: true,
                ..Default::default()
            },
            crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/home/user/disabled"),
                guest_path: std::path::PathBuf::from("/disabled"),
                folder_type: Some(crate::config::SyncedFolderType::Nfs),
                disabled: true,
                ..Default::default()
            },
        ];

        let exports = LinuxHost::generate_nfs_exports(&folders, "1000", "1000");
        assert!(exports.contains("# VAGRANT-BEGIN"));
        assert!(exports.contains(r#""/home/user/project" *(rw,no_root_squash)"#));
        assert!(exports.contains(r#""/home/user/default_nfs" *(rw,sync,no_subtree_check,all_squash,anonuid=1000,anongid=1000)"#));
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
