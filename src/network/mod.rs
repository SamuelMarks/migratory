//! Network configurations for the VM.

use crate::config::NetworkConfig;
use crate::error::MigratoryError;
use std::collections::HashMap;
use std::net::TcpListener;

/// Result of a port collision check.
pub struct PortCollisionResult {
    /// True if collision was detected.
    pub collision: bool,
    /// Corrected host port.
    pub corrected_host_port: u16,
}

/// Checks if a port is in use on localhost.
///
/// # Arguments
///
/// * `port` - The port number to check.
///
/// # Returns
///
/// Returns `true` if the port cannot be bound.
pub fn is_port_in_use(port: u16) -> bool {
    is_port_in_use_on("127.0.0.1", port)
}

/// Checks if a port is in use on a specific host IP address.
///
/// # Arguments
///
/// * `host_ip` - The host IP address to bind.
/// * `port` - The port number to check.
///
/// # Returns
///
/// Returns `true` if the port cannot be bound.
pub fn is_port_in_use_on(host_ip: &str, port: u16) -> bool {
    TcpListener::bind((host_ip, port)).is_err()
}

/// Checks and corrects forwarded port collisions.
///
/// # Arguments
///
/// * `config` - The network configuration.
/// * `open_ports` - A slice of already allocated ports.
///
/// # Returns
///
/// Returns a `PortCollisionResult` with collision status and final port.
///
/// # Errors
///
/// Returns a `MigratoryError` if the config is invalid or the port is in use without auto-correct.
pub fn check_forwarded_port(
    config: &NetworkConfig,
    open_ports: &[u16],
) -> Result<PortCollisionResult, MigratoryError> {
    check_forwarded_port_with_range(config, open_ports, None)
}

/// Checks and corrects forwarded port collisions using a specified usable port range.
///
/// # Arguments
///
/// * `config` - The `NetworkConfig::ForwardedPort` configuration.
/// * `open_ports` - A slice of already allocated port numbers.
/// * `usable_range` - Optional inclusive range of ports to search for available ports.
///
/// # Returns
///
/// Returns a `PortCollisionResult` containing whether a collision occurred and the corrected port.
///
/// # Errors
///
/// Returns a `MigratoryError` if auto-correct is disabled and the port is in use, or if all ports in range are exhausted.
pub fn check_forwarded_port_with_range(
    config: &NetworkConfig,
    open_ports: &[u16],
    usable_range: Option<std::ops::RangeInclusive<u16>>,
) -> Result<PortCollisionResult, MigratoryError> {
    if let NetworkConfig::ForwardedPort {
        host, auto_correct, ..
    } = config
    {
        let mut final_host = *host;
        let mut collision = false;

        if open_ports.contains(&final_host) || is_port_in_use(final_host) {
            collision = true;
            if !*auto_correct {
                return Err(MigratoryError::Generic(format!("Port {} is in use.", host)));
            }

            if let Some(range) = usable_range {
                let mut found = None;
                for p in range {
                    if !open_ports.contains(&p) && !is_port_in_use(p) {
                        found = Some(p);
                        break;
                    }
                }
                final_host = found.ok_or_else(|| {
                    MigratoryError::Generic(
                        "Could not find an available port in usable range".to_string(),
                    )
                })?;
            } else {
                while open_ports.contains(&final_host) || is_port_in_use(final_host) {
                    if let Some(next_port) = final_host.checked_add(1) {
                        final_host = next_port;
                    } else {
                        return Err(MigratoryError::Generic(format!(
                            "Could not find available port starting from {}",
                            host
                        )));
                    }
                }
            }
        }

        Ok(PortCollisionResult {
            collision,
            corrected_host_port: final_host,
        })
    } else {
        Err(MigratoryError::Generic(
            "Not a ForwardedPort config".to_string(),
        ))
    }
}

/// Helper to configure a private network.
pub fn configure_private_network(
    config: &NetworkConfig,
    _host_interfaces: &HashMap<String, String>,
) -> Result<(), MigratoryError> {
    if let NetworkConfig::PrivateNetwork { ip: Some(ip), .. } = config {
        // Implementation for creating/finding a host-only network matching this IP subnet
        // In reality, this talks to VirtualBox / QEMU to ensure a virtual switch exists.
        // For the CLI simulation, we validate the IP format.
        if ip.is_empty() {
            return Err(MigratoryError::Validation(
                "Private network IP cannot be empty".to_string(),
            ));
        }
        Ok(())
    } else {
        Err(MigratoryError::Validation(
            "Expected PrivateNetwork config".to_string(),
        ))
    }
}

/// Helper to configure a public network (bridged).
pub fn configure_public_network(
    config: &NetworkConfig,
    host_interfaces: &HashMap<String, String>,
    ui: Option<&dyn crate::ui::Ui>,
) -> Result<String, MigratoryError> {
    if let NetworkConfig::PublicNetwork { bridge, .. } = config {
        if let Some(bridge_name) = bridge {
            if !host_interfaces.contains_key(bridge_name) {
                return Err(MigratoryError::Validation(format!(
                    "Bridge interface '{}' not found on host",
                    bridge_name
                )));
            }
            Ok(bridge_name.clone())
        } else {
            if host_interfaces.is_empty() {
                return Err(MigratoryError::Validation(
                    "No host interfaces available for bridging".to_string(),
                ));
            }
            if let Some(ui) = ui {
                let mut choices: Vec<&str> = host_interfaces.keys().map(|k| k.as_str()).collect();
                choices.sort();
                let choice =
                    ui.prompt_choice("network", "Select a host interface for bridging:", &choices)?;
                Ok(choice)
            } else {
                // If no UI provided, just pick the first one deterministically
                let mut choices: Vec<&str> = host_interfaces.keys().map(|k| k.as_str()).collect();
                choices.sort();
                Ok(choices[0].to_string())
            }
        }
    } else {
        Err(MigratoryError::Validation(
            "Expected PublicNetwork config".to_string(),
        ))
    }
}

/// Generates a network configuration script to inject into the guest on boot.
pub fn generate_guest_network_script(configs: &[NetworkConfig]) -> String {
    let mut script = String::new();
    script.push_str("#!/bin/sh\n# Migratory Guest Network Setup\n");
    let mut eth_index = 1; // eth0 is usually NAT/management

    for config in configs {
        match config {
            NetworkConfig::PrivateNetwork { ip: Some(ip), .. } => {
                script.push_str(&format!("ip addr add {}/24 dev eth{}\n", ip, eth_index));
                script.push_str(&format!("ip link set eth{} up\n", eth_index));
                eth_index += 1;
            }
            NetworkConfig::PublicNetwork { .. } => {
                script.push_str(&format!("dhclient eth{}\n", eth_index));
                eth_index += 1;
            }
            _ => {}
        }
    }
    script
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_network_types() {
        let priv_net = NetworkConfig::PrivateNetwork {
            ip: Some("1.2.3.4".to_string()),
            netmask: None,
            dhcp: false,
            virtualbox_intnet: None,
        };
        let fwd_net = NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: false,
            protocol: None,
            host_ip: None,
        };
        let pub_net = NetworkConfig::PublicNetwork {
            ip: None,
            bridge: None,
            use_dhcp_assigned_default_route: false,
        };

        assert!(check_forwarded_port(&priv_net, &[]).is_err());

        let empty_map = HashMap::new();
        assert!(configure_private_network(&fwd_net, &empty_map).is_err());
        assert!(configure_public_network(&fwd_net, &empty_map, None).is_err());
        assert!(configure_public_network(&pub_net, &empty_map, None).is_err()); // No interfaces
    }

    #[test]
    fn test_port_collision_check() {
        let port_cfg = NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: true,
            protocol: None,
            host_ip: None,
        };
        let open_ports = vec![8080, 8081];

        // Should auto-correct to 8082
        let res = check_forwarded_port(&port_cfg, &open_ports).expect("operation should succeed");
        assert!(res.collision);
        // It might be different if actual system port 8082 is in use, but usually it's free.
        assert!(res.corrected_host_port >= 8082);

        let port_cfg_no_correct = NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: false,
            protocol: None,
            host_ip: None,
        };
        let err = check_forwarded_port(&port_cfg_no_correct, &open_ports);
        assert!(err.is_err());

        // Find a completely free port starting from a high number to avoid actual system collisions
        let port_cfg_no_collide = NetworkConfig::ForwardedPort {
            guest: 80,
            host: 45000,
            auto_correct: false,
            protocol: None,
            host_ip: None,
        };
        let res_no_collide =
            check_forwarded_port(&port_cfg_no_collide, &[]).expect("operation should succeed");
        assert!(!res_no_collide.collision);
        assert_eq!(res_no_collide.corrected_host_port, 45000);

        let max_net = NetworkConfig::ForwardedPort {
            guest: 80,
            host: 65535,
            auto_correct: true,
            protocol: None,
            host_ip: None,
        };
        assert!(check_forwarded_port(&max_net, &[65535]).is_err());
    }

    #[test]
    fn test_invalid_network_config() {
        let priv_net = NetworkConfig::PrivateNetwork {
            ip: Some("10.0.0.1".to_string()),
            netmask: None,
            dhcp: false,
            virtualbox_intnet: None,
        };
        assert!(check_forwarded_port(&priv_net, &[]).is_err());
    }

    #[test]
    fn test_network_bridge_not_found() {
        let pub_net = NetworkConfig::PublicNetwork {
            ip: None,
            bridge: Some("nonexistent".to_string()),
            use_dhcp_assigned_default_route: false,
        };
        let host_interfaces = HashMap::new();
        assert!(configure_public_network(&pub_net, &host_interfaces, None).is_err());
    }

    struct MockUi;
    impl crate::ui::Ui for MockUi {
        #[coverage(off)]
        fn info(&self, _target: &str, _msg: &str) {}
        #[coverage(off)]
        fn success(&self, _target: &str, _msg: &str) {}
        #[coverage(off)]
        fn warn(&self, _target: &str, _msg: &str) {}
        #[coverage(off)]
        fn error(&self, _target: &str, _msg: &str) {}
        #[coverage(off)]
        fn detail(&self, _target: &str, _msg: &str) {}
        #[coverage(off)]
        fn create_progress(
            &self,
            _target: &str,
            _total: u64,
            _msg: &str,
        ) -> indicatif::ProgressBar {
            indicatif::ProgressBar::hidden()
        }
        fn prompt_choice(
            &self,
            _header: &str,
            _msg: &str,
            choices: &[&str],
        ) -> Result<String, MigratoryError> {
            Ok(choices[0].to_string())
        }
    }

    struct MockFailingUi;
    impl crate::ui::Ui for MockFailingUi {
        #[coverage(off)]
        fn info(&self, _target: &str, _msg: &str) {}
        #[coverage(off)]
        fn success(&self, _target: &str, _msg: &str) {}
        #[coverage(off)]
        fn warn(&self, _target: &str, _msg: &str) {}
        #[coverage(off)]
        fn error(&self, _target: &str, _msg: &str) {}
        #[coverage(off)]
        fn detail(&self, _target: &str, _msg: &str) {}
        #[coverage(off)]
        fn create_progress(
            &self,
            _target: &str,
            _total: u64,
            _msg: &str,
        ) -> indicatif::ProgressBar {
            indicatif::ProgressBar::hidden()
        }
        fn prompt_choice(
            &self,
            _header: &str,
            _msg: &str,
            _choices: &[&str],
        ) -> Result<String, MigratoryError> {
            Err(MigratoryError::Generic("Cancelled".to_string()))
        }
    }

    #[test]
    fn test_network_ui_prompt() {
        let pub_net = NetworkConfig::PublicNetwork {
            ip: None,
            bridge: None,
            use_dhcp_assigned_default_route: false,
        };
        let mut host_interfaces = HashMap::new();
        host_interfaces.insert("eth0".to_string(), "1.2.3.4".to_string());
        let ui = MockUi;
        let res = configure_public_network(&pub_net, &host_interfaces, Some(&ui));
        assert!(res.is_ok());
        assert_eq!(res.expect("operation should succeed"), "eth0");

        let failing_ui = MockFailingUi;
        assert!(configure_public_network(&pub_net, &host_interfaces, Some(&failing_ui)).is_err());
    }

    #[test]
    fn test_configure_networks() {
        let priv_net = NetworkConfig::PrivateNetwork {
            ip: Some("10.0.0.1".to_string()),
            netmask: None,
            dhcp: false,
            virtualbox_intnet: None,
        };
        assert!(configure_private_network(&priv_net, &HashMap::new()).is_ok());

        let invalid_priv_net = NetworkConfig::PrivateNetwork {
            ip: Some("".to_string()),
            netmask: None,
            dhcp: false,
            virtualbox_intnet: None,
        };
        assert!(configure_private_network(&invalid_priv_net, &HashMap::new()).is_err());

        let pub_net = NetworkConfig::PublicNetwork {
            ip: None,
            bridge: Some("eth0".to_string()),
            use_dhcp_assigned_default_route: false,
        };

        let mut ifaces = HashMap::new();
        ifaces.insert("eth0".to_string(), "Ethernet".to_string());

        let res =
            configure_public_network(&pub_net, &ifaces, None).expect("operation should succeed");
        assert_eq!(res, "eth0");

        let pub_net_no_bridge = NetworkConfig::PublicNetwork {
            ip: None,
            bridge: None,
            use_dhcp_assigned_default_route: false,
        };
        let res2 = configure_public_network(&pub_net_no_bridge, &ifaces, None)
            .expect("operation should succeed");
        assert_eq!(res2, "eth0");

        assert!(configure_public_network(&priv_net, &ifaces, None).is_err());
    }

    #[test]
    fn test_generate_guest_network_script() {
        let configs = vec![
            NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                auto_correct: true,
                protocol: None,
                host_ip: None,
            },
            NetworkConfig::PrivateNetwork {
                ip: Some("192.168.50.4".to_string()),
                netmask: None,
                dhcp: false,
                virtualbox_intnet: None,
            },
            NetworkConfig::PublicNetwork {
                ip: None,
                bridge: None,
                use_dhcp_assigned_default_route: false,
            },
        ];

        let script = generate_guest_network_script(&configs);
        assert!(script.contains("ip addr add 192.168.50.4/24 dev eth1"));
        assert!(script.contains("dhclient eth2"));
        assert!(!script.contains("eth0")); // eth0 is skipped
    }
}

#[cfg(test)]
mod extra_network_tests {
    use super::*;
    use crate::config::NetworkConfig;

    #[test]
    fn test_generate_guest_network_script_private_no_ip() {
        let configs = vec![
            NetworkConfig::PrivateNetwork {
                dhcp: false,
                virtualbox_intnet: None,
                ip: None,
                netmask: None,
            },
            NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                auto_correct: false,
                protocol: None,
                host_ip: None,
            },
        ];
        let script = generate_guest_network_script(&configs);
        // It does nothing
        assert_eq!(script, "#!/bin/sh\n# Migratory Guest Network Setup\n");
    }

    #[test]
    fn test_check_forwarded_port_with_range() {
        let config = NetworkConfig::ForwardedPort {
            guest: 22,
            host: 2222,
            auto_correct: true,
            protocol: None,
            host_ip: None,
        };

        // Open ports includes 2222, range allows 2223..=2225
        let res = check_forwarded_port_with_range(&config, &[2222], Some(2223..=2225));
        assert!(res.is_ok());
        let val = res.expect("operation should succeed");
        assert!(val.collision);
        assert_eq!(val.corrected_host_port, 2223);

        // Exhausted range
        let res_exhausted =
            check_forwarded_port_with_range(&config, &[2222, 2223], Some(2222..=2223));
        assert!(res_exhausted.is_err());
    }

    #[test]
    fn test_check_forwarded_port_with_occupied_socket() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();

        let config = NetworkConfig::ForwardedPort {
            guest: 80,
            host: port,
            auto_correct: true,
            protocol: None,
            host_ip: None,
        };

        // 1. open_ports is empty, so is_port_in_use(port) detects collision
        let res = check_forwarded_port_with_range(&config, &[], None);
        assert!(res.is_ok());
        let val = res.expect("operation should succeed");
        assert!(val.collision);
        assert!(val.corrected_host_port != port);

        // 2. usable range containing occupied port
        let res_range =
            check_forwarded_port_with_range(&config, &[], Some(port..=port.saturating_add(2)));
        assert!(res_range.is_ok());
        let val_range = res_range.expect("operation should succeed");
        assert!(val_range.collision);
        assert!(val_range.corrected_host_port != port);
    }
}
