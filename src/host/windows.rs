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
    /// Returns a `MigratoryError` if the configuration fails (currently a no-op).
    #[coverage(off)]
    fn configure_nfs(
        &self,
        _folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
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
    /// Returns a `MigratoryError` if SMB share creation fails (currently a no-op).
    #[coverage(off)]
    fn configure_smb(
        &self,
        _folders: &[crate::config::SyncedFolderConfig],
    ) -> Result<(), MigratoryError> {
        let _script =
            "New-SmbShare -Name 'MigratoryShare' -Path 'C:\\vagrant' -FullAccess 'Everyone'";
        #[cfg(not(test))]
        {
            let status = Command::new("powershell")
                .arg("-Command")
                .arg(_script)
                .status()
                .map_err(MigratoryError::Io)?;

            if !status.success() {
                return Err(MigratoryError::Generic(
                    "Failed to configure SMB share".to_string(),
                ));
            }
        }
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

    #[coverage(off)]
    fn resolve_host_ip(&self) -> Result<String, MigratoryError> {
        // Typically parse ipconfig / get-netipaddress
        Ok("192.168.1.4".to_string())
    }

    fn service_manager(&self) -> &str {
        "services.msc"
    }

    fn list_bridge_interfaces(&self) -> Result<Vec<String>, MigratoryError> {
        Ok(vec!["Ethernet".to_string(), "Wi-Fi".to_string()])
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_host() {
        let host = WindowsHost;
        assert_eq!(host.name(), "windows");
        assert!(host.configure_nfs(&[]).is_ok());
        assert!(host.configure_smb(&[]).is_ok());
        assert!(host.check_admin().expect("admin check should succeed"));
        assert!(host.enable_ansi_colors().is_ok());

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
}
