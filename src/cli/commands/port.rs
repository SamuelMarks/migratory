//! Semantic implementation of the `port` command.
//!
//! This module provides the logic to output the guest port bindings.

use crate::cli::PortArgs;
use crate::error::MigratoryError;
use std::path::Path;

/// Forwarded port binding representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedPortEntry {
    /// Guest port number.
    pub guest: u16,
    /// Host port number.
    pub host: u16,
    /// Network protocol ("tcp" or "udp").
    pub protocol: String,
}

/// Parses forwarded port entries from `VBoxManage showvminfo --machinereadable` output.
///
/// # Arguments
///
/// * `text` - The standard output string from `VBoxManage showvminfo`.
///
/// # Returns
///
/// Returns `Some(Vec<ForwardedPortEntry>)` if valid entries are found, or `None`.
pub fn parse_vboxmanage_ports(text: &str) -> Option<Vec<ForwardedPortEntry>> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        // Example: Forwarding(0)="ssh,tcp,127.0.0.1,2222,,22"
        if let Some(stripped) = trimmed.strip_prefix("Forwarding(")
            && let Some(pos) = stripped.find("=\"")
        {
            let rule = stripped[pos + 2..].trim_end_matches('"');
            let parts: Vec<&str> = rule.split(',').collect();
            if parts.len() >= 6 {
                let proto = parts[1].to_string();
                if let (Ok(h), Ok(g)) = (parts[3].parse::<u16>(), parts[5].parse::<u16>()) {
                    entries.push(ForwardedPortEntry {
                        guest: g,
                        host: h,
                        protocol: proto,
                    });
                }
            }
        }
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// Queries live forwarded ports for a VirtualBox machine by parsing `showvminfo`.
///
/// # Arguments
///
/// * `machine_id` - The VirtualBox machine UUID or name.
///
/// # Returns
///
/// Returns a list of `ForwardedPortEntry` if successfully retrieved from `VBoxManage`.
fn query_virtualbox_ports(machine_id: &str) -> Option<Vec<ForwardedPortEntry>> {
    #[cfg(test)]
    if let Ok(mock) = std::env::var("MIGRATORY_TEST_MOCK_LIVE_PORTS") {
        let mut entries = Vec::new();
        for item in mock.split(';') {
            let parts: Vec<&str> = item.split(',').collect();
            if parts.len() == 3 {
                if let (Ok(g), Ok(h)) = (parts[0].parse(), parts[1].parse()) {
                    entries.push(ForwardedPortEntry {
                        guest: g,
                        host: h,
                        protocol: parts[2].to_string(),
                    });
                }
            }
        }
        return Some(entries);
    }

    let output = std::process::Command::new("VBoxManage")
        .args(["showvminfo", machine_id, "--machinereadable"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_vboxmanage_ports(&text)
}

/// Executes the `port` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `port` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or if a specified machine is missing.
pub fn execute(cwd: &Path, args: &PortArgs) -> Result<(), MigratoryError> {
    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = crate::config::evaluate_vagrantfile(path_str).unwrap_or_default();

    if let Some(target_name) = &args.name
        && !env_config.machines.contains_key(target_name)
    {
        return Err(MigratoryError::NotFound(format!(
            "Machine '{}' not found",
            target_name
        )));
    }

    if let Some(guest_port) = args.guest {
        println!("Looking up host port for guest port {}", guest_port);
    } else {
        println!("The forwarded ports for the machine are listed below. Please note that");
        println!("these values may differ from values configured in the Vagrantfile if the");
        println!("provider supports automatic port collision resolution and the port was in use.");
    }

    let state_mgr = crate::provider::StateManager::new(cwd.join(".vagrant"));

    for (m_name, machine) in &env_config.machines {
        if let Some(target_name) = &args.name
            && m_name != target_name
        {
            continue;
        }

        let provider_name = machine
            .vm
            .providers
            .first()
            .map(|p| p.name.as_str())
            .unwrap_or("virtualbox");

        let live_ports = if provider_name == "virtualbox" {
            state_mgr
                .read_id(m_name, "virtualbox")
                .ok()
                .flatten()
                .and_then(|id| query_virtualbox_ports(&id))
        } else {
            None
        };

        if let Some(ports) = live_ports {
            for port in ports {
                if let Some(target_guest) = args.guest
                    && port.guest != target_guest
                {
                    continue;
                }
                println!("{} ({}) -> {}", port.guest, port.protocol, port.host);
            }
        } else {
            for net in &machine.vm.networks {
                if let crate::config::NetworkConfig::ForwardedPort {
                    guest,
                    host,
                    protocol,
                    ..
                } = net
                {
                    if let Some(target_guest) = args.guest
                        && *guest != target_guest
                    {
                        continue;
                    }
                    println!(
                        "{} ({}) -> {}",
                        guest,
                        protocol.as_deref().unwrap_or("tcp"),
                        host
                    );
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_port_missing() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let args = PortArgs {
            name: None,
            guest: None,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_port_success() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PortArgs {
            name: None,
            guest: Some(80),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_port_success_no_guest() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PortArgs {
            name: None,
            guest: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_port_unknown_machine() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PortArgs {
            name: Some("unknown_machine".to_string()),
            guest: None,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_port_with_forwarded_ports() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web" do |web|
    web.vm.network "forwarded_port", guest: 80, host: 8080
    web.vm.network "forwarded_port", guest: 443, host: 8443, protocol: "tcp"
  end
  config.vm.define "db" do |db|
    db.vm.network "forwarded_port", guest: 3306, host: 3306
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), content).expect("write failed");

        let args_all = PortArgs {
            name: None,
            guest: None,
        };
        assert!(execute(cwd, &args_all).is_ok());

        let args_guest = PortArgs {
            name: Some("web".to_string()),
            guest: Some(80),
        };
        assert!(execute(cwd, &args_guest).is_ok());

        let args_db = PortArgs {
            name: Some("db".to_string()),
            guest: None,
        };
        assert!(execute(cwd, &args_db).is_ok());
    }

    #[test]
    fn test_execute_port_with_live_ports() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web" do |web|
    web.vm.network "forwarded_port", guest: 80, host: 8080
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), content).expect("write failed");

        let state_dir = cwd.join(".vagrant/machines/web/virtualbox");
        fs::create_dir_all(&state_dir).expect("create_dir failed");
        fs::write(state_dir.join("id"), "web-vm-uuid").expect("write failed");

        unsafe {
            std::env::set_var(
                "MIGRATORY_TEST_MOCK_LIVE_PORTS",
                "80,8081,tcp;invalid_parts;bad_guest,8082,tcp;443,8443,tcp",
            );
        }

        let args = PortArgs {
            name: Some("web".to_string()),
            guest: Some(80),
        };
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_LIVE_PORTS");
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_vboxmanage_ports_edge_cases() {
        assert_eq!(parse_vboxmanage_ports(""), None);
        assert_eq!(parse_vboxmanage_ports("VMState=\"running\"\n"), None);
        assert_eq!(parse_vboxmanage_ports("Forwarding(0)broken\n"), None);
        assert_eq!(
            parse_vboxmanage_ports("Forwarding(0)=\"short,tcp,127.0.0.1\"\n"),
            None
        );
        assert_eq!(
            parse_vboxmanage_ports("Forwarding(0)=\"rule,tcp,127.0.0.1,not_a_port,,80\"\n"),
            None
        );
        assert_eq!(
            parse_vboxmanage_ports("Forwarding(0)=\"rule,tcp,127.0.0.1,8080,,not_a_port\"\n"),
            None
        );

        let valid = "Forwarding(0)=\"rule,tcp,127.0.0.1,8080,,80\"\nForwarding(1)=\"ssh,tcp,127.0.0.1,2222,,22\"\n";
        let parsed = parse_vboxmanage_ports(valid).expect("should parse");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].guest, 80);
        assert_eq!(parsed[0].host, 8080);
        assert_eq!(parsed[0].protocol, "tcp");
        assert_eq!(parsed[1].guest, 22);
        assert_eq!(parsed[1].host, 2222);
        assert_eq!(parsed[1].protocol, "tcp");
    }

    #[test]
    fn test_query_virtualbox_ports_with_mock_vboxmanage() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        let dir = tempdir().expect("tempdir failed");
        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("create_dir failed");

        // First test without mock executable (fails)
        let old_path = std::env::var_os("PATH");
        unsafe {
            std::env::set_var("PATH", &bin_dir);
            std::env::remove_var("MIGRATORY_TEST_MOCK_LIVE_PORTS");
        }
        assert_eq!(query_virtualbox_ports("vm-id-1"), None);

        // Now test with failing mock VBoxManage (exit 1)
        let script_path = bin_dir.join("VBoxManage");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::write(&script_path, "#!/bin/sh\nexit 1\n").expect("write failed");
            let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms).expect("set_permissions");
        }
        #[cfg(windows)]
        {
            fs::write(bin_dir.join("VBoxManage.bat"), "@echo off\nexit /b 1\n")
                .expect("write failed");
        }
        assert_eq!(query_virtualbox_ports("vm-id-2"), None);

        // Now test with succeeding mock VBoxManage returning Forwarding rules
        #[cfg(unix)]
        {
            fs::write(
                &script_path,
                "#!/bin/sh\necho 'Forwarding(0)=\"ssh,tcp,127.0.0.1,2222,,22\"'\nexit 0\n",
            )
            .expect("write failed");
        }
        #[cfg(windows)]
        {
            fs::write(
                bin_dir.join("VBoxManage.bat"),
                "@echo off\necho Forwarding(0)=\"ssh,tcp,127.0.0.1,2222,,22\"\nexit /b 0\n",
            )
            .expect("write failed");
        }
        let ports = query_virtualbox_ports("vm-id-3").expect("should return ports");
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].guest, 22);
        assert_eq!(ports[0].host, 2222);

        unsafe {
            if let Some(p) = old_path {
                std::env::set_var("PATH", p);
            }
        }
    }

    #[test]
    fn test_execute_port_custom_provider_and_private_network() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "custom" do |node|
    node.vm.provider "docker" do |d|
    end
    node.vm.network "private_network", ip: "192.168.56.10"
    node.vm.network "forwarded_port", guest: 80, host: 8080
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), content).expect("write failed");

        let args = PortArgs {
            name: Some("custom".to_string()),
            guest: Some(80),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
}
