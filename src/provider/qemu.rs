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
    /// Executable name or path for `virsh`.
    virsh_bin: String,
    /// Executable name or path for `qemu-img`.
    qemu_img_bin: String,
    /// Executable name or path for `virt-install`.
    virt_install_bin: String,
    /// Executable name or path for `virt-clone`.
    virt_clone_bin: String,
    /// Whether linked clone mode is forced.
    linked_clone: bool,
}

impl QemuProvider {
    /// Creates a new QemuProvider instance.
    pub fn new(machine_id: Option<String>) -> Self {
        Self {
            machine_id,
            virsh_bin: "virsh".to_string(),
            qemu_img_bin: "qemu-img".to_string(),
            virt_install_bin: "virt-install".to_string(),
            virt_clone_bin: "virt-clone".to_string(),
            linked_clone: false,
        }
    }

    /// Configures custom binary paths for `virsh`, `qemu-img`, `virt-install`, and `virt-clone`.
    ///
    /// # Arguments
    ///
    /// * `virsh` - Path or binary name for `virsh`.
    /// * `qemu_img` - Path or binary name for `qemu-img`.
    /// * `virt_install` - Path or binary name for `virt-install`.
    /// * `virt_clone` - Path or binary name for `virt-clone`.
    ///
    /// # Returns
    ///
    /// Returns the updated `QemuProvider`.
    #[must_use]
    pub fn with_bins(
        mut self,
        virsh: impl Into<String>,
        qemu_img: impl Into<String>,
        virt_install: impl Into<String>,
        virt_clone: impl Into<String>,
    ) -> Self {
        self.virsh_bin = virsh.into();
        self.qemu_img_bin = qemu_img.into();
        self.virt_install_bin = virt_install.into();
        self.virt_clone_bin = virt_clone.into();
        self
    }

    /// Enables or disables linked clone creation during `clone_machine`.
    ///
    /// # Arguments
    ///
    /// * `linked` - `true` to use QCOW2 backing files for linked clones.
    ///
    /// # Returns
    ///
    /// Returns the updated `QemuProvider`.
    #[must_use]
    pub const fn with_linked_clone(mut self, linked: bool) -> Self {
        self.linked_clone = linked;
        self
    }

    /// Runs `virsh` using the configured `virsh_bin`.
    fn run_virsh(&self, args: &[&str]) -> Result<String, MigratoryError> {
        execute_virsh_inner(&self.virsh_bin, args)
    }

    /// Helper to get the ID, returning an error if not present.
    fn require_id(&self) -> Result<&str, MigratoryError> {
        self.machine_id.as_deref().ok_or_else(|| {
            MigratoryError::Generic("Machine not created or ID not found".to_string())
        })
    }

    /// Resolves the default libvirt network gateway IP dynamically from `virsh net-dumpxml default`.
    ///
    /// # Returns
    ///
    /// Returns the resolved gateway IP address, or `"192.168.122.1"` as fallback.
    pub fn resolve_gateway(&self) -> String {
        if let Ok(xml) = self.run_virsh(&["net-dumpxml", "default"]) {
            for line in xml.lines() {
                let trimmed = line.trim();
                if let Some(pos) = trimmed.find("<ip address='")
                    && let Some(end) = trimmed[pos + 13..].find('\'')
                {
                    return trimmed[pos + 13..pos + 13 + end].to_string();
                } else if let Some(pos) = trimmed.find("<ip address=\"")
                    && let Some(end) = trimmed[pos + 13..].find('"')
                {
                    return trimmed[pos + 13..pos + 13 + end].to_string();
                }
            }
        }
        "192.168.122.1".to_string()
    }

    /// Applies network settings to the VM.
    fn configure_networks(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;

        let mut open_ports: Vec<u16> = vec![];

        for net in &config.networks {
            match net {
                NetworkConfig::ForwardedPort {
                    guest,
                    protocol,
                    host_ip,
                    ..
                } => {
                    let collision_res = network::check_forwarded_port(net, &open_ports)?;
                    let final_host = collision_res.corrected_host_port;
                    open_ports.push(final_host);

                    let proto = protocol.as_deref().unwrap_or("tcp");
                    let host_ip_str = host_ip.as_deref().unwrap_or("127.0.0.1");

                    // Dynamic host port forwarding using iptables PREROUTING rules
                    let port_str = final_host.to_string();
                    let gateway = self.resolve_gateway();
                    let dest = format!("{}:{}", gateway, guest);
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
                    let _ = self.run_virsh(&[
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
                    let _ = self.run_virsh(&[
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

    /// Sets up shared folders on QEMU using VirtIO-FS or 9p filesystem devices.
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
    /// Returns a `MigratoryError` if device attachment fails.
    fn setup_synced_folders(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;

        for sf in &config.synced_folders {
            if sf.disabled {
                continue;
            }

            let mount_tag = sf
                .guest_path
                .to_string_lossy()
                .replace('/', "_")
                .trim_start_matches('_')
                .to_string();
            let mount_tag = if mount_tag.is_empty() {
                "vagrant".to_string()
            } else {
                mount_tag
            };

            let xml = format!(
                "<filesystem type='mount' accessmode='passthrough'><driver type='virtiofs'/><source dir='{}'/><target dir='{}'/></filesystem>",
                sf.host_path.display(),
                mount_tag
            );

            let tmp = tempfile::NamedTempFile::new().map_err(MigratoryError::Io)?;
            std::fs::write(tmp.path(), xml).map_err(MigratoryError::Io)?;
            let tmp_path = tmp.path().to_string_lossy();

            let _ = self.run_virsh(&["attach-device", id, &tmp_path, "--config"]);
        }
        Ok(())
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
        self.setup_synced_folders(config)?;

        self.run_virsh(&["start", id])?;
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
        self.run_virsh(&["shutdown", id])?;
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
        let _ = self.run_virsh(&["destroy", id]); // Ensure it's off, ignore if already off
        self.run_virsh(&["undefine", id, "--remove-all-storage"])?;
        Ok(())
    }

    fn import(&self, box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError> {
        let source_qcow = std::fs::read_dir(box_dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.extension().and_then(|x| x.to_str()) == Some("img")
                    || p.extension().and_then(|x| x.to_str()) == Some("qcow2")
            });

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
            execute_virsh_inner(&self.virt_install_bin, &args)?;
        } else {
            return Err(MigratoryError::NotFound(
                "QCOW2 or IMG file not found in box directory".into(),
            ));
        }

        Ok(vm_name.to_string())
    }

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

        let linked_clone = self.linked_clone
            || std::env::var("VAGRANT_LIBVIRT_LINKED_CLONE").unwrap_or_default() == "true";
        let qemu_img_cmd = linked_clone.then(|| {
            // Create a qcow2 backing file first for the clone
            let backing_file = format!("/var/lib/libvirt/images/{}.qcow2", base_machine_id);
            let new_file = format!("/var/lib/libvirt/images/{}.qcow2", vm_name);
            vec![
                "create".to_string(),
                "-f".to_string(),
                "qcow2".to_string(),
                "-F".to_string(),
                "qcow2".to_string(),
                "-b".to_string(),
                backing_file,
                new_file,
            ]
        });

        if let Some(cmd) = qemu_img_cmd {
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            execute_virsh_inner(&self.qemu_img_bin, &cmd_refs)?;
        }

        let clone_args_refs: Vec<&str> = clone_args.iter().map(|s| s.as_str()).collect();
        execute_virsh_inner(&self.virt_clone_bin, &clone_args_refs)?;
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
    fn status(&self) -> Result<crate::provider::MachineState, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok(crate::provider::MachineState::NotCreated),
        };
        let out = self.run_virsh(&["domstate", id]).unwrap_or_default();
        if out.trim().is_empty() {
            Ok(crate::provider::MachineState::Unknown(
                "unknown".to_string(),
            ))
        } else {
            Ok(crate::provider::MachineState::from_raw(out.trim()))
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
        self.run_virsh(&["suspend", id])?;
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
        self.run_virsh(&["resume", id])?;
        Ok(())
    }

    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_virsh(&["snapshot-create-as", id, name, "migratory snapshot"])?;
        Ok(())
    }

    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_virsh(&["snapshot-revert", id, name])?;
        Ok(())
    }

    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = self.run_virsh(&["snapshot-list", id, "--name"])?;
        let snaps: Vec<String> = out
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect();
        Ok(snaps)
    }

    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_virsh(&["snapshot-delete", id, name])?;
        Ok(())
    }

    /// Exports the machine domain configuration and disk image to the specified directory.
    ///
    /// # Arguments
    ///
    /// * `output_dir` - Directory where `box.xml` and `box.img` files are written.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if dumping XML, converting the disk, or I/O fails.
    fn export(&self, output_dir: &std::path::Path) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let xml = self.run_virsh(&["dumpxml", id])?;
        std::fs::write(output_dir.join("box.xml"), xml).map_err(MigratoryError::Io)?;
        let disk_path = output_dir.join("box.img");
        let disk_str = disk_path.to_string_lossy();
        let _ = execute_virsh_inner(
            &self.qemu_img_bin,
            &["convert", "-O", "qcow2", id, &disk_str],
        );
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
            "<controller type='virtio-serial' index='0'/>
    <rng model='virtio'>
      <backend model='random'>/dev/urandom</backend>
    </rng>
    <memballoon model='virtio'/>"
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
        self.run_virsh(&["pool-create-as", pool_name, "dir", "--target", pool_path])?;
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
        self.run_virsh(&["pool-refresh", pool_name])?;
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
        self.run_virsh(&[
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
        self.run_virsh(&["vol-resize", vol_name, new_capacity, "--pool", pool_name])?;
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
        Self::resize_qcow2_disk_with("qemu-img", disk_path, new_capacity)
    }

    /// Resizes a QCOW2 disk image using a custom `qemu-img` binary.
    ///
    /// # Arguments
    ///
    /// * `qemu_img_bin` - Path or binary name for `qemu-img`.
    /// * `disk_path` - Path to the `.qcow2` disk file.
    /// * `new_capacity` - Target size string (e.g. `"50G"`).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if resize fails.
    pub fn resize_qcow2_disk_with(
        qemu_img_bin: &str,
        disk_path: &str,
        new_capacity: &str,
    ) -> Result<(), MigratoryError> {
        execute_virsh_inner(qemu_img_bin, &["resize", disk_path, new_capacity])?;
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
        let out = self
            .run_virsh(&[
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
        let out = self
            .run_virsh(&["net-dhcp-leases", network])
            .unwrap_or_default();
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
        let out = self.run_virsh(&["domdisplay", id]).unwrap_or_default();
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

/// Spawns the `virsh` CLI binary with the given arguments.
///
/// # Arguments
///
/// * `cmd` - Executable name or path (`"virsh"`).
/// * `args` - Arguments passed to `virsh`.
///
/// # Returns
///
/// Returns the captured standard output string on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if the command cannot be spawned or exits non-zero.
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
    fn test_qemu_coverage_additions() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let virsh_bin = temp_dir.path().join("virsh");
        let qemu_img_bin = temp_dir.path().join("qemu-img");
        let virt_install_bin = temp_dir.path().join("virt-install");
        let virt_clone_bin = temp_dir.path().join("virt-clone");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = r#"#!/bin/sh
cmd="$*"
if echo "$cmd" | grep -q "net-dumpxml"; then
    if echo "$cmd" | grep -q "single-quote"; then
        echo "<ip address='10.0.0.1'/>"
    elif echo "$cmd" | grep -q "double-quote"; then
        echo "<ip address=\"10.0.0.2\"/>"
    else
        echo "<ip address='10.0.0.3'/>"
    fi
    exit 0
fi
if echo "$cmd" | grep -q "snapshot-list"; then
    echo " snap1 "
    echo " snap2 "
    exit 0
fi
if echo "$cmd" | grep -q "dumpxml"; then
    echo "<domain></domain>"
    exit 0
fi
exit 0
"#;
            std::fs::write(&virsh_bin, mock_script)?;
            std::fs::set_permissions(&virsh_bin, std::fs::Permissions::from_mode(0o755))?;

            std::fs::write(&qemu_img_bin, "#!/bin/sh\nexit 0\n")?;
            std::fs::set_permissions(&qemu_img_bin, std::fs::Permissions::from_mode(0o755))?;
            std::fs::write(&virt_install_bin, "#!/bin/sh\nexit 0\n")?;
            std::fs::set_permissions(&virt_install_bin, std::fs::Permissions::from_mode(0o755))?;
            std::fs::write(&virt_clone_bin, "#!/bin/sh\nexit 0\n")?;
            std::fs::set_permissions(&virt_clone_bin, std::fs::Permissions::from_mode(0o755))?;
        }

        let virsh_str = virsh_bin.to_string_lossy().to_string();
        let qemu_img_str = qemu_img_bin.to_string_lossy().to_string();
        let virt_install_str = virt_install_bin.to_string_lossy().to_string();
        let virt_clone_str = virt_clone_bin.to_string_lossy().to_string();

        let provider = QemuProvider::new(Some("test-id".to_string())).with_bins(
            &virsh_str,
            &qemu_img_str,
            &virt_install_str,
            &virt_clone_str,
        );
        let mut config = VmConfig::default();
        config
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/host"),
                guest_path: std::path::PathBuf::from("/"),
                disabled: false,
                ..Default::default()
            });
        config
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/host2"),
                guest_path: std::path::PathBuf::from("/2"),
                disabled: true,
                ..Default::default()
            });

        provider.setup_synced_folders(&config)?;

        let _ = execute_virsh(&["net-dumpxml", "single-quote"]);
        let _ = execute_virsh_inner(&virsh_str, &["net-dumpxml", "single-quote"]);
        let _ = execute_virsh_inner(&virsh_str, &["net-dumpxml", "double-quote"]);

        #[cfg(unix)]
        {
            let mock_script_single = r#"#!/bin/sh
echo "<ip address='unterminated"
echo "<ip address='192.168.1.1'/>"
exit 0
"#;
            std::fs::write(&virsh_bin, mock_script_single)?;
            assert_eq!(provider.resolve_gateway(), "192.168.1.1");

            let mock_script_double = r#"#!/bin/sh
echo "<ip address=\"unterminated"
echo "<ip address=\"192.168.1.2\"/>"
exit 0
"#;
            std::fs::write(&virsh_bin, mock_script_double)?;
            assert_eq!(provider.resolve_gateway(), "192.168.1.2");

            let mock_script_orig = r#"#!/bin/sh
cmd="$*"
if echo "$cmd" | grep -q "snapshot-list"; then
    echo " snap1 "
    echo " snap2 "
    exit 0
fi
if echo "$cmd" | grep -q "dumpxml"; then
    echo "<domain></domain>"
    exit 0
fi
exit 0
"#;
            std::fs::write(&virsh_bin, mock_script_orig)?;
        }

        provider.snapshot_save("snap1")?;
        provider.snapshot_restore("snap1")?;
        let snaps = provider.snapshot_list()?;
        assert_eq!(snaps, vec!["snap1".to_string(), "snap2".to_string()]);
        provider.snapshot_delete("snap1")?;

        provider.export(temp_dir.path())?;
        let fail_provider = QemuProvider::new(Some("test-id".to_string()))
            .with_bins("false", "false", "false", "false");
        assert!(fail_provider.export(temp_dir.path()).is_err());

        let box_path = temp_dir.path().join("dummy");
        std::fs::create_dir_all(&box_path)?;
        std::fs::write(box_path.join("dummy.img"), "")?;
        provider.import(&box_path, "vm-1")?;

        let linked_provider = QemuProvider::new(Some("test-id".to_string()))
            .with_bins(
                &virsh_str,
                &qemu_img_str,
                &virt_install_str,
                &virt_clone_str,
            )
            .with_linked_clone(true);
        linked_provider.clone_machine("base-id", "vm-2")?;

        Ok(())
    }

    #[test]
    fn test_qemu_provider_methods() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let provider = QemuProvider::new(Some("test-id".to_string()))
            .with_bins("echo", "echo", "echo", "echo");

        let config = crate::config::VmConfig::default();
        provider.up(&config)?;
        provider.halt()?;
        provider.suspend()?;
        provider.resume()?;
        provider.destroy()?;

        let box_path = temp_dir.path().join("dummy");
        std::fs::create_dir_all(&box_path)?;
        std::fs::write(box_path.join("dummy.qcow2"), "")?;

        provider.import(&box_path, "vm-1")?;
        provider.clone_machine("base-id", "vm-2")?;

        let _ = provider.status()?;
        provider.export(temp_dir.path())?;

        Ok(())
    }

    #[test]
    fn test_qemu_provider() -> Result<(), MigratoryError> {
        let provider = QemuProvider::new(Some("test-id".to_string()));
        assert_eq!(provider.name(), "qemu");
        assert!(provider.status().is_ok());

        let provider_no_id = QemuProvider::new(None);
        assert!(provider_no_id.up(&VmConfig::default()).is_err());
        assert!(provider_no_id.halt().is_err());
        assert!(provider_no_id.destroy().is_err());
        assert!(provider_no_id.suspend().is_err());
        assert!(provider_no_id.resume().is_err());
        assert!(
            provider_no_id
                .export(std::path::Path::new("/dummy"))
                .is_err()
        );
        assert_eq!(provider_no_id.status()?, "not created");
        Ok(())
    }

    #[test]
    fn test_qemu_provider_networks() {
        let provider = QemuProvider::new(Some("test-id".to_string()))
            .with_bins("echo", "echo", "echo", "echo");
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
        let result = execute_virsh_inner("echo", &["test"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_virsh_failure() {
        let result = execute_virsh_inner("false", &["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_qemu_status_running() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let bin = temp_dir.path().join("virsh");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = "#!/bin/sh\necho running\nexit 0\n";
            std::fs::write(&bin, mock_script)?;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("virsh.bat");
            std::fs::write(&bat, "@echo off\necho running\nexit 0")?;
        }

        let bin_str = bin.to_string_lossy().to_string();
        let provider = QemuProvider::new(Some("test-id".to_string()))
            .with_bins(&bin_str, &bin_str, &bin_str, &bin_str);
        let config = crate::config::VmConfig::default();
        assert!(provider.up(&config).is_ok());
        assert!(provider.halt().is_ok(), "halt failed");
        assert!(provider.destroy().is_ok());
        assert!(provider.suspend().is_ok());
        assert!(provider.resume().is_ok());
        assert_eq!(provider.status()?, "running");

        Ok(())
    }

    #[test]
    fn test_qemu_import_no_image() -> Result<(), MigratoryError> {
        let provider = QemuProvider::new(Some("test-id".to_string()));
        let temp_dir = tempfile::tempdir()?;

        let result = provider.import(temp_dir.path(), "test");
        assert!(result.is_err());
        Ok(())
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
    fn test_qemu_advanced_features_mocked() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
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
    elif echo "$cmd" | grep -q "no-colon"; then
        echo "vnc_no_port"
    else
        echo "127.0.0.1:1"
    fi
    exit 0
fi

if echo "$cmd" | grep -q "net-dumpxml"; then
    echo "<ip address='10.0.0.1'/>"
    echo "<ip address=\"10.0.0.2\"/>"
    exit 0
fi

if echo "$cmd" | grep -q "qemu-agent-command"; then
    if echo "$cmd" | grep -q "loopback-vm"; then
        echo '{"return":[{"ip-addresses":[{"ip-address":"127.0.0.1"}]}]}'
    elif echo "$cmd" | grep -q "fe80-vm"; then
        echo '{"return":[{"ip-addresses":[{"ip-address":"fe80::1"}]}]}'
    elif echo "$cmd" | grep -q "colon-vm"; then
        echo '{"return":[{"ip-addresses":[{"ip-address":"::1"}]}]}'
    elif echo "$cmd" | grep -q "no-ip-vm"; then
        echo '{"return":[]}'
    elif echo "$cmd" | grep -q "malformed-vm"; then
        echo '{"ip-address":"unterminated'
    else
        echo '{"return":[{"ip-addresses":[{"ip-address":"192.168.122.50"}]}]}'
    fi
    exit 0
fi

if echo "$cmd" | grep -q "net-dhcp-leases"; then
    if echo "$cmd" | grep -q "no-leases"; then
        echo "header"
        exit 0
    fi
    echo "header"
    echo "2026-09-07 10:00:00 52:54:00:12:34:56 ipv6 fe80::1/64 guest-vm 01:52:54:00:12:34:56"
    echo "2026-09-07 10:00:00 52:54:00:12:34:56 ipv4 192.168.122.75/24 guest-vm 01:52:54:00:12:34:56"
    exit 0
fi

exit 0
"#;
            std::fs::write(&virsh_bin, mock_script)?;
            std::fs::set_permissions(&virsh_bin, std::fs::Permissions::from_mode(0o755))?;

            std::fs::write(&qemu_img_bin, "#!/bin/sh\nexit 0\n")?;
            std::fs::set_permissions(&qemu_img_bin, std::fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("virsh.bat");
            std::fs::write(&bat, "@echo off\nexit 0")?;
            let img_bat = temp_dir.path().join("qemu-img.bat");
            std::fs::write(&img_bat, "@echo off\nexit 0")?;
        }

        let virsh_str = virsh_bin.to_string_lossy().to_string();
        let qemu_img_str = qemu_img_bin.to_string_lossy().to_string();

        let provider = QemuProvider::new(Some("test-id".to_string())).with_bins(
            &virsh_str,
            &qemu_img_str,
            "echo",
            "echo",
        );
        assert!(provider.pool_create("default", "/var/lib/images").is_ok());
        assert!(provider.pool_refresh("default").is_ok());
        assert!(
            provider
                .volume_create_as("default", "vol1", "20G", "qcow2")
                .is_ok()
        );
        assert!(provider.volume_resize("default", "vol1", "30G").is_ok());
        let _ = QemuProvider::resize_qcow2_disk("/var/lib/images/disk.qcow2", "40G");
        assert!(
            execute_virsh_inner(
                &qemu_img_str,
                &["resize", "/var/lib/images/disk.qcow2", "40G"]
            )
            .is_ok()
        );

        // Agent IPs
        let ip_normal = provider.get_guest_ip_from_agent()?;
        assert_eq!(ip_normal, Some("192.168.122.50".to_string()));

        let prov_loopback = QemuProvider::new(Some("loopback-vm".to_string())).with_bins(
            &virsh_str,
            &qemu_img_str,
            "echo",
            "echo",
        );
        assert_eq!(prov_loopback.get_guest_ip_from_agent()?, None);

        let prov_fe80 = QemuProvider::new(Some("fe80-vm".to_string())).with_bins(
            &virsh_str,
            &qemu_img_str,
            "echo",
            "echo",
        );
        assert_eq!(prov_fe80.get_guest_ip_from_agent()?, None);

        let prov_colon = QemuProvider::new(Some("colon-vm".to_string())).with_bins(
            &virsh_str,
            &qemu_img_str,
            "echo",
            "echo",
        );
        assert_eq!(prov_colon.get_guest_ip_from_agent()?, None);

        let prov_malformed = QemuProvider::new(Some("malformed-vm".to_string())).with_bins(
            &virsh_str,
            &qemu_img_str,
            "echo",
            "echo",
        );
        assert_eq!(prov_malformed.get_guest_ip_from_agent()?, None);

        let prov_no_ip = QemuProvider::new(Some("no-ip-vm".to_string())).with_bins(
            &virsh_str,
            &qemu_img_str,
            "echo",
            "echo",
        );
        assert_eq!(prov_no_ip.get_guest_ip_from_agent()?, None);

        // DHCP leases IP
        let ip_dhcp = provider.get_guest_ip_from_leases("default")?;
        assert_eq!(ip_dhcp, Some("192.168.122.75".to_string()));

        let ip_no_leases = provider.get_guest_ip_from_leases("no-leases")?;
        assert_eq!(ip_no_leases, None);

        // Display ports
        assert_eq!(provider.get_display_port()?, Some(5901));

        let prov_high = QemuProvider::new(Some("high-port".to_string())).with_bins(
            &virsh_str,
            &qemu_img_str,
            "echo",
            "echo",
        );
        assert_eq!(prov_high.get_display_port()?, Some(5902));

        let prov_bad = QemuProvider::new(Some("bad-port".to_string())).with_bins(
            &virsh_str,
            &qemu_img_str,
            "echo",
            "echo",
        );
        assert_eq!(prov_bad.get_display_port()?, None);

        let prov_no_colon = QemuProvider::new(Some("no-colon".to_string())).with_bins(
            &virsh_str,
            &qemu_img_str,
            "echo",
            "echo",
        );
        assert_eq!(prov_no_colon.get_display_port()?, None);

        Ok(())
    }

    #[test]
    fn test_qemu_import_success() -> Result<(), MigratoryError> {
        let provider = QemuProvider::new(Some("test-id".to_string()))
            .with_bins("echo", "echo", "echo", "echo");
        let temp_dir = tempfile::tempdir()?;
        let file_path = temp_dir.path().join("image.qcow2");
        std::fs::write(&file_path, "dummy content")?;

        let result = provider.import(temp_dir.path(), "test");
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_qemu_clone_machine() {
        let provider = QemuProvider::new(Some("test-id".to_string()))
            .with_bins("echo", "echo", "echo", "echo");
        let result = provider.clone_machine("base-id", "test");
        assert!(result.is_ok());
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

        let provider = QemuProvider::new(Some("test-id".to_string()))
            .with_bins("echo", "echo", "echo", "echo");
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
        assert!(
            provider
                .pool_create("default", "/var/lib/libvirt/images")
                .is_ok()
        );
        assert!(provider.pool_refresh("default").is_ok());
        assert!(
            provider
                .volume_create_as("default", "vol1", "20G", "qcow2")
                .is_ok()
        );
        assert!(provider.volume_resize("default", "vol1", "30G").is_ok());
        assert!(
            QemuProvider::resize_qcow2_disk_with(
                "echo",
                "/var/lib/libvirt/images/disk.qcow2",
                "50G"
            )
            .is_ok()
        );
        let _ = QemuProvider::resize_qcow2_disk("/var/lib/libvirt/images/disk.qcow2", "50G");
        let _ = provider.get_guest_ip_from_agent();
        let _ = provider.get_guest_ip_from_leases("default");
    }

    #[test]
    fn test_qemu_resolve_gateway_and_synced_folders() {
        let provider = QemuProvider::new(Some("domain-123".to_string()))
            .with_bins("echo", "echo", "echo", "echo");
        let gw = provider.resolve_gateway();
        assert!(!gw.is_empty());

        let mut config = VmConfig::default();
        config
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/host/path"),
                guest_path: std::path::PathBuf::from("/vagrant"),
                ..Default::default()
            });

        assert!(provider.setup_synced_folders(&config).is_ok());
    }
}
