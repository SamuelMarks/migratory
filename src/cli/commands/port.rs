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
#[coverage(off)]
pub fn query_virtualbox_ports(machine_id: &str) -> Option<Vec<ForwardedPortEntry>> {
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

/// Parses forwarded port entries from `docker port <container>` output.
///
/// Output format example:
/// ```text
/// 80/tcp -> 0.0.0.0:8080
/// 80/tcp -> :::8080
/// 443/udp -> 127.0.0.1:8443
/// ```
///
/// # Arguments
///
/// * `text` - Output text from `docker port`.
///
/// # Returns
///
/// Returns `Some(Vec<ForwardedPortEntry>)` if valid entries are found, or `None`.
#[coverage(off)]
pub fn parse_docker_ports(text: &str) -> Option<Vec<ForwardedPortEntry>> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some((left, right)) = trimmed.split_once(" -> ") {
            let left_parts: Vec<&str> = left.split('/').collect();
            if left_parts.len() == 2
                && let Ok(guest) = left_parts[0].parse::<u16>()
            {
                let protocol = left_parts[1].to_lowercase();
                if let Some(pos) = right.rfind(':')
                    && let Ok(host) = right[pos + 1..].parse::<u16>()
                {
                    let entry = ForwardedPortEntry {
                        guest,
                        host,
                        protocol,
                    };
                    if !entries.contains(&entry) {
                        entries.push(entry);
                    }
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

/// Queries live forwarded ports for a Docker container.
///
/// # Arguments
///
/// * `container_id` - The Docker container name or ID.
///
/// # Returns
///
/// Returns a list of `ForwardedPortEntry` if successfully retrieved from `docker port`.
#[coverage(off)]
pub fn query_docker_ports(container_id: &str) -> Option<Vec<ForwardedPortEntry>> {
    #[cfg(test)]
    if let Ok(mock) = std::env::var("MIGRATORY_TEST_MOCK_DOCKER_PORTS") {
        return parse_docker_ports(&mock);
    }

    let output = std::process::Command::new("docker")
        .args(["port", container_id])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_docker_ports(&text)
}

/// Parses forwarded port entries from QEMU monitor output or command line arguments.
///
/// Supports:
/// - QEMU monitor `info usernet`: `TCP[HOST_FORWARD] 13 127.0.0.1 2222 10.0.2.15 22`
/// - QEMU hostfwd argument: `hostfwd=tcp::2222-:22`
///
/// # Arguments
///
/// * `text` - Output text from virsh/qemu monitor.
///
/// # Returns
///
/// Returns `Some(Vec<ForwardedPortEntry>)` if valid entries are found, or `None`.
#[coverage(off)]
pub fn parse_qemu_ports(text: &str) -> Option<Vec<ForwardedPortEntry>> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        for part in trimmed.split("hostfwd=").skip(1) {
            let spec = part.split([',', ' ']).next().unwrap_or(part);
            if let Some((host_part, guest_part)) = spec.split_once('-') {
                let proto = host_part.split(':').next().unwrap_or("tcp").to_lowercase();
                let host_port = host_part
                    .rfind(':')
                    .and_then(|p| host_part[p + 1..].parse::<u16>().ok());
                let guest_port = guest_part
                    .rfind(':')
                    .and_then(|p| guest_part[p + 1..].parse::<u16>().ok());
                if let (Some(host), Some(guest)) = (host_port, guest_port) {
                    let entry = ForwardedPortEntry {
                        guest,
                        host,
                        protocol: proto,
                    };
                    if !entries.contains(&entry) {
                        entries.push(entry);
                    }
                }
            }
        }

        if trimmed.contains("[HOST_FORWARD]") {
            let proto = if trimmed.starts_with("UDP") {
                "udp".to_string()
            } else {
                "tcp".to_string()
            };
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 6
                && let (Ok(host), Ok(guest)) = (parts[3].parse::<u16>(), parts[5].parse::<u16>())
            {
                let entry = ForwardedPortEntry {
                    guest,
                    host,
                    protocol: proto,
                };
                if !entries.contains(&entry) {
                    entries.push(entry);
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

/// Queries live forwarded ports for a QEMU domain via virsh qemu-monitor-command.
///
/// # Arguments
///
/// * `domain_id` - The domain name or UUID.
///
/// # Returns
///
/// Returns a list of `ForwardedPortEntry` if successfully retrieved from `virsh`.
#[coverage(off)]
pub fn query_qemu_ports(domain_id: &str) -> Option<Vec<ForwardedPortEntry>> {
    #[cfg(test)]
    if let Ok(mock) = std::env::var("MIGRATORY_TEST_MOCK_QEMU_PORTS") {
        return parse_qemu_ports(&mock);
    }

    let output = std::process::Command::new("virsh")
        .args(["qemu-monitor-command", domain_id, "--hmp", "info usernet"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_qemu_ports(&text)
}

/// Parses forwarded port entries from PowerShell `Get-NetNatStaticMapping` output.
///
/// Supports formatted key-value blocks and JSON objects.
///
/// # Arguments
///
/// * `text` - Output text from PowerShell.
///
/// # Returns
///
/// Returns `Some(Vec<ForwardedPortEntry>)` if valid entries are found, or `None`.
#[coverage(off)]
pub fn parse_hyperv_ports(text: &str) -> Option<Vec<ForwardedPortEntry>> {
    let mut entries = Vec::new();

    if let Ok(val) = serde_json::from_str::<serde_json::Value>(text) {
        let items = if let Some(arr) = val.as_array() {
            arr.clone()
        } else if val.is_object() {
            vec![val]
        } else {
            Vec::new()
        };

        for item in items {
            let proto = item
                .get("Protocol")
                .and_then(|v| v.as_str())
                .unwrap_or("TCP")
                .to_lowercase();
            let host = item
                .get("ExternalPort")
                .and_then(|v| v.as_u64())
                .and_then(|p| u16::try_from(p).ok());
            let guest = item
                .get("InternalPort")
                .and_then(|v| v.as_u64())
                .and_then(|p| u16::try_from(p).ok());

            if let (Some(h), Some(g)) = (host, guest) {
                let entry = ForwardedPortEntry {
                    guest: g,
                    host: h,
                    protocol: proto,
                };
                if !entries.contains(&entry) {
                    entries.push(entry);
                }
            }
        }
        if !entries.is_empty() {
            return Some(entries);
        }
    }

    let mut current_proto = None;
    let mut current_host = None;
    let mut current_guest = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let (Some(proto), Some(host), Some(guest)) = (
                current_proto.take(),
                current_host.take(),
                current_guest.take(),
            ) {
                let entry = ForwardedPortEntry {
                    guest,
                    host,
                    protocol: proto,
                };
                if !entries.contains(&entry) {
                    entries.push(entry);
                }
            }
            continue;
        }

        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim();
            let val = v.trim();
            match key {
                "Protocol" => current_proto = Some(val.to_lowercase()),
                "ExternalPort" => current_host = val.parse::<u16>().ok(),
                "InternalPort" => current_guest = val.parse::<u16>().ok(),
                _ => {}
            }
        }
    }

    if let (Some(proto), Some(host), Some(guest)) = (current_proto, current_host, current_guest) {
        let entry = ForwardedPortEntry {
            guest,
            host,
            protocol: proto,
        };
        if !entries.contains(&entry) {
            entries.push(entry);
        }
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// Queries live forwarded ports for Hyper-V using PowerShell `Get-NetNatStaticMapping`.
///
/// # Arguments
///
/// * `_vm_id` - The Hyper-V virtual machine identifier.
///
/// # Returns
///
/// Returns a list of `ForwardedPortEntry` if successfully retrieved from PowerShell.
#[coverage(off)]
pub fn query_hyperv_ports(_vm_id: &str) -> Option<Vec<ForwardedPortEntry>> {
    #[cfg(test)]
    if let Ok(mock) = std::env::var("MIGRATORY_TEST_MOCK_HYPERV_PORTS") {
        return parse_hyperv_ports(&mock);
    }

    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-NetNatStaticMapping | ConvertTo-Json",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_hyperv_ports(&text)
}

/// Parses forwarded port entries from VMware NAT configuration file (`nat.conf`).
///
/// Example sections:
/// ```ini
/// [incomingtcp]
/// 2222 = 192.168.56.10:22
///
/// [incomingudp]
/// 8080 = 192.168.56.10:80
/// ```
///
/// # Arguments
///
/// * `text` - Content of the VMware `nat.conf` file.
///
/// # Returns
///
/// Returns `Some(Vec<ForwardedPortEntry>)` if valid entries are found, or `None`.
#[coverage(off)]
pub fn parse_vmware_nat_conf(text: &str) -> Option<Vec<ForwardedPortEntry>> {
    let mut entries = Vec::new();
    let mut current_section = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = trimmed[1..trimmed.len() - 1].to_lowercase();
            continue;
        }

        let proto = match current_section.as_str() {
            "incomingtcp" => "tcp",
            "incomingudp" => "udp",
            _ => continue,
        };

        if let Some((host_str, target)) = trimmed.split_once('=')
            && let Ok(host) = host_str.trim().parse::<u16>()
        {
            let target_trimmed = target.trim();
            if let Some(pos) = target_trimmed.rfind(':')
                && let Ok(guest) = target_trimmed[pos + 1..].parse::<u16>()
            {
                let entry = ForwardedPortEntry {
                    guest,
                    host,
                    protocol: proto.to_string(),
                };
                if !entries.contains(&entry) {
                    entries.push(entry);
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

/// Queries live forwarded ports for VMware by checking NAT configuration files.
///
/// # Arguments
///
/// * `_vm_id` - The VMware machine identifier or VMX path.
///
/// # Returns
///
/// Returns a list of `ForwardedPortEntry` if successfully retrieved from `nat.conf`.
#[coverage(off)]
pub fn query_vmware_ports(_vm_id: &str) -> Option<Vec<ForwardedPortEntry>> {
    #[cfg(test)]
    if let Ok(mock) = std::env::var("MIGRATORY_TEST_MOCK_VMWARE_PORTS") {
        return parse_vmware_nat_conf(&mock);
    }

    let candidate_paths = [
        "/Library/Preferences/VMware Fusion/vmnet8/nat.conf",
        "/etc/vmware/vmnet8/nat/nat.conf",
        r"C:\ProgramData\VMware\vmnetnat.conf",
    ];

    for path in candidate_paths {
        if let Ok(content) = std::fs::read_to_string(path)
            && let Some(ports) = parse_vmware_nat_conf(&content)
        {
            return Some(ports);
        }
    }

    None
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

        let machine_id = state_mgr.read_id(m_name, provider_name).ok().flatten();

        let live_ports = if let Some(ref id) = machine_id {
            match provider_name {
                "virtualbox" => query_virtualbox_ports(id),
                "docker" => query_docker_ports(id),
                "qemu" | "libvirt" => query_qemu_ports(id),
                "hyperv" | "hyper-v" => query_hyperv_ports(id),
                "vmware" | "vmware_desktop" | "vmware_fusion" | "vmware_workstation" => {
                    query_vmware_ports(id)
                }
                _ => None,
            }
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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
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

        let args_guest_nomatch = PortArgs {
            name: Some("web".to_string()),
            guest: Some(9999),
        };
        assert!(execute(cwd, &args_guest_nomatch).is_ok());

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
        assert!(result.is_ok());

        let args_nomatch = PortArgs {
            name: Some("web".to_string()),
            guest: Some(9999),
        };
        assert!(execute(cwd, &args_nomatch).is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_LIVE_PORTS");
        }
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

        let old_path = std::env::var_os("PATH");
        unsafe {
            std::env::set_var("PATH", &bin_dir);
            std::env::remove_var("MIGRATORY_TEST_MOCK_LIVE_PORTS");
        }
        assert_eq!(query_virtualbox_ports("vm-id-1"), None);

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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
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
  config.vm.define "other" do |node|
    node.vm.provider "unknown_provider"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), content).expect("write failed");

        let other_state = cwd.join(".vagrant/machines/other/unknown_provider");
        fs::create_dir_all(&other_state).expect("mkdir failed");
        fs::write(other_state.join("id"), "other-vm-id").expect("write failed");

        let args = PortArgs {
            name: Some("custom".to_string()),
            guest: Some(80),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        let args_other = PortArgs {
            name: Some("other".to_string()),
            guest: None,
        };
        assert!(execute(cwd, &args_other).is_ok());
    }

    #[test]
    fn test_docker_port_parsing_and_query() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        assert_eq!(parse_docker_ports(""), None);
        assert_eq!(parse_docker_ports("invalid line without arrow\n"), None);
        assert_eq!(parse_docker_ports("bad/tcp -> 0.0.0.0:8080\n"), None);
        assert_eq!(parse_docker_ports("80/tcp -> 0.0.0.0:bad\n"), None);
        assert_eq!(parse_docker_ports("80/tcp -> 0.0.0.0\n"), None);
        assert_eq!(parse_docker_ports("80/tcp/extra -> 0.0.0.0:80\n"), None);

        let sample = "80/tcp -> 0.0.0.0:8080
80/tcp -> :::8080
443/udp -> 127.0.0.1:8443
";
        let parsed = parse_docker_ports(sample).expect("should parse");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].guest, 80);
        assert_eq!(parsed[0].host, 8080);
        assert_eq!(parsed[0].protocol, "tcp");
        assert_eq!(parsed[1].guest, 443);
        assert_eq!(parsed[1].host, 8443);
        assert_eq!(parsed[1].protocol, "udp");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER_PORTS", sample);
        }
        let queried = query_docker_ports("container-123").expect("query should succeed");
        assert_eq!(queried.len(), 2);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER_PORTS");
        }
    }

    #[test]
    fn test_qemu_port_parsing_and_query() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        assert_eq!(parse_qemu_ports(""), None);
        assert_eq!(parse_qemu_ports("random info output\n"), None);
        assert_eq!(parse_qemu_ports("hostfwd=bad\n"), None);
        assert_eq!(parse_qemu_ports("[HOST_FORWARD] short\n"), None);

        let usernet_sample = "
VLAN -1 (net0):
  Protocol[State]    FD  Source Address  Port   Dest. Address  Port RecvQ SendQ
  TCP[HOST_FORWARD]  13       127.0.0.1  2222       10.0.2.15    22     0     0
  UDP[HOST_FORWARD]  14       0.0.0.0    8080       10.0.2.15    80     0     0
";
        let parsed_usernet = parse_qemu_ports(usernet_sample).expect("should parse usernet");
        assert_eq!(parsed_usernet.len(), 2);
        assert_eq!(parsed_usernet[0].guest, 22);
        assert_eq!(parsed_usernet[0].host, 2222);
        assert_eq!(parsed_usernet[0].protocol, "tcp");
        assert_eq!(parsed_usernet[1].guest, 80);
        assert_eq!(parsed_usernet[1].host, 8080);
        assert_eq!(parsed_usernet[1].protocol, "udp");

        let hostfwd_sample = "qemu -net nic -net user,hostfwd=tcp::2222-:22,hostfwd=udp::5353-:53";
        let parsed_hostfwd = parse_qemu_ports(hostfwd_sample).expect("should parse hostfwd");
        assert_eq!(parsed_hostfwd.len(), 2);
        assert_eq!(parsed_hostfwd[0].guest, 22);
        assert_eq!(parsed_hostfwd[0].host, 2222);
        assert_eq!(parsed_hostfwd[1].guest, 53);
        assert_eq!(parsed_hostfwd[1].host, 5353);

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_QEMU_PORTS", usernet_sample);
        }
        let queried = query_qemu_ports("qemu-domain-456").expect("query should succeed");
        assert_eq!(queried.len(), 2);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_QEMU_PORTS");
        }
    }

    #[test]
    fn test_hyperv_port_parsing_and_query() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        assert_eq!(parse_hyperv_ports(""), None);
        assert_eq!(parse_hyperv_ports("Status: Running\n"), None);
        assert_eq!(parse_hyperv_ports("42"), None);

        let kv_sample = "
Protocol     : TCP
ExternalPort : 2222
InternalPort : 22

Protocol     : UDP
ExternalPort : 5353
InternalPort : 53
";
        let parsed_kv = parse_hyperv_ports(kv_sample).expect("should parse key-value");
        assert_eq!(parsed_kv.len(), 2);
        assert_eq!(parsed_kv[0].guest, 22);
        assert_eq!(parsed_kv[0].host, 2222);
        assert_eq!(parsed_kv[0].protocol, "tcp");
        assert_eq!(parsed_kv[1].guest, 53);
        assert_eq!(parsed_kv[1].host, 5353);
        assert_eq!(parsed_kv[1].protocol, "udp");

        let json_sample = r#"[
            {"Protocol": "TCP", "ExternalPort": 8080, "InternalPort": 80},
            {"Protocol": "UDP", "ExternalPort": 9090, "InternalPort": 90}
        ]"#;
        let parsed_json = parse_hyperv_ports(json_sample).expect("should parse json");
        assert_eq!(parsed_json.len(), 2);
        assert_eq!(parsed_json[0].guest, 80);
        assert_eq!(parsed_json[0].host, 8080);
        assert_eq!(parsed_json[0].protocol, "tcp");

        let single_json = r#"{"Protocol": "TCP", "ExternalPort": 3000, "InternalPort": 3000}"#;
        let parsed_single = parse_hyperv_ports(single_json).expect("should parse single json");
        assert_eq!(parsed_single.len(), 1);

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_HYPERV_PORTS", json_sample);
        }
        let queried = query_hyperv_ports("hyperv-vm-789").expect("query should succeed");
        assert_eq!(queried.len(), 2);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_HYPERV_PORTS");
        }
    }

    #[test]
    fn test_vmware_nat_conf_parsing_and_query() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        assert_eq!(parse_vmware_nat_conf(""), None);
        assert_eq!(parse_vmware_nat_conf("# Comment only\n"), None);
        assert_eq!(parse_vmware_nat_conf("[incomingtcp]\n8080 = bad\n"), None);
        assert!(parse_vmware_nat_conf("[incomingtcp]\nbad = 127.0.0.1:80\n").is_none());

        let conf_sample = "
# VMware NAT configuration
[incomingtcp]
2222 = 192.168.56.10:22
8080 = 192.168.56.10:80

[incomingudp]
5353 = 192.168.56.10:53
invalid_line
8888 = missing_colon

[other_section]
9999 = 192.168.56.10:99
";
        let parsed = parse_vmware_nat_conf(conf_sample).expect("should parse nat conf");
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].guest, 22);
        assert_eq!(parsed[0].host, 2222);
        assert_eq!(parsed[0].protocol, "tcp");
        assert_eq!(parsed[1].guest, 80);
        assert_eq!(parsed[1].host, 8080);
        assert_eq!(parsed[1].protocol, "tcp");
        assert_eq!(parsed[2].guest, 53);
        assert_eq!(parsed[2].host, 5353);
        assert_eq!(parsed[2].protocol, "udp");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VMWARE_PORTS", conf_sample);
        }
        let queried = query_vmware_ports("vmware-vm-101").expect("query should succeed");
        assert_eq!(queried.len(), 3);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMWARE_PORTS");
        }
    }

    #[test]
    fn test_execute_port_multi_provider_live() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "docker_node" do |node|
    node.vm.provider "docker"
  end
  config.vm.define "qemu_node" do |node|
    node.vm.provider "qemu"
  end
  config.vm.define "hyperv_node" do |node|
    node.vm.provider "hyperv"
  end
  config.vm.define "vmware_node" do |node|
    node.vm.provider "vmware"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), content).expect("write failed");

        let vagrant_dir = cwd.join(".vagrant/machines");
        fs::create_dir_all(vagrant_dir.join("docker_node/docker")).expect("mkdir failed");
        fs::write(vagrant_dir.join("docker_node/docker/id"), "container-id").expect("write");

        fs::create_dir_all(vagrant_dir.join("qemu_node/qemu")).expect("mkdir failed");
        fs::write(vagrant_dir.join("qemu_node/qemu/id"), "domain-id").expect("write");

        fs::create_dir_all(vagrant_dir.join("hyperv_node/hyperv")).expect("mkdir failed");
        fs::write(vagrant_dir.join("hyperv_node/hyperv/id"), "vm-id").expect("write");

        fs::create_dir_all(vagrant_dir.join("vmware_node/vmware")).expect("mkdir failed");
        fs::write(vagrant_dir.join("vmware_node/vmware/id"), "vmx-path").expect("write");

        unsafe {
            std::env::set_var(
                "MIGRATORY_TEST_MOCK_DOCKER_PORTS",
                "80/tcp -> 0.0.0.0:8080
",
            );
            std::env::set_var(
                "MIGRATORY_TEST_MOCK_QEMU_PORTS",
                "TCP[HOST_FORWARD] 13 127.0.0.1 2222 10.0.2.15 22
",
            );
            std::env::set_var(
                "MIGRATORY_TEST_MOCK_HYPERV_PORTS",
                r#"[{"Protocol": "TCP", "ExternalPort": 3389, "InternalPort": 3389}]"#,
            );
            std::env::set_var(
                "MIGRATORY_TEST_MOCK_VMWARE_PORTS",
                "[incomingtcp]
8443 = 192.168.1.1:443
",
            );
        }

        let args = PortArgs {
            name: None,
            guest: None,
        };
        let result = execute(cwd, &args);

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER_PORTS");
            std::env::remove_var("MIGRATORY_TEST_MOCK_QEMU_PORTS");
            std::env::remove_var("MIGRATORY_TEST_MOCK_HYPERV_PORTS");
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMWARE_PORTS");
        }

        assert!(result.is_ok());
    }
}
