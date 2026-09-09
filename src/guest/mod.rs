//! Guest OS capabilities module.
//!
//! This module provides the central interface `Guest` for interacting with
//! different types of guest operating systems, along with implementations
//! for BSD, Linux, and Windows.

use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::path::Path;

pub mod bsd;
pub mod linux;
pub mod windows;

/// Core interface for interacting with the VM's OS.
///
/// Implementors of this trait define how to execute OS-specific tasks
/// like changing hostnames, configuring networks, and mounting folders.
pub trait Guest {
    /// Detects if this guest OS matches the running VM.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the OS is a match, or `Ok(false)` otherwise.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if detection encounters a fatal failure.
    fn detect(&self, comm: &dyn Communicator) -> Result<bool, MigratoryError>;

    /// Changes the hostname of the guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `hostname` - The desired hostname.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the operation fails.
    fn change_hostname(
        &self,
        comm: &dyn Communicator,
        hostname: &str,
    ) -> Result<(), MigratoryError>;

    /// Configures network interfaces on the guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `networks` - The list of network configurations.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if network configuration fails.
    fn configure_networks(
        &self,
        comm: &dyn Communicator,
        networks: &[crate::config::NetworkConfig],
    ) -> Result<(), MigratoryError>;

    /// Mounts a shared folder on the guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `name` - The logical name of the shared folder.
    /// * `guest_path` - The absolute path on the guest to mount the folder.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the mount command fails.
    fn mount_shared_folder(
        &self,
        comm: &dyn Communicator,
        name: &str,
        guest_path: &Path,
    ) -> Result<(), MigratoryError>;

    /// Gracefully halts the guest OS.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the shutdown command fails.
    fn halt(&self, comm: &dyn Communicator) -> Result<(), MigratoryError>;

    /// Detects and optionally updates guest additions (VirtualBox/VMware).
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `provider_name` - The hypervisor name (e.g. "virtualbox", "vmware").
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn update_guest_additions(
        &self,
        comm: &dyn Communicator,
        provider_name: &str,
        machine_id: Option<&str>,
    ) -> Result<(), MigratoryError>;

    /// Reboots the guest operating system.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn reboot(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let _ = comm.execute(
            "sudo reboot 2>/dev/null || reboot 2>/dev/null || shutdown /r /t 0 /f 2>/dev/null",
        );
        Ok(())
    }

    /// Inserts a public SSH key into the guest authorized_keys file.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `public_key` - Public key string to insert.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn insert_public_key(
        &self,
        comm: &dyn Communicator,
        public_key: &str,
    ) -> Result<(), MigratoryError> {
        let key = public_key.trim();
        let cmd = format!(
            "mkdir -p ~/.ssh && chmod 0700 ~/.ssh && echo '{key}' >> ~/.ssh/authorized_keys && chmod 0600 ~/.ssh/authorized_keys"
        );
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Removes a public SSH key matching the given pattern from authorized_keys.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `public_key_pattern` - Search pattern or substring of the key to remove.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn remove_public_key(
        &self,
        comm: &dyn Communicator,
        public_key_pattern: &str,
    ) -> Result<(), MigratoryError> {
        let pattern = public_key_pattern.replace('/', "\\/");
        let cmd = format!("sed -i '/{pattern}/d' ~/.ssh/authorized_keys 2>/dev/null || true");
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Mounts a VirtualBox shared folder on the guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `name` - The logical name of the shared folder.
    /// * `guest_path` - The mount path in the guest.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn mount_virtualbox_shared_folder(
        &self,
        comm: &dyn Communicator,
        name: &str,
        guest_path: &Path,
    ) -> Result<(), MigratoryError> {
        self.mount_shared_folder(comm, name, guest_path)
    }

    /// Mounts an NFS exported folder on the guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `host_ip` - IP address of the NFS server.
    /// * `host_path` - Export path on the host.
    /// * `guest_path` - Mount path in the guest.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn mount_nfs_folder(
        &self,
        comm: &dyn Communicator,
        host_ip: &str,
        host_path: &str,
        guest_path: &Path,
    ) -> Result<(), MigratoryError> {
        let g = guest_path.to_string_lossy().replace('\'', "'\\''");
        let h = host_path.replace('\'', "'\\''");
        let cmd = format!(
            "sudo mkdir -p '{g}' && sudo mount -t nfs -o vers=3,udp,nolock,rw '{host_ip}:{h}' '{g}'"
        );
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Mounts an SMB / CIFS share on the guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `host_path` - UNC or host path of the share.
    /// * `guest_path` - Mount path in the guest.
    /// * `username` - Credentials username.
    /// * `password` - Credentials password.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn mount_smb_shared_folder(
        &self,
        comm: &dyn Communicator,
        host_path: &str,
        guest_path: &Path,
        username: &str,
        password: &str,
    ) -> Result<(), MigratoryError> {
        let guest_str = guest_path.to_string_lossy();
        let cmd = format!(
            "sudo mkdir -p '{0}' && sudo mount -t cifs -o username='{1}',password='{2}',uid=1000,gid=1000 '{3}' '{0}'",
            guest_str.replace('\'', "'\\''"),
            username.replace('\'', "'\\''"),
            password.replace('\'', "'\\''"),
            host_path.replace('\'', "'\\''")
        );
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Mounts/prepares an Rsync synced folder on the guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `guest_path` - Destination path on the guest.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn mount_rsync_folder(
        &self,
        comm: &dyn Communicator,
        guest_path: &Path,
    ) -> Result<(), MigratoryError> {
        let guest_str = guest_path.to_string_lossy();
        let cmd = format!(
            "sudo mkdir -p '{0}' && sudo chown -R `id -u`:`id -g` '{0}'",
            guest_str.replace('\'', "'\\''")
        );
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Checks if the rsync binary is installed on the guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if rsync is available, or `Ok(false)`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on fatal communicator failure.
    fn rsync_installed(&self, comm: &dyn Communicator) -> Result<bool, MigratoryError> {
        let out = match comm.execute("which rsync 2>/dev/null || where rsync 2>/dev/null") {
            Ok(output) => output,
            Err(_) => return Ok(false),
        };
        Ok(!out.trim().is_empty())
    }

    /// Installs the rsync package on the guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn rsync_install(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let cmd = "sudo apt-get update -y && sudo apt-get install -y rsync || sudo dnf install -y rsync || sudo yum install -y rsync || sudo pacman -Sy --noconfirm rsync || sudo apk add --no-cache rsync || sudo zypper install -y rsync || sudo pkg install -y rsync";
        let _ = comm.execute(cmd)?;
        Ok(())
    }

    /// Verifies if guest additions or tools are active and matches hypervisor.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `provider_name` - The provider name.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if active, or `Ok(false)`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn verify_guest_additions(
        &self,
        comm: &dyn Communicator,
        provider_name: &str,
    ) -> Result<bool, MigratoryError> {
        let check_cmd = match provider_name.to_lowercase().as_str() {
            "virtualbox" => {
                "VBoxService --version 2>/dev/null || lsmod | grep vboxguest 2>/dev/null || sc query VBoxGuest 2>/dev/null"
            }
            "vmware" | "vmware_desktop" => {
                "vmtoolsd -v 2>/dev/null || sc query VMTools 2>/dev/null"
            }
            _ => "echo ok",
        };
        let out = comm.execute(check_cmd).unwrap_or_default();
        Ok(!out.trim().is_empty())
    }
}

/// Helper to detect the guest OS type.
///
/// Iterates through known guest implementations and returns the first one that matches.
///
/// # Arguments
///
/// * `comm` - The communicator used to interact with the VM.
///
/// # Returns
///
/// Returns a boxed `Guest` trait object representing the detected OS.
///
/// # Errors
///
/// Returns a `MigratoryError` if no known OS can be detected, or if an
/// underlying communication error occurs during detection.
pub fn detect_guest(comm: &dyn Communicator) -> Result<Box<dyn Guest>, MigratoryError> {
    let linux = linux::LinuxGuest;
    if matches!(linux.detect(comm), Ok(true)) {
        return Ok(Box::new(linux));
    }
    let windows = windows::WindowsGuest;
    if matches!(windows.detect(comm), Ok(true)) {
        return Ok(Box::new(windows));
    }
    let bsd = bsd::BsdGuest;
    if matches!(bsd.detect(comm), Ok(true)) {
        return Ok(Box::new(bsd));
    }

    Err(MigratoryError::Generic(
        "Could not detect guest OS".to_string(),
    ))
}

/// Resolves a guest OS implementation, respecting an optional explicit override.
///
/// If `override_name` is Some, it attempts to match common guest names:
/// - "linux", "debian", "ubuntu", "centos", "redhat", "fedora", "arch", "alpine", "suse" -> Linux
/// - "windows" -> Windows
/// - "bsd", "freebsd", "openbsd", "netbsd" -> BSD
///
/// If `override_name` is None or unrecognized, it falls back to auto-detection via `detect_guest(comm)`.
///
/// # Arguments
///
/// * `comm` - Communicator for detection fallback.
/// * `override_name` - Optional explicit guest override string from configuration.
///
/// # Returns
///
/// Returns a boxed `Guest` trait object.
///
/// # Errors
///
/// Returns a `MigratoryError` if detection fails.
pub fn resolve_guest(
    comm: &dyn Communicator,
    override_name: Option<&str>,
) -> Result<Box<dyn Guest>, MigratoryError> {
    if let Some(name) = override_name {
        let lower = name.trim().to_ascii_lowercase();
        match lower.as_str() {
            "linux" | "debian" | "ubuntu" | "centos" | "redhat" | "fedora" | "arch" | "alpine"
            | "suse" => return Ok(Box::new(linux::LinuxGuest)),
            "windows" => return Ok(Box::new(windows::WindowsGuest)),
            "bsd" | "freebsd" | "openbsd" | "netbsd" => return Ok(Box::new(bsd::BsdGuest)),
            _ => {}
        }
    }
    detect_guest(comm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::communicator::Communicator;
    use std::time::Duration;

    struct MockComm {
        os: String,
        error_on_detect: bool,
    }

    impl Communicator for MockComm {
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            if self.error_on_detect {
                return Err(MigratoryError::Generic("Force failed".to_string()));
            }

            if command == "uname -s" && self.os == "linux" {
                Ok("Linux\n".to_string())
            } else if command == "uname -s" && self.os == "bsd" {
                Ok("FreeBSD\n".to_string())
            } else if command == "cmd.exe /c ver" && self.os == "windows" {
                Ok("Windows\n".to_string())
            } else {
                Err(MigratoryError::Generic("Command failed".to_string()))
            }
        }
        #[coverage(off)]
        fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn execute_interactive(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn wait_for_ready(&self, _timeout: Duration) -> Result<(), MigratoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_mock_comm_coverage() {
        let comm = MockComm {
            os: "".to_string(),
            error_on_detect: false,
        };
        let _ = comm.upload(std::path::Path::new(""), "");
        let _ = comm.download("", std::path::Path::new(""));
        let _ = comm.execute_interactive();
        let _ = comm.wait_for_ready(std::time::Duration::from_secs(1));
    }

    #[test]
    fn test_detect_linux() {
        let comm = MockComm {
            os: "linux".to_string(),
            error_on_detect: false,
        };
        let guest = detect_guest(&comm);
        assert!(guest.is_ok());
    }

    #[test]
    fn test_detect_windows() {
        let comm = MockComm {
            os: "windows".to_string(),
            error_on_detect: false,
        };
        let guest = detect_guest(&comm);
        assert!(guest.is_ok());
    }

    #[test]
    fn test_detect_bsd() {
        let comm = MockComm {
            os: "bsd".to_string(),
            error_on_detect: false,
        };
        let guest = detect_guest(&comm);
        assert!(guest.is_ok());
    }

    #[test]
    fn test_detect_unknown() {
        let comm = MockComm {
            os: "unknown".to_string(),
            error_on_detect: false,
        };
        let guest = detect_guest(&comm);
        assert!(guest.is_err());
    }

    #[test]
    fn test_detect_error_propagation() {
        // `linux::LinuxGuest::detect` absorbs command errors and returns `Ok(false)`.
        // Let's verify `detect_guest` doesn't fail out early on `linux` if it absorbs it,
        // and instead eventually hits `detect_unknown` block.
        let comm = MockComm {
            os: "unknown".to_string(),
            error_on_detect: true,
        };
        let guest = detect_guest(&comm);
        assert!(guest.is_err());
    }

    #[test]
    fn test_resolve_guest_overrides() {
        let comm = MockComm {
            os: "unknown".to_string(),
            error_on_detect: false,
        };
        assert!(resolve_guest(&comm, Some("linux")).is_ok());
        assert!(resolve_guest(&comm, Some("ubuntu")).is_ok());
        assert!(resolve_guest(&comm, Some("windows")).is_ok());
        assert!(resolve_guest(&comm, Some("freebsd")).is_ok());
        assert!(resolve_guest(&comm, Some("bsd")).is_ok());
        assert!(resolve_guest(&comm, Some("unrecognized")).is_err());

        let comm_linux = MockComm {
            os: "linux".to_string(),
            error_on_detect: false,
        };
        assert!(resolve_guest(&comm_linux, None).is_ok());
    }

    struct DummyGuest;
    #[coverage(off)]
    impl Guest for DummyGuest {
        fn detect(&self, _comm: &dyn Communicator) -> Result<bool, MigratoryError> {
            Ok(true)
        }
        fn change_hostname(
            &self,
            _comm: &dyn Communicator,
            _hostname: &str,
        ) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn configure_networks(
            &self,
            _comm: &dyn Communicator,
            _networks: &[crate::config::NetworkConfig],
        ) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn mount_shared_folder(
            &self,
            _comm: &dyn Communicator,
            _name: &str,
            _guest_path: &Path,
        ) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn halt(&self, _comm: &dyn Communicator) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn update_guest_additions(
            &self,
            _comm: &dyn Communicator,
            _provider_name: &str,
            _machine_id: Option<&str>,
        ) -> Result<(), MigratoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_guest_default_capabilities() {
        struct AlwaysOkComm;
        impl Communicator for AlwaysOkComm {
            fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
                Ok("ok\n".to_string())
            }
            #[coverage(off)]
            fn upload(&self, _l: &Path, _r: &str) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn download(&self, _r: &str, _l: &Path) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn execute_interactive(&self) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn wait_for_ready(&self, _t: Duration) -> Result<(), MigratoryError> {
                Ok(())
            }
        }

        struct FailingComm;
        impl Communicator for FailingComm {
            fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
                Err(MigratoryError::Generic("err".to_string()))
            }
            #[coverage(off)]
            fn upload(&self, _l: &Path, _r: &str) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn download(&self, _r: &str, _l: &Path) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn execute_interactive(&self) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn wait_for_ready(&self, _t: Duration) -> Result<(), MigratoryError> {
                Ok(())
            }
        }

        let comm = AlwaysOkComm;
        let guest = DummyGuest;
        assert!(guest.reboot(&comm).is_ok());
        assert!(guest.insert_public_key(&comm, "ssh-key").is_ok());
        assert!(guest.remove_public_key(&comm, "ssh-key").is_ok());
        assert!(
            guest
                .mount_virtualbox_shared_folder(&comm, "sh", Path::new("/v"))
                .is_ok()
        );
        assert!(
            guest
                .mount_nfs_folder(&comm, "127.0.0.1", "/h", Path::new("/g"))
                .is_ok()
        );
        assert!(
            guest
                .mount_smb_shared_folder(&comm, "//h/s", Path::new("/g"), "u", "p")
                .is_ok()
        );
        assert!(guest.mount_rsync_folder(&comm, Path::new("/g")).is_ok());
        assert!(guest.rsync_installed(&comm).unwrap_or(false));
        assert!(guest.rsync_install(&comm).is_ok());
        assert!(
            guest
                .verify_guest_additions(&comm, "virtualbox")
                .unwrap_or(false)
        );
        assert!(
            guest
                .verify_guest_additions(&comm, "vmware")
                .unwrap_or(false)
        );
        assert!(
            guest
                .verify_guest_additions(&comm, "unknown")
                .unwrap_or(false)
        );

        let fail_comm = FailingComm;
        assert!(guest.insert_public_key(&fail_comm, "ssh-key").is_err());
        assert!(guest.remove_public_key(&fail_comm, "ssh-key").is_err());
        assert!(
            guest
                .mount_nfs_folder(&fail_comm, "127.0.0.1", "/h", Path::new("/g"))
                .is_err()
        );
        assert!(
            guest
                .mount_smb_shared_folder(&fail_comm, "//h/s", Path::new("/g"), "u", "p")
                .is_err()
        );
        assert!(
            guest
                .mount_rsync_folder(&fail_comm, Path::new("/g"))
                .is_err()
        );
        assert!(guest.rsync_install(&fail_comm).is_err());
        assert!(!guest.rsync_installed(&fail_comm).unwrap_or(true));
    }
}
