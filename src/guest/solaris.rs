//! Solaris / OmniOS (illumos) Guest OS capabilities implementation.
//!
//! Provides OS-level management for Solaris and illumos-based guests such as OmniOS,
//! including hostname modification via `/etc/nodename`, network configuration with `ipadm`,
//! and system shutdown.

use super::Guest;
use crate::communicator::Communicator;
use crate::config::NetworkConfig;
use crate::error::MigratoryError;
use std::path::Path;

/// Solaris / OmniOS guest implementation.
pub struct SolarisGuest;

impl Guest for SolarisGuest {
    fn detect(&self, comm: &dyn Communicator) -> Result<bool, MigratoryError> {
        match comm.execute("uname -s") {
            Ok(output) => Ok(output.trim() == "SunOS"),
            Err(_) => Ok(false),
        }
    }

    fn change_hostname(
        &self,
        comm: &dyn Communicator,
        hostname: &str,
    ) -> Result<(), MigratoryError> {
        let h = hostname.replace('\x27', "'''");
        let cmd = format!(
            "echo '{h}' | sudo tee /etc/nodename >/dev/null && (sudo hostname '{h}' || true) && (sudo svcadm restart system/identity:node 2>/dev/null || true)"
        );
        comm.execute(&cmd)?;
        Ok(())
    }

    fn configure_networks(
        &self,
        comm: &dyn Communicator,
        networks: &[NetworkConfig],
    ) -> Result<(), MigratoryError> {
        for (i, net) in networks.iter().enumerate() {
            if let NetworkConfig::PrivateNetwork {
                ip: Some(ip_addr), ..
            } = net
            {
                let iface = format!("net{}", i + 1);
                let cmd = format!(
                    "sudo ipadm create-addr -T static -a {ip_addr}/24 {iface}/v4 2>/dev/null || sudo ifconfig {iface} {ip_addr} netmask 255.255.255.0 up 2>/dev/null || true"
                );
                let _ = comm.execute(&cmd);
            }
        }
        Ok(())
    }

    fn mount_shared_folder(
        &self,
        comm: &dyn Communicator,
        name: &str,
        guest_path: &Path,
    ) -> Result<(), MigratoryError> {
        let g = guest_path.to_string_lossy().replace('\x27', "'''");
        let n = name.replace('\x27', "'''");
        let cmd =
            format!("sudo mkdir -p '{g}' && sudo mount -F vboxsf '{n}' '{g}' 2>/dev/null || true");
        comm.execute(&cmd)?;
        Ok(())
    }

    fn halt(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let _ = comm.execute(
            "sudo /usr/sbin/shutdown -y -i5 -g0 2>/dev/null || sudo poweroff 2>/dev/null || true",
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    struct MockComm {
        response: String,
        should_fail: bool,
    }

    impl Communicator for MockComm {
        fn execute(&self, _cmd: &str) -> Result<String, MigratoryError> {
            if self.should_fail {
                Err(MigratoryError::Generic("fail".to_string()))
            } else {
                Ok(self.response.clone())
            }
        }

        fn upload(&self, _local: &Path, _remote: &str) -> Result<(), MigratoryError> {
            Ok(())
        }

        fn download(&self, _remote: &str, _local: &Path) -> Result<(), MigratoryError> {
            Ok(())
        }

        fn execute_interactive(&self) -> Result<(), MigratoryError> {
            Ok(())
        }

        fn wait_for_ready(&self, _timeout: std::time::Duration) -> Result<(), MigratoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_solaris_guest_methods() {
        let guest = SolarisGuest;

        let ok_comm = MockComm {
            response: "SunOS
"
            .to_string(),
            should_fail: false,
        };
        assert!(guest.detect(&ok_comm).expect("detect ok"));

        let linux_comm = MockComm {
            response: "Linux
"
            .to_string(),
            should_fail: false,
        };
        assert!(!guest.detect(&linux_comm).expect("detect ok"));

        let fail_comm = MockComm {
            response: String::new(),
            should_fail: true,
        };
        assert!(!guest.detect(&fail_comm).expect("detect ok"));

        assert!(guest.change_hostname(&ok_comm, "bento-omnios").is_ok());
        assert!(guest.change_hostname(&fail_comm, "bento-omnios").is_err());

        let nets = vec![
            NetworkConfig::PrivateNetwork {
                ip: Some("192.168.56.20".to_string()),
                netmask: None,
                dhcp: false,
                virtualbox_intnet: None,
            },
            NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                protocol: None,
                auto_correct: true,
                host_ip: None,
            },
        ];
        assert!(guest.configure_networks(&ok_comm, &nets).is_ok());

        let nets_empty_ip = vec![NetworkConfig::PrivateNetwork {
            ip: None,
            netmask: None,
            dhcp: false,
            virtualbox_intnet: None,
        }];
        assert!(guest.configure_networks(&ok_comm, &nets_empty_ip).is_ok());

        let _ = ok_comm.upload(Path::new(""), "");
        let _ = ok_comm.download("", Path::new(""));
        let _ = ok_comm.execute_interactive();
        let _ = ok_comm.wait_for_ready(std::time::Duration::from_secs(1));

        assert!(
            guest
                .mount_shared_folder(&ok_comm, "vagrant", Path::new("/vagrant"))
                .is_ok()
        );
        assert!(
            guest
                .mount_shared_folder(&fail_comm, "vagrant", Path::new("/vagrant"))
                .is_err()
        );
        assert!(guest.halt(&ok_comm).is_ok());
        assert!(
            guest
                .update_guest_additions(&ok_comm, "virtualbox", None)
                .is_ok()
        );
    }
}
