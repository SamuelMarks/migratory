//! Host OS capabilities module.
//!
//! Provides traits and implementations for OS-specific host operations,
//! including NFS configuration, SMB sharing, administrative privilege checks,
//! and host network detection.

use crate::error::MigratoryError;

pub mod bsd;
pub mod darwin;
pub mod linux;
pub mod windows;

/// Interface for OS-specific host operations.
pub trait Host: Send + Sync {
    /// Name of the host OS (e.g. "darwin", "linux", "windows", "bsd").
    fn name(&self) -> &str;

    /// True if the current operating system matches this host implementation.
    fn is_match(&self) -> bool;

    /// Configures NFS exports on the host.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError` if host NFS export manipulation fails.
    fn configure_nfs(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError>;

    /// Configures SMB shares on the host.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError` if SMB share configuration fails.
    fn configure_smb(
        &self,
        folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError>;

    /// Checks if the current process has administrative privileges.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError` if privilege checking encounters a system error.
    fn check_admin(&self) -> Result<bool, MigratoryError>;

    /// Resolves the correct local IP for bridging.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError` if host IP cannot be determined.
    fn resolve_host_ip(&self) -> Result<String, MigratoryError>;

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
    let darwin = darwin::DarwinHost;
    if darwin.is_match() {
        return Ok(Box::new(darwin));
    }
    let linux = linux::LinuxHost;
    if linux.is_match() {
        return Ok(Box::new(linux));
    }
    let windows = windows::WindowsHost;
    if windows.is_match() {
        return Ok(Box::new(windows));
    }
    let bsd = bsd::BsdHost;
    if bsd.is_match() {
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
    fn test_detect_host() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        unsafe { std::env::set_var("MOCK_OS", "darwin") };
        let res_darwin = detect_host();
        assert!(res_darwin.is_ok());
        assert_eq!(
            res_darwin.expect("darwin host detection failed").name(),
            "darwin"
        );

        unsafe { std::env::set_var("MOCK_OS", "linux") };
        let res_linux = detect_host();
        assert!(res_linux.is_ok());
        assert_eq!(
            res_linux.expect("linux host detection failed").name(),
            "linux"
        );

        unsafe { std::env::set_var("MOCK_OS", "windows") };
        let res_win = detect_host();
        assert!(res_win.is_ok());
        assert_eq!(
            res_win.expect("win host detection failed").name(),
            "windows"
        );

        unsafe { std::env::set_var("MOCK_OS", "freebsd") };
        let res_bsd = detect_host();
        assert!(res_bsd.is_ok());
        assert_eq!(res_bsd.expect("bsd host detection failed").name(), "bsd");

        unsafe { std::env::set_var("MOCK_OS", "unknown_os_xyz") };
        assert!(detect_host().is_err());

        unsafe { std::env::remove_var("MOCK_OS") };
    }

    struct MinimalHost;
    impl Host for MinimalHost {
        #[coverage(off)]
        fn name(&self) -> &str {
            "minimal"
        }
        #[coverage(off)]
        fn is_match(&self) -> bool {
            true
        }
        #[coverage(off)]
        fn configure_nfs(
            &self,
            _folders: &[crate::config::SyncedFolderConfig],
        ) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn configure_smb(
            &self,
            _folders: &[crate::config::SyncedFolderConfig],
        ) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn check_admin(&self) -> Result<bool, MigratoryError> {
            Ok(false)
        }
        #[coverage(off)]
        fn resolve_host_ip(&self) -> Result<String, MigratoryError> {
            Ok("127.0.0.1".to_string())
        }
    }

    /// Tests default methods of Host trait.
    #[test]
    fn test_host_trait_defaults() {
        let host = MinimalHost;
        assert_eq!(host.service_manager(), "unknown");
        let ifaces = host.list_bridge_interfaces();
        assert!(ifaces.is_ok());
        assert_eq!(ifaces.expect("operation should succeed"), vec!["lo"]);
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
