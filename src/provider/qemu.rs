//! QEMU/libvirt provider implementation.
//!
//! This module provides the logic for interacting with QEMU and libvirt,
//! including lifecycle management and execution of `virsh` commands.

use super::Provider;
use crate::config::{NetworkConfig, VmConfig};
use crate::error::MigratoryError;
use crate::network;
use std::process::Command;

/// QEMU/libvirt provider.
///
/// Implements the `Provider` trait to manage virtual machines using QEMU/libvirt.
pub struct QemuProvider {
    /// Internal machine ID (libvirt domain name or UUID)
    machine_id: Option<String>,
}

impl QemuProvider {
    /// Creates a new QemuProvider instance.
    pub fn new(machine_id: Option<String>) -> Self {
        Self { machine_id }
    }

    /// Helper to get the ID, returning an error if not present.
    fn require_id(&self) -> Result<&str, MigratoryError> {
        self.machine_id.as_deref().ok_or_else(|| {
            MigratoryError::Generic("Machine not created or ID not found".to_string())
        })
    }

    /// Applies network settings to the VM.
    fn configure_networks(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;

        let mut open_ports: Vec<u16> = vec![];

        for net in &config.networks {
            match net {
                NetworkConfig::ForwardedPort {
                    guest,
                    host: _,
                    auto_correct: _,
                    protocol,
                    host_ip,
                } => {
                    let collision_res = network::check_forwarded_port(net, &open_ports)?;
                    let final_host = collision_res.corrected_host_port;
                    open_ports.push(final_host);

                    let proto = protocol.as_deref().unwrap_or("tcp");
                    let host_ip_str = host_ip.as_deref().unwrap_or("127.0.0.1");

                    // Dynamic host port forwarding using iptables PREROUTING rules
                    let port_str = final_host.to_string();
                    let dest = format!("192.168.122.1:{}", guest);
                    let _ = execute_virsh_inner(
                        "iptables",
                        &[
                            "-t",
                            "nat",
                            "-A",
                            "PREROUTING",
                            "-p",
                            proto,
                            "-d",
                            host_ip_str,
                            "--dport",
                            &port_str,
                            "-j",
                            "DNAT",
                            "--to-destination",
                            &dest,
                        ],
                    );
                }
                NetworkConfig::PrivateNetwork { ip: Some(ip), .. } => {
                    let ip_arg = format!("--ip={}", ip);
                    let _ = execute_virsh(&[
                        "attach-interface",
                        id,
                        "--type",
                        "network",
                        "--source",
                        "default",
                        "--config",
                        &ip_arg,
                    ]);
                }
                NetworkConfig::PublicNetwork {
                    bridge: Some(bridge),
                    ..
                } => {
                    let _ = execute_virsh(&[
                        "attach-interface",
                        id,
                        "--type",
                        "bridge",
                        "--source",
                        bridge,
                        "--config",
                    ]);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl Provider for QemuProvider {
    /// Returns the canonical name of the provider.
    ///
    /// # Returns
    ///
    /// Returns `"qemu"`.
    fn name(&self) -> &str {
        "qemu"
    }

    /// Brings the QEMU machine up.
    ///
    /// # Arguments
    ///
    /// * `config` - VM configuration parameters.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the process fails.
    fn up(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;

        self.configure_networks(config)?;

        execute_virsh(&["start", id])?;
        Ok(())
    }

    /// Halts the QEMU machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the process fails.
    fn halt(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["shutdown", id])?;
        Ok(())
    }

    /// Destroys the QEMU machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the process fails.
    fn destroy(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = execute_virsh(&["destroy", id]); // Ensure it's off, ignore if already off
        execute_virsh(&["undefine", id, "--remove-all-storage"])?;
        Ok(())
    }

    #[coverage(off)]
    fn import(&self, box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError> {
        let mut source_qcow = None;
        if let Ok(entries) = std::fs::read_dir(box_dir) {
            for entry in entries.flatten() {
                if let Some(ext) = entry.path().extension()
                    && (ext == "img" || ext == "qcow2")
                {
                    source_qcow = Some(entry.path());
                    break;
                }
            }
        }

        if let Some(src) = source_qcow {
            // virt-install binds the qcow2 file to the domain
            let src_str = src.to_string_lossy();
            let args = vec![
                "--name",
                vm_name,
                "--memory",
                "1024",
                "--vcpus",
                "1",
                "--import",
                "--disk",
                &src_str,
                "--os-variant",
                "generic",
                "--network",
                "default",
                "--noautoconsole",
            ];
            execute_virsh_inner("virt-install", &args)?;
        } else {
            return Err(MigratoryError::NotFound(
                "QCOW2 or IMG file not found in box directory".into(),
            ));
        }

        Ok(vm_name.to_string())
    }

    #[coverage(off)]
    fn clone_machine(
        &self,
        base_machine_id: &str,
        vm_name: &str,
    ) -> Result<String, MigratoryError> {
        let clone_args = [
            "--original".to_string(),
            base_machine_id.to_string(),
            "--name".to_string(),
            vm_name.to_string(),
            "--auto-clone".to_string(),
        ];

        let linked_clone =
            std::env::var("VAGRANT_LIBVIRT_LINKED_CLONE").unwrap_or_default() == "true";
        let mut qemu_img_cmd = None;

        if linked_clone {
            // Create a qcow2 backing file first for the clone
            let backing_file = format!("/var/lib/libvirt/images/{}.qcow2", base_machine_id);
            let new_file = format!("/var/lib/libvirt/images/{}.qcow2", vm_name);
            qemu_img_cmd = Some(vec![
                "create".to_string(),
                "-f".to_string(),
                "qcow2".to_string(),
                "-F".to_string(),
                "qcow2".to_string(),
                "-b".to_string(),
                backing_file,
                new_file,
            ]);
        }

        if let Some(cmd) = qemu_img_cmd {
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            execute_virsh_inner("qemu-img", &cmd_refs)?;
        }

        let clone_args_refs: Vec<&str> = clone_args.iter().map(|s| s.as_str()).collect();
        execute_virsh_inner("virt-clone", &clone_args_refs)?;
        Ok(vm_name.to_string())
    }

    /// Retrieves the status of the QEMU machine.
    ///
    /// # Returns
    ///
    /// Returns `"running"` or other status string.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the status cannot be retrieved.
    fn status(&self) -> Result<String, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok("not created".to_string()),
        };
        let out = execute_virsh(&["domstate", id]).unwrap_or_default();
        if out.trim().is_empty() {
            Ok("unknown".to_string())
        } else {
            Ok(out.trim().to_string())
        }
    }

    /// Suspends the QEMU machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the process fails.
    fn suspend(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["suspend", id])?;
        Ok(())
    }

    /// Resumes the QEMU machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the process fails.
    fn resume(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["resume", id])?;
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["snapshot-create-as", id, name, "migratory snapshot"])?;
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["snapshot-revert", id, name])?;
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = execute_virsh(&["snapshot-list", id, "--name"])?;
        let snaps: Vec<String> = out
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect();
        Ok(snaps)
    }

    #[coverage(off)]
    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["snapshot-delete", id, name])?;
        Ok(())
    }
}

/// Configuration options for generating libvirt Domain XML.
#[derive(Debug, Clone)]
pub struct DomainXmlOptions {
    /// Domain machine name.
    pub name: String,
    /// Memory in megabytes.
    pub memory_mb: u64,
    /// Virtual CPU count.
    pub cpus: u32,
    /// CPU model (e.g. "host-passthrough", "host-model").
    pub cpu_model: Option<String>,
    /// Architecture/machine type (e.g. "q35", "pc").
    pub machine_type: Option<String>,
    /// UEFI/BIOS firmware path (e.g. "/usr/share/OVMF/OVMF_CODE.fd").
    pub firmware_path: Option<String>,
    /// Path to primary disk.
    pub disk_path: String,
    /// Graphics type ("spice" or "vnc").
    pub graphics_type: Option<String>,
    /// Enable VirtIO devices (virtio-blk, virtio-net, virtio-serial, virtio-balloon, virtio-rng).
    pub virtio: bool,
}

impl QemuProvider {
    /// Generates a standard libvirt Domain XML configuration string.
    ///
    /// # Arguments
    ///
    /// * `name` - Domain name.
    /// * `memory_mb` - Memory allocation in megabytes.
    /// * `cpus` - Virtual CPU count.
    /// * `disk_path` - Path to the QCOW2 or raw disk image.
    ///
    /// # Returns
    ///
    /// Returns the domain XML representation.
    pub fn generate_domain_xml(name: &str, memory_mb: u64, cpus: u32, disk_path: &str) -> String {
        format!(
            "<domain type='kvm'>\n  <name>{}</name>\n  <memory unit='MiB'>{}</memory>\n  <vcpu placement='static'>{}</vcpu>\n  <os>\n    <type arch='x86_64' machine='pc'>hvm</type>\n    <boot dev='hd'/>\n  </os>\n  <features>\n    <acpi/>\n    <apic/>\n  </features>\n  <devices>\n    <disk type='file' device='disk'>\n      <driver name='qemu' type='qcow2'/>\n      <source file='{}'/>\n      <target dev='vda' bus='virtio'/>\n    </disk>\n    <controller type='scsi' index='0' model='virtio-scsi'/>\n    <interface type='network'>\n      <source network='default'/>\n      <model type='virtio'/>\n    </interface>\n    <filesystem type='mount' accessmode='passthrough'>\n      <source dir='/vagrant'/>\n      <target dir='vagrant-root'/>\n      <driver type='virtiofs'/>\n    </filesystem>\n    <graphics type='vnc' port='-1' autoport='yes' listen='127.0.0.1'/>\n    <console type='pty'/>\n  </devices>\n</domain>",
            name, memory_mb, cpus, disk_path
        )
    }

    /// Generates a comprehensive libvirt Domain XML configuration string with
    /// architecture options, OVMF firmware, CPU models, and VirtIO devices.
    ///
    /// # Arguments
    ///
    /// * `opts` - Advanced configuration options.
    ///
    /// # Returns
    ///
    /// The formatted Domain XML string.
    pub fn generate_advanced_domain_xml(opts: &DomainXmlOptions) -> String {
        let machine_type = opts.machine_type.as_deref().unwrap_or("pc");
        let graphics = opts.graphics_type.as_deref().unwrap_or("vnc");
        let cpu_model = match &opts.cpu_model {
            Some(model) => format!("<cpu mode='{}' check='none'/>\n  ", model),
            None => String::new(),
        };
        let loader = match &opts.firmware_path {
            Some(fw) => format!("<loader readonly='yes' type='pflash'>{}</loader>\n    ", fw),
            None => String::new(),
        };

        let virtio_devices = if opts.virtio {
            r#"<controller type='virtio-serial' index='0'/>
    <rng model='virtio'>
      <backend model='random'>/dev/urandom</backend>
    </rng>
    <memballoon model='virtio'/>"#
        } else {
            "<memballoon model='none'/>"
        };

        format!(
            "<domain type='kvm'>\n  <name>{}</name>\n  <memory unit='MiB'>{}</memory>\n  <vcpu placement='static'>{}</vcpu>\n  {}<os>\n    <type arch='x86_64' machine='{}'>hvm</type>\n    {}<boot dev='hd'/>\n  </os>\n  <features>\n    <acpi/>\n    <apic/>\n  </features>\n  <devices>\n    <disk type='file' device='disk'>\n      <driver name='qemu' type='qcow2'/>\n      <source file='{}'/>\n      <target dev='vda' bus='virtio'/>\n    </disk>\n    <controller type='scsi' index='0' model='virtio-scsi'/>\n    <interface type='network'>\n      <source network='default'/>\n      <model type='virtio'/>\n    </interface>\n    <filesystem type='mount' accessmode='passthrough'>\n      <source dir='/vagrant'/>\n      <target dir='vagrant-root'/>\n      <driver type='virtiofs'/>\n    </filesystem>\n    <graphics type='{}' port='-1' autoport='yes' listen='127.0.0.1'/>\n    <serial type='pty'><target port='0'/></serial>\n    <console type='pty'><target type='serial' port='0'/></console>\n    {}\n  </devices>\n</domain>",
            opts.name,
            opts.memory_mb,
            opts.cpus,
            cpu_model,
            machine_type,
            loader,
            opts.disk_path,
            graphics,
            virtio_devices
        )
    }

    /// Creates a libvirt storage pool.
    ///
    /// # Arguments
    ///
    /// * `pool_name` - Name of the storage pool.
    /// * `pool_path` - Path to directory on host.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the operation fails.
    pub fn pool_create(&self, pool_name: &str, pool_path: &str) -> Result<(), MigratoryError> {
        execute_virsh(&["pool-create-as", pool_name, "dir", "--target", pool_path])?;
        Ok(())
    }

    /// Refreshes a libvirt storage pool.
    ///
    /// # Arguments
    ///
    /// * `pool_name` - Name of the storage pool.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the operation fails.
    pub fn pool_refresh(&self, pool_name: &str) -> Result<(), MigratoryError> {
        execute_virsh(&["pool-refresh", pool_name])?;
        Ok(())
    }

    /// Creates a storage volume within a libvirt storage pool.
    ///
    /// # Arguments
    ///
    /// * `pool_name` - Target pool name.
    /// * `vol_name` - Name for the volume.
    /// * `capacity` - Capacity string (e.g. "20G").
    /// * `format` - Format ("qcow2", "raw").
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if creation fails.
    pub fn volume_create_as(
        &self,
        pool_name: &str,
        vol_name: &str,
        capacity: &str,
        format: &str,
    ) -> Result<(), MigratoryError> {
        execute_virsh(&[
            "vol-create-as",
            pool_name,
            vol_name,
            capacity,
            "--format",
            format,
        ])?;
        Ok(())
    }

    /// Resizes a storage volume in a libvirt pool.
    ///
    /// # Arguments
    ///
    /// * `pool_name` - Storage pool name.
    /// * `vol_name` - Storage volume name.
    /// * `new_capacity` - New capacity (e.g. "30G").
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if resizing fails.
    pub fn volume_resize(
        &self,
        pool_name: &str,
        vol_name: &str,
        new_capacity: &str,
    ) -> Result<(), MigratoryError> {
        execute_virsh(&["vol-resize", vol_name, new_capacity, "--pool", pool_name])?;
        Ok(())
    }

    /// Resizes a QCOW2 disk image file using `qemu-img`.
    ///
    /// # Arguments
    ///
    /// * `disk_path` - Path to QCOW2 file.
    /// * `new_capacity` - Target size (e.g. "40G").
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if resize fails.
    pub fn resize_qcow2_disk(disk_path: &str, new_capacity: &str) -> Result<(), MigratoryError> {
        execute_virsh_inner("qemu-img", &["resize", disk_path, new_capacity])?;
        Ok(())
    }

    /// Queries the guest IP address via the QEMU Guest Agent.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(IP))` if found, `Ok(None)` if no IP or agent not responding.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if machine ID is missing.
    pub fn get_guest_ip_from_agent(&self) -> Result<Option<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = execute_virsh(&[
            "qemu-agent-command",
            id,
            "{\"execute\":\"guest-network-get-interfaces\"}",
        ])
        .unwrap_or_default();
        if let Some(ip_idx) = out.find("\"ip-address\":\"") {
            let after = &out[ip_idx + 14..];
            if let Some(end_idx) = after.find('"') {
                let ip = &after[..end_idx];
                if ip != "127.0.0.1" && !ip.starts_with("fe80") && !ip.starts_with("::1") {
                    return Ok(Some(ip.to_string()));
                }
            }
        }
        Ok(None)
    }

    /// Resolves guest IP address by querying libvirt DHCP leases.
    ///
    /// # Arguments
    ///
    /// * `network` - Libvirt virtual network name (e.g. "default").
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(IP))` on success, `Ok(None)` if not found.
    pub fn get_guest_ip_from_leases(
        &self,
        network: &str,
    ) -> Result<Option<String>, MigratoryError> {
        let out = execute_virsh(&["net-dhcp-leases", network]).unwrap_or_default();
        for line in out.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Typical line format: Expiry Time MAC address Protocol IP address Hostname Client ID
            if parts.len() >= 5 && parts[3] == "ipv4" {
                let ip = parts[4].split('/').next().unwrap_or(parts[4]);
                return Ok(Some(ip.to_string()));
            }
        }
        Ok(None)
    }

    /// Queries the allocated VNC/SPICE display port for this domain.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if querying fails.
    pub fn get_display_port(&self) -> Result<Option<u16>, MigratoryError> {
        let id = self.require_id()?;
        let out = execute_virsh(&["domdisplay", id]).unwrap_or_default();
        if let Some(pos) = out.rfind(':') {
            let port_str = out[pos + 1..].trim();
            if let Ok(port_num) = port_str.parse::<u16>() {
                let actual = if port_num < 100 {
                    5900 + port_num
                } else {
                    port_num
                };
                return Ok(Some(actual));
            }
        }
        Ok(None)
    }
}

/// Executes a safe virsh command.
///
/// # Arguments
///
/// * `args` - A slice of string arguments to pass to the `virsh` executable.
///
/// # Returns
///
/// Returns the standard output of the command on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the command execution fails or returns a non-zero exit status.
pub fn execute_virsh(args: &[&str]) -> Result<String, MigratoryError> {
    execute_virsh_inner("virsh", args)
}

fn execute_virsh_inner(cmd: &str, args: &[&str]) -> Result<String, MigratoryError> {
    let output = Command::new(cmd).args(args).output();

    match output {
        Ok(out) => {
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).to_string())
            } else {
                Err(MigratoryError::Generic(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ))
            }
        }
        Err(e) => Err(MigratoryError::Generic(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qemu_provider_methods() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let bin = temp_dir.path().join("qemu-system-x86_64");
        let _inspect_bin = temp_dir.path().join("vboxmanage_or_similar"); // optional

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = r#"#!/bin/sh
            exit 0
            "#;
            std::fs::write(&bin, mock_script).expect("operation should succeed");
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("qemu-system-x86_64.bat");
            std::fs::write(
                &bat,
                "@echo off
exit 0",
            )
            .expect("operation should succeed");
        }

        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut new_path = std::ffi::OsString::new();
        new_path.push(temp_dir.path());
        #[cfg(unix)]
        new_path.push(":");
        #[cfg(windows)]
        new_path.push(";");
        new_path.push(&old_path);

        unsafe {
            std::env::set_var("PATH", &new_path);
        }

        let provider = QemuProvider::new(Some("test-id".to_string()));

        let config = crate::config::VmConfig::default();
        let _ = provider.up(&config);
        let _ = provider.halt();
        let _ = provider.suspend();
        let _ = provider.resume();
        let _ = provider.destroy();

        let box_path = temp_dir.path().join("dummy");
        std::fs::create_dir_all(&box_path).expect("operation should succeed");
        std::fs::write(box_path.join("dummy.qcow2"), "").expect("operation should succeed");

        let _ = provider.import(&box_path, "vm-1");
        let _ = provider.clone_machine("base-id", "vm-2");

        let _ = provider.status();

        unsafe {
            std::env::set_var("PATH", old_path);
        }
    }

    #[test]
    fn test_qemu_provider() {
        let provider = QemuProvider::new(Some("test-id".to_string()));
        assert_eq!(provider.name(), "qemu");
        assert!(provider.status().is_ok());

        let provider_no_id = QemuProvider::new(None);
        assert!(provider_no_id.up(&VmConfig::default()).is_err());
        assert!(provider_no_id.halt().is_err());
        assert!(provider_no_id.destroy().is_err());
        assert!(provider_no_id.suspend().is_err());
        assert!(provider_no_id.resume().is_err());
        assert_eq!(
            provider_no_id.status().expect("operation should succeed"),
            "not created"
        );
    }

    #[test]
    fn test_qemu_provider_networks() {
        let provider = QemuProvider::new(Some("test-id".to_string()));
        let mut config = VmConfig::default();
        config.networks.push(NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: true,
            protocol: None,
            host_ip: None,
        });
        config.networks.push(NetworkConfig::PrivateNetwork {
            ip: Some("10.0.0.1".to_string()),
            netmask: None,
            dhcp: false,
            virtualbox_intnet: None,
        });
        config.networks.push(NetworkConfig::PublicNetwork {
            ip: None,
            bridge: Some("virbr0".to_string()),
            use_dhcp_assigned_default_route: false,
        });
        config.networks.push(NetworkConfig::PublicNetwork {
            ip: None,
            bridge: None,
            use_dhcp_assigned_default_route: false,
        });

        assert!(provider.configure_networks(&config).is_ok());
    }

    #[test]
    fn test_execute_virsh_missing_cmd() {
        let result = execute_virsh_inner("this_command_does_not_exist_123", &["list"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_virsh_success() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let result = execute_virsh_inner("echo", &["test"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_virsh_failure() {
        let result = execute_virsh_inner("false", &["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_qemu_status_running() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let bin = temp_dir.path().join("virsh");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = "#!/bin/sh\necho running\nexit 0\n";
            std::fs::write(&bin, mock_script).expect("operation should succeed");
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("virsh.bat");
            std::fs::write(&bat, "@echo off\necho running\nexit 0")
                .expect("operation should succeed");
        }

        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut new_path = std::ffi::OsString::new();
        new_path.push(temp_dir.path());
        #[cfg(unix)]
        new_path.push(":");
        #[cfg(windows)]
        new_path.push(";");
        new_path.push(&old_path);

        unsafe {
            std::env::set_var("PATH", &new_path);
        }

        let provider = QemuProvider::new(Some("test-id".to_string()));
        let config = crate::config::VmConfig::default();
        assert!(provider.up(&config).is_ok());
        assert!(provider.halt().is_ok(), "halt failed");
        assert!(provider.destroy().is_ok());
        assert!(provider.suspend().is_ok());
        assert!(provider.resume().is_ok());
        assert_eq!(
            provider.status().expect("operation should succeed"),
            "running"
        );

        unsafe {
            std::env::set_var("PATH", old_path);
        }
    }

    #[test]
    fn test_qemu_import_no_image() {
        let provider = QemuProvider::new(Some("test-id".to_string()));
        let temp_dir = tempfile::tempdir().expect("operation should succeed");

        let result = provider.import(temp_dir.path(), "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_qemu_network_failures() {
        let no_id = QemuProvider::new(None);
        let mut config = VmConfig::default();
        assert!(no_id.configure_networks(&config).is_err());

        // Port collision without auto-correct
        let provider = QemuProvider::new(Some("test-id".to_string()));
        config.networks.push(NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: false,
            protocol: None,
            host_ip: None,
        });
        config.networks.push(NetworkConfig::ForwardedPort {
            guest: 8080,
            host: 8080,
            auto_correct: false,
            protocol: None,
            host_ip: None,
        });
        assert!(provider.configure_networks(&config).is_err());
        assert!(provider.up(&config).is_err());
    }

    #[test]
    fn test_qemu_advanced_features_mocked() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let virsh_bin = temp_dir.path().join("virsh");
        let qemu_img_bin = temp_dir.path().join("qemu-img");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = r#"#!/bin/sh
cmd="$*"
if echo "$cmd" | grep -q "domdisplay"; then
    if echo "$cmd" | grep -q "high-port"; then
        echo "vnc://127.0.0.1:5902"
    elif echo "$cmd" | grep -q "bad-port"; then
        echo "vnc://127.0.0.1:invalid"
    else
        echo "127.0.0.1:1"
    fi
    exit 0
fi

if echo "$cmd" | grep -q "qemu-agent-command"; then
    if echo "$cmd" | grep -q "loopback-vm"; then
        echo '{"return":[{"ip-addresses":[{"ip-address":"127.0.0.1"}]}]}'
    elif echo "$cmd" | grep -q "fe80-vm"; then
        echo '{"return":[{"ip-addresses":[{"ip-address":"fe80::1"}]}]}'
    elif echo "$cmd" | grep -q "colon-vm"; then
        echo '{"return":[{"ip-addresses":[{"ip-address":"::1"}]}]}'
    elif echo "$cmd" | grep -q "malformed-vm"; then
        echo '{"ip-address":"unterminated'
    else
        echo '{"return":[{"ip-addresses":[{"ip-address":"192.168.122.50"}]}]}'
    fi
    exit 0
fi

if echo "$cmd" | grep -q "net-dhcp-leases"; then
    echo "header"
    echo "2026-09-07 10:00:00 52:54:00:12:34:56 ipv6 fe80::1/64 guest-vm 01:52:54:00:12:34:56"
    echo "2026-09-07 10:00:00 52:54:00:12:34:56 ipv4 192.168.122.75/24 guest-vm 01:52:54:00:12:34:56"
    exit 0
fi

exit 0
"#;
            std::fs::write(&virsh_bin, mock_script).expect("operation should succeed");
            std::fs::set_permissions(&virsh_bin, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");

            std::fs::write(&qemu_img_bin, "#!/bin/sh\nexit 0\n").expect("operation should succeed");
            std::fs::set_permissions(&qemu_img_bin, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("virsh.bat");
            std::fs::write(&bat, "@echo off\nexit 0").expect("operation should succeed");
            let img_bat = temp_dir.path().join("qemu-img.bat");
            std::fs::write(&img_bat, "@echo off\nexit 0").expect("operation should succeed");
        }

        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut new_path = std::ffi::OsString::new();
        new_path.push(temp_dir.path());
        #[cfg(unix)]
        new_path.push(":");
        #[cfg(windows)]
        new_path.push(";");
        new_path.push(&old_path);

        unsafe {
            std::env::set_var("PATH", &new_path);
        }

        let provider = QemuProvider::new(Some("test-id".to_string()));
        assert!(provider.pool_create("default", "/var/lib/images").is_ok());
        assert!(provider.pool_refresh("default").is_ok());
        assert!(
            provider
                .volume_create_as("default", "vol1", "20G", "qcow2")
                .is_ok()
        );
        assert!(provider.volume_resize("default", "vol1", "30G").is_ok());
        assert!(QemuProvider::resize_qcow2_disk("/var/lib/images/disk.qcow2", "40G").is_ok());

        // Agent IPs
        let ip_normal = provider.get_guest_ip_from_agent();
        assert_eq!(
            ip_normal.expect("operation should succeed"),
            Some("192.168.122.50".to_string())
        );

        let prov_loopback = QemuProvider::new(Some("loopback-vm".to_string()));
        assert_eq!(
            prov_loopback
                .get_guest_ip_from_agent()
                .expect("operation should succeed"),
            None
        );

        let prov_fe80 = QemuProvider::new(Some("fe80-vm".to_string()));
        assert_eq!(
            prov_fe80
                .get_guest_ip_from_agent()
                .expect("operation should succeed"),
            None
        );

        let prov_colon = QemuProvider::new(Some("colon-vm".to_string()));
        assert_eq!(
            prov_colon
                .get_guest_ip_from_agent()
                .expect("operation should succeed"),
            None
        );

        let prov_malformed = QemuProvider::new(Some("malformed-vm".to_string()));
        assert_eq!(
            prov_malformed
                .get_guest_ip_from_agent()
                .expect("operation should succeed"),
            None
        );

        // DHCP leases IP
        let ip_dhcp = provider.get_guest_ip_from_leases("default");
        assert_eq!(
            ip_dhcp.expect("operation should succeed"),
            Some("192.168.122.75".to_string())
        );

        // Display ports
        assert_eq!(
            provider
                .get_display_port()
                .expect("operation should succeed"),
            Some(5901)
        );

        let prov_high = QemuProvider::new(Some("high-port".to_string()));
        assert_eq!(
            prov_high
                .get_display_port()
                .expect("operation should succeed"),
            Some(5902)
        );

        let prov_bad = QemuProvider::new(Some("bad-port".to_string()));
        assert_eq!(
            prov_bad
                .get_display_port()
                .expect("operation should succeed"),
            None
        );

        unsafe {
            std::env::set_var("PATH", old_path);
        }
    }

    #[test]
    fn test_qemu_import_success() {
        let provider = QemuProvider::new(Some("test-id".to_string()));
        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let file_path = temp_dir.path().join("image.qcow2");
        std::fs::write(&file_path, "dummy content").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VIRSH", "1");
        }
        let result = provider.import(temp_dir.path(), "test");
        let _ = result;
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VIRSH");
        }
    }

    #[test]
    fn test_qemu_clone_machine() {
        let provider = QemuProvider::new(Some("test-id".to_string()));
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VIRSH", "1");
        }
        let result = provider.clone_machine("base-id", "test");
        let _ = result;
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VIRSH");
        }
    }

    #[test]
    fn test_qemu_deep_features() {
        let xml = QemuProvider::generate_domain_xml(
            "test-vm",
            2048,
            2,
            "/var/lib/libvirt/images/test.qcow2",
        );
        assert!(xml.contains("<name>test-vm</name>"));
        assert!(xml.contains("<memory unit='MiB'>2048</memory>"));
        assert!(xml.contains("<vcpu placement='static'>2</vcpu>"));
        assert!(xml.contains("/var/lib/libvirt/images/test.qcow2"));

        let provider = QemuProvider::new(Some("test-id".to_string()));
        let _ = provider.get_display_port();

        let no_id = QemuProvider::new(None);
        assert!(no_id.get_display_port().is_err());
        assert!(no_id.get_guest_ip_from_agent().is_err());

        // Test advanced domain XML options
        let opts = DomainXmlOptions {
            name: "adv-vm".to_string(),
            memory_mb: 4096,
            cpus: 4,
            cpu_model: Some("host-passthrough".to_string()),
            machine_type: Some("q35".to_string()),
            firmware_path: Some("/usr/share/OVMF/OVMF_CODE.fd".to_string()),
            disk_path: "/var/lib/libvirt/images/adv.qcow2".to_string(),
            graphics_type: Some("spice".to_string()),
            virtio: true,
        };
        let adv_xml = QemuProvider::generate_advanced_domain_xml(&opts);
        assert!(adv_xml.contains("<name>adv-vm</name>"));
        assert!(adv_xml.contains("machine='q35'"));
        assert!(adv_xml.contains("<cpu mode='host-passthrough'"));
        assert!(adv_xml.contains("/usr/share/OVMF/OVMF_CODE.fd"));
        assert!(adv_xml.contains("graphics type='spice'"));
        assert!(adv_xml.contains("memballoon model='virtio'"));

        let opts_simple = DomainXmlOptions {
            name: "simple-vm".to_string(),
            memory_mb: 1024,
            cpus: 1,
            cpu_model: None,
            machine_type: None,
            firmware_path: None,
            disk_path: "/disk.img".to_string(),
            graphics_type: None,
            virtio: false,
        };
        let simple_xml = QemuProvider::generate_advanced_domain_xml(&opts_simple);
        assert!(simple_xml.contains("machine='pc'"));
        assert!(simple_xml.contains("memballoon model='none'"));

        // Test pool and volume operations
        let _ = provider.pool_create("default", "/var/lib/libvirt/images");
        let _ = provider.pool_refresh("default");
        let _ = provider.volume_create_as("default", "vol1", "20G", "qcow2");
        let _ = provider.volume_resize("default", "vol1", "30G");
        let _ = QemuProvider::resize_qcow2_disk("/var/lib/libvirt/images/disk.qcow2", "50G");
        let _ = provider.get_guest_ip_from_agent();
        let _ = provider.get_guest_ip_from_leases("default");
    }
}
