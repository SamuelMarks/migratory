//! Host OS capabilities module.
//!
//! Provides traits and implementations for OS-specific host operations,
//! including NFS configuration, SMB sharing, administrative privilege checks,
//! and host network detection.

use crate::error::MigratoryError;

/// BSD host OS capabilities.
pub mod bsd;
/// Darwin (macOS) host OS capabilities.
pub mod darwin;
/// Linux host OS capabilities.
pub mod linux;
/// Windows host OS capabilities.
pub mod windows;

/// Interface for OS-specific host operations.
pub trait Host: Send + Sync {
    /// Name of the host OS (e.g. "darwin", "linux", "windows", "bsd").
    fn name(&self) -> &str {
        "unknown"
    }

    /// True if the current operating system matches this host implementation.
    fn is_match(&self) -> bool {
        false
    }

    /// Configures NFS exports on the host.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError` if host NFS export manipulation fails.
    fn configure_nfs(
        &self,
        _folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Prunes stale Migratory/Vagrant NFS export entries on the host.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if pruning host NFS exports fails.
    fn prune_nfs_exports(&self) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Configures SMB shares on the host.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError` if SMB share configuration fails.
    fn configure_smb(
        &self,
        _folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Checks if the current process has administrative privileges.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError` if privilege checking encounters a system error.
    fn check_admin(&self) -> Result<bool, MigratoryError> {
        Ok(false)
    }

    /// Resolves the correct local IP for bridging.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError` if host IP cannot be determined.
    fn resolve_host_ip(&self) -> Result<String, MigratoryError> {
        Ok("127.0.0.1".to_string())
    }

    /// Returns the system service manager used on this host (e.g. "launchd", "systemd", "openrc", "services.msc", "rc.d").
    fn service_manager(&self) -> &str {
        "unknown"
    }

    /// Lists network interfaces available on the host for bridging.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError` if network interfaces cannot be queried.
    fn list_bridge_interfaces(&self) -> Result<Vec<String>, MigratoryError> {
        Ok(vec!["lo".to_string()])
    }
}

/// Helper to detect the current host OS.
///
/// Iterates through known host implementations and returns the first one that matches.
///
/// # Errors
///
/// Returns `MigratoryError` if the host OS cannot be determined.
pub fn detect_host() -> Result<Box<dyn Host>, MigratoryError> {
    detect_host_for_os(std::env::consts::OS)
}

/// Detects the host OS implementation for an explicit target OS identifier.
///
/// # Arguments
///
/// * `target_os` - OS identifier string (e.g. `"macos"`, `"darwin"`, `"linux"`, `"windows"`, `"freebsd"`).
///
/// # Returns
///
/// Returns a boxed [`Host`] implementation matching `target_os`.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if no host implementation matches `target_os`.
pub fn detect_host_for_os(target_os: &str) -> Result<Box<dyn Host>, MigratoryError> {
    let darwin = darwin::DarwinHost;
    if darwin.is_match_os(target_os) {
        return Ok(Box::new(darwin));
    }
    let linux = linux::LinuxHost;
    if linux.is_match_os(target_os) {
        return Ok(Box::new(linux));
    }
    let windows = windows::WindowsHost;
    if windows.is_match_os(target_os) {
        return Ok(Box::new(windows));
    }
    let bsd = bsd::BsdHost;
    if bsd.is_match_os(target_os) {
        return Ok(Box::new(bsd));
    }
    Err(MigratoryError::Generic(
        "Could not detect host OS".to_string(),
    ))
}

/// Generates sudoers configuration rules for host NFS daemon control.
///
/// This avoids repeated password prompts when starting or modifying NFS exports.
///
/// # Returns
///
/// Returns the formatted sudoers rules string.
pub fn generate_nfs_sudoers_rules() -> String {
    "# Vagrant NFS Sudoers rules
Cmnd_Alias VAGRANT_NFS_COMMANDS = /sbin/nfsd, /usr/sbin/exportfs, /etc/rc.d/mountd, /usr/bin/tee -a /etc/exports
%admin ALL=(ALL) NOPASSWD: VAGRANT_NFS_COMMANDS
%sudo ALL=(ALL) NOPASSWD: VAGRANT_NFS_COMMANDS
%wheel ALL=(ALL) NOPASSWD: VAGRANT_NFS_COMMANDS
".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_host() -> Result<(), MigratoryError> {
        let res_current = detect_host()?;
        assert!(!res_current.name().is_empty());

        let res_darwin = detect_host_for_os("darwin")?;
        assert_eq!(res_darwin.name(), "darwin");
        assert_eq!(detect_host_for_os("macos")?.name(), "darwin");

        let res_linux = detect_host_for_os("linux")?;
        assert_eq!(res_linux.name(), "linux");

        let res_win = detect_host_for_os("windows")?;
        assert_eq!(res_win.name(), "windows");

        let res_bsd = detect_host_for_os("freebsd")?;
        assert_eq!(res_bsd.name(), "bsd");

        assert!(detect_host_for_os("unknown_os_xyz").is_err());
        Ok(())
    }

    /// Minimal host implementation used to test default trait methods on [`Host`].
    struct MinimalHost;
    impl Host for MinimalHost {}

    /// Tests default methods of Host trait.
    #[test]
    fn test_host_trait_defaults() -> Result<(), MigratoryError> {
        let host = MinimalHost;
        assert_eq!(host.name(), "unknown");
        assert!(!host.is_match());
        assert!(host.configure_nfs(&[]).is_ok());
        assert!(host.prune_nfs_exports().is_ok());
        assert!(host.configure_smb(&[]).is_ok());
        assert!(!host.check_admin()?);
        assert_eq!(host.resolve_host_ip()?, "127.0.0.1");
        assert_eq!(host.service_manager(), "unknown");
        let ifaces = host.list_bridge_interfaces()?;
        assert_eq!(ifaces, vec!["lo"]);
        Ok(())
    }

    #[test]
    fn test_generate_nfs_sudoers_rules() {
        let rules = generate_nfs_sudoers_rules();
        assert!(rules.contains("VAGRANT_NFS_COMMANDS"));
        assert!(rules.contains("/sbin/nfsd"));
        assert!(rules.contains("/usr/sbin/exportfs"));
        assert!(rules.contains("/etc/rc.d/mountd"));
    }
}
