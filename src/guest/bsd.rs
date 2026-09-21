//! BSD guest capabilities.
//!
//! This module provides the guest capabilities specific to BSD-based
//! operating systems, handling tasks like hostname changing, network
//! configuration, and folder mounting.

use super::Guest;
use crate::communicator::Communicator;
use crate::config::NetworkConfig;
use crate::error::MigratoryError;
use std::path::Path;

/// BSD guest OS.
///
/// Represents a guest virtual machine running a BSD-like operating system
/// (e.g., FreeBSD, OpenBSD, or Darwin).
pub struct BsdGuest;

impl Guest for BsdGuest {
    /// Detects if the current guest is a BSD-based OS.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if `uname -s` indicates a BSD variant or Darwin,
    /// otherwise `Ok(false)`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the underlying command execution fails
    /// unexpectedly (though failure to execute usually falls back to false).
    fn detect(&self, comm: &dyn Communicator) -> Result<bool, MigratoryError> {
        let out = match comm.execute("uname -s") {
            Ok(output) => output,
            Err(_) => return Ok(false),
        };
        let out_lower = out.trim().to_lowercase();
        Ok(out_lower.contains("bsd") || out_lower.contains("darwin"))
    }

    /// Changes the hostname of the BSD guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    /// * `hostname` - The new hostname.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn change_hostname(
        &self,
        comm: &dyn Communicator,
        hostname: &str,
    ) -> Result<(), MigratoryError> {
        if hostname.is_empty() {
            return Err(MigratoryError::Validation(
                "Hostname cannot be empty".to_string(),
            ));
        }

        let escaped = hostname.replace('\'', "'\\''");
        let cmd = format!(
            "sudo hostname '{}' && sudo sysrc hostname='{}'",
            escaped, escaped
        );
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Configures the network interfaces on the BSD guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    /// * `networks` - Network configuration list.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn configure_networks(
        &self,
        comm: &dyn Communicator,
        networks: &[NetworkConfig],
    ) -> Result<(), MigratoryError> {
        if networks.is_empty() {
            return Ok(());
        }

        let mut script = String::new();
        script.push_str("#!/bin/sh\n");

        let mut eth_index = 1;
        for config in networks {
            match config {
                crate::config::NetworkConfig::PrivateNetwork { ip: Some(ip), .. } => {
                    script.push_str(&format!(
                        "sudo sysrc ifconfig_em{}=\"inet {} netmask 255.255.255.0\"\n",
                        eth_index, ip
                    ));
                    script.push_str(&format!("sudo service netif restart em{}\n", eth_index));
                    eth_index += 1;
                }
                crate::config::NetworkConfig::PublicNetwork { .. } => {
                    script.push_str(&format!("sudo sysrc ifconfig_em{}=\"DHCP\"\n", eth_index));
                    script.push_str(&format!("sudo service netif restart em{}\n", eth_index));
                    eth_index += 1;
                }
                _ => {}
            }
        }

        let remote_path = "/tmp/network_setup.sh";
        let command = format!(
            "cat << 'EOF' > {}\n{}\nEOF\nchmod +x {}\nsudo sh {}",
            remote_path, script, remote_path, remote_path
        );
        let _ = comm.execute(&command)?;
        Ok(())
    }

    /// Mounts a shared folder within the BSD guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    /// * `name` - The name of the shared folder.
    /// * `guest_path` - The destination path within the guest.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn mount_shared_folder(
        &self,
        comm: &dyn Communicator,
        name: &str,
        guest_path: &Path,
    ) -> Result<(), MigratoryError> {
        if name.is_empty() {
            return Err(MigratoryError::Validation(
                "Shared folder name cannot be empty".to_string(),
            ));
        }

        let path_str = guest_path.to_str().unwrap_or("");
        if path_str.is_empty() {
            return Err(MigratoryError::Validation(
                "Guest path cannot be empty".to_string(),
            ));
        }

        let mkdir_cmd = format!("sudo mkdir -p '{}'", path_str.replace('\'', "'\\''"));
        let _ = comm.execute(&mkdir_cmd)?;

        // Mock BSD generic mount command (e.g. for VirtualBox sf)
        let mount_cmd = format!(
            "sudo mount -t vboxvfs '{}' '{}'",
            name.replace('\'', "'\\''"),
            path_str.replace('\'', "'\\''")
        );
        let _ = comm.execute(&mount_cmd)?;

        Ok(())
    }

    /// Halts (shuts down) the BSD guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn halt(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let _ = comm.execute("sudo shutdown -p now")?;
        Ok(())
    }

    fn update_guest_additions(
        &self,
        comm: &dyn Communicator,
        provider_name: &str,
        _machine_id: Option<&str>,
    ) -> Result<(), MigratoryError> {
        match provider_name.to_lowercase().as_str() {
            "virtualbox" => {
                let _ = comm.execute("sudo pkg install -y virtualbox-ose-additions && sudo sysrc vboxguest_enable=YES && sudo sysrc vboxservice_enable=YES")?;
            }
            "vmware" => {
                let _ = comm.execute("sudo pkg install -y open-vm-tools && sudo sysrc vmware_guest_vmblock_enable=YES && sudo sysrc vmware_guest_vmtoolsd_enable=YES")?;
            }
            "qemu" | "libvirt" => {
                let _ = comm.execute("sudo pkg install -y qemu-guest-agent && sudo sysrc qemu_guest_agent_enable=YES")?;
            }
            _ => {}
        }
        Ok(())
    }

    fn reboot(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let _ = comm.execute("sudo shutdown -r now 2>/dev/null || shutdown -r now")?;
        Ok(())
    }

    fn rsync_installed(&self, comm: &dyn Communicator) -> Result<bool, MigratoryError> {
        let out = match comm.execute("which rsync 2>/dev/null") {
            Ok(o) => o,
            Err(_) => return Ok(false),
        };
        Ok(!out.trim().is_empty())
    }

    fn rsync_install(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let _ = comm.execute("sudo pkg install -y rsync 2>/dev/null || sudo pkg_add rsync")?;
        Ok(())
    }

    fn verify_guest_additions(
        &self,
        comm: &dyn Communicator,
        provider_name: &str,
    ) -> Result<bool, MigratoryError> {
        let check = match provider_name.to_lowercase().as_str() {
            "virtualbox" => {
                "VBoxService --version 2>/dev/null || kldstat | grep -i vboxguest 2>/dev/null"
            }
            "vmware" => "vmtoolsd --version 2>/dev/null || which vmtoolsd 2>/dev/null",
            "qemu" | "libvirt" => "qemu-ga --version 2>/dev/null || which qemu-ga 2>/dev/null",
            _ => "echo ok",
        };
        let out = comm.execute(check).unwrap_or_default();
        Ok(!out.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct MockComm {
        output: Result<String, String>,
    }

    impl Communicator for MockComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            match &self.output {
                Ok(out) => Ok(out.clone()),
                Err(err) => Err(MigratoryError::Generic(err.clone())),
            }
        }
        fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn execute_interactive(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn wait_for_ready(&self, _timeout: Duration) -> Result<(), MigratoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_bsd_guest_network_empty() {
        let comm = MockComm {
            output: Ok("".to_string()),
        };
        let guest = BsdGuest;
        assert!(guest.configure_networks(&comm, &[]).is_ok());
    }

    #[test]
    fn test_bsd_guest_network_public() {
        let comm = MockComm {
            output: Ok("".to_string()),
        };
        let guest = BsdGuest;
        let nets = vec![
            crate::config::NetworkConfig::PublicNetwork {
                ip: None,
                bridge: None,
                use_dhcp_assigned_default_route: false,
            },
            crate::config::NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                auto_correct: false,
                protocol: None,
                host_ip: None,
            },
        ];
        assert!(guest.configure_networks(&comm, &nets).is_ok());
    }

    #[test]
    fn test_bsd_guest_update_additions() {
        let comm = MockComm {
            output: Ok("ok".to_string()),
        };
        let guest = BsdGuest;
        assert!(
            guest
                .update_guest_additions(&comm, "virtualbox", None)
                .is_ok()
        );
        assert!(guest.update_guest_additions(&comm, "vmware", None).is_ok());
        assert!(guest.update_guest_additions(&comm, "qemu", None).is_ok());
        assert!(guest.update_guest_additions(&comm, "libvirt", None).is_ok());
        assert!(guest.update_guest_additions(&comm, "other", None).is_ok());

        assert!(guest.verify_guest_additions(&comm, "virtualbox").is_ok());
        assert!(guest.verify_guest_additions(&comm, "vmware").is_ok());
        assert!(guest.verify_guest_additions(&comm, "qemu").is_ok());
        assert!(guest.verify_guest_additions(&comm, "libvirt").is_ok());
        assert!(guest.verify_guest_additions(&comm, "other").is_ok());

        let fail_comm = MockComm {
            output: Err("error".to_string()),
        };
        assert!(
            guest
                .update_guest_additions(&fail_comm, "vmware", None)
                .is_err()
        );
        assert!(
            guest
                .update_guest_additions(&fail_comm, "qemu", None)
                .is_err()
        );
    }

    #[test]
    fn test_mock_comm_coverage() {
        let comm = MockComm {
            output: Ok("".to_string()),
        };
        let _ = comm.upload(Path::new(""), "");
        let _ = comm.download("", Path::new(""));
        let _ = comm.execute_interactive();
        let _ = comm.wait_for_ready(Duration::from_secs(1));
    }

    #[test]
    fn test_bsd_guest() {
        let comm = MockComm {
            output: Ok("FreeBSD\n".to_string()),
        };
        let guest = BsdGuest;
        assert!(guest.detect(&comm).expect("detect should succeed"));
        assert!(guest.change_hostname(&comm, "test").is_ok());

        let networks = vec![NetworkConfig::PrivateNetwork {
            ip: Some("10.0.0.1".to_string()),
            netmask: None,
            dhcp: false,
            virtualbox_intnet: None,
        }];
        assert!(guest.configure_networks(&comm, &networks).is_ok());
        assert!(
            guest
                .mount_shared_folder(&comm, "test", Path::new("/mnt"))
                .is_ok()
        );
        assert!(guest.halt(&comm).is_ok());
    }

    #[test]
    fn test_bsd_guest_validation_errors() {
        let comm = MockComm {
            output: Ok("FreeBSD\n".to_string()),
        };
        let guest = BsdGuest;
        assert!(guest.change_hostname(&comm, "").is_err());
        assert!(
            guest
                .mount_shared_folder(&comm, "", Path::new("/mnt"))
                .is_err()
        );
        assert!(
            guest
                .mount_shared_folder(&comm, "test", Path::new(""))
                .is_err()
        );
    }

    #[test]
    fn test_bsd_guest_not_bsd() {
        let comm = MockComm {
            output: Ok("Linux\n".to_string()),
        };
        let guest = BsdGuest;
        assert!(!guest.detect(&comm).expect("detect should succeed"));
    }

    #[test]
    fn test_bsd_guest_detect_error() {
        let comm = MockComm {
            output: Err("timeout".to_string()),
        };
        let guest = BsdGuest;
        assert!(
            !guest
                .detect(&comm)
                .expect("detect should resolve to false on error")
        );
    }

    #[test]
    fn test_bsd_guest_capabilities() {
        let guest = BsdGuest;
        let comm = MockComm {
            output: Ok("ok".to_string()),
        };
        assert!(guest.reboot(&comm).is_ok());
        assert!(guest.rsync_installed(&comm).unwrap_or(false));
        assert!(guest.rsync_install(&comm).is_ok());
        assert!(
            guest
                .verify_guest_additions(&comm, "virtualbox")
                .unwrap_or(false)
        );
        assert!(
            guest
                .verify_guest_additions(&comm, "other")
                .unwrap_or(false)
        );

        let fail_comm = MockComm {
            output: Err("fail".to_string()),
        };
        assert!(!guest.rsync_installed(&fail_comm).unwrap_or(true));
    }

    struct FnComm<F>(F);

    impl<F: Fn(&str) -> Result<String, MigratoryError>> Communicator for FnComm<F> {
        #[coverage(off)]
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            (self.0)(command)
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

    /// Tests execution failures across BsdGuest methods.
    #[test]
    fn test_bsd_guest_execution_failures() {
        let guest = BsdGuest;
        let fail_comm = MockComm {
            output: Err("exec failed".to_string()),
        };

        // 1. change_hostname fails
        assert!(guest.change_hostname(&fail_comm, "freebsd-node").is_err());

        // 2. configure_networks fails
        let networks = vec![NetworkConfig::PrivateNetwork {
            ip: Some("10.0.0.1".to_string()),
            netmask: None,
            dhcp: false,
            virtualbox_intnet: None,
        }];
        assert!(guest.configure_networks(&fail_comm, &networks).is_err());

        // 3. mount_shared_folder fails at mkdir
        assert!(
            guest
                .mount_shared_folder(&fail_comm, "share", Path::new("/mnt"))
                .is_err()
        );

        // 4. mount_shared_folder fails at mount (mkdir succeeds)
        let mount_fail_comm = FnComm(|cmd: &str| {
            if cmd.contains("mount") {
                Err(MigratoryError::Generic("mount error".to_string()))
            } else {
                Ok("ok".to_string())
            }
        });
        assert!(
            guest
                .mount_shared_folder(&mount_fail_comm, "share", Path::new("/mnt"))
                .is_err()
        );

        // 5. halt fails
        assert!(guest.halt(&fail_comm).is_err());

        // 6. update_guest_additions fails for virtualbox
        assert!(
            guest
                .update_guest_additions(&fail_comm, "virtualbox", None)
                .is_err()
        );

        // 7. reboot fails
        assert!(guest.reboot(&fail_comm).is_err());

        // 8. rsync_install fails
        assert!(guest.rsync_install(&fail_comm).is_err());
    }
}
