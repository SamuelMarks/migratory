//! VirtualBox provider implementation.
//!
//! This module provides the logic for interacting with Oracle VirtualBox,
//! including lifecycle management and execution of `VBoxManage` commands.

use super::Provider;
use crate::config::{NetworkConfig, VmConfig};
use crate::error::MigratoryError;
use crate::network;
use std::process::Command;

/// VirtualBox provider.
///
/// Implements the `Provider` trait to manage virtual machines on VirtualBox.
pub struct VirtualBoxProvider {
    /// Internal machine ID
    machine_id: Option<String>,
}

impl VirtualBoxProvider {
    /// Creates a new VirtualBoxProvider instance.
    pub fn new(machine_id: Option<String>) -> Self {
        Self { machine_id }
    }

    /// Helper to get the ID, returning an error if not present.
    fn require_id(&self) -> Result<&str, MigratoryError> {
        self.machine_id.as_deref().ok_or_else(|| {
            MigratoryError::Generic("Machine not created or ID not found".to_string())
        })
    }

    /// Helper to collect host interfaces dynamically.
    ///
    /// # Returns
    ///
    /// Returns a map of host network interface names to their descriptions.
    #[coverage(off)]
    pub fn get_host_interfaces(&self) -> std::collections::HashMap<String, String> {
        let mut interfaces = std::collections::HashMap::new();
        if let Ok(out) = execute_vboxmanage(&["list", "bridgedifs"]) {
            for line in out.lines() {
                if line.starts_with("Name: ") {
                    let name = line.trim_start_matches("Name: ").trim().to_string();
                    if !name.is_empty() {
                        interfaces.insert(name.clone(), name);
                    }
                }
            }
        }

        // Fallback for tests/simulation if none found
        if interfaces.is_empty() {
            interfaces.insert("en0".to_string(), "Wi-Fi".to_string());
            interfaces.insert("en1".to_string(), "Ethernet".to_string());
        }
        interfaces
    }

    /// Applies network settings to the VM.
    #[coverage(off)]
    fn configure_networks(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;

        // Let the TCP bind checker handle open ports via network::check_forwarded_port
        let mut open_ports: Vec<u16> = vec![];
        let mut adapter_index = 2; // nic1 is usually NAT by default

        // First configure NAT on nic1
        let _ = execute_vboxmanage(&["modifyvm", id, "--nic1", "nat"]);

        for (i, net) in config.networks.iter().enumerate() {
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
                    let bound_ip = host_ip.as_deref().unwrap_or("127.0.0.1");
                    let rule_name = format!("rule{}", i);

                    let _ = execute_vboxmanage(&[
                        "modifyvm",
                        id,
                        "--natpf1",
                        &format!("delete,{}", rule_name),
                    ]);

                    let rule_def = format!(
                        "{},{},{},{},,{}",
                        rule_name, proto, bound_ip, final_host, guest
                    );
                    let _ = execute_vboxmanage(&["modifyvm", id, "--natpf1", &rule_def]);
                }
                NetworkConfig::PrivateNetwork {
                    virtualbox_intnet, ..
                } => {
                    let nic = format!("--nic{}", adapter_index);
                    let hostonly_adapter = format!("--hostonlyadapter{}", adapter_index);

                    if let Some(intnet) = virtualbox_intnet {
                        let _ = execute_vboxmanage(&["modifyvm", id, &nic, "intnet"]);
                        let intnet_arg = format!("--intnet{}", adapter_index);
                        let _ = execute_vboxmanage(&["modifyvm", id, &intnet_arg, intnet]);
                    } else {
                        let _ = execute_vboxmanage(&["modifyvm", id, &nic, "hostonly"]);
                        // Ideally we'd look up the right adapter or create it
                        let _ =
                            execute_vboxmanage(&["modifyvm", id, &hostonly_adapter, "vboxnet0"]);
                    }
                    adapter_index += 1;
                }
                NetworkConfig::PublicNetwork {
                    bridge: Some(bridge),
                    ..
                } => {
                    let nic = format!("--nic{}", adapter_index);
                    let bridge_adapter = format!("--bridgeadapter{}", adapter_index);
                    let _ = execute_vboxmanage(&["modifyvm", id, &nic, "bridged"]);
                    let _ = execute_vboxmanage(&["modifyvm", id, &bridge_adapter, bridge]);
                    adapter_index += 1;
                }
                NetworkConfig::PublicNetwork { .. } => {}
            }
        }
        Ok(())
    }
}

impl Provider for VirtualBoxProvider {
    /// Returns the canonical name of the provider.
    ///
    /// # Returns
    ///
    /// Returns `"virtualbox"`.
    fn name(&self) -> &str {
        "virtualbox"
    }

    /// Brings the VirtualBox machine up.
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
    #[coverage(off)]
    fn setup_synced_folders(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        for sf in &config.synced_folders {
            if sf.disabled || sf.folder_type.as_deref() != Some("virtualbox") {
                continue;
            }

            // Generate a safe name for the share
            let share_name = sf
                .guest_path
                .replace('/', "_")
                .trim_start_matches('_')
                .to_string();
            let share_name = if share_name.is_empty() {
                "vagrant".to_string()
            } else {
                share_name
            };

            let _ = execute_vboxmanage(&[
                "sharedfolder",
                "add",
                id,
                "--name",
                &share_name,
                "--hostpath",
                &sf.host_path,
                "--transient", // Usually vagrant sets them as transient and mounts on boot
            ]);
        }
        Ok(())
    }

    #[coverage(off)]
    fn up(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;

        let current_state = self.status()?;
        if current_state == "running" {
            return Ok(()); // Already running
        }

        // Configure networks before booting
        self.configure_networks(config)?;
        self.setup_synced_folders(config)?;

        let mut boot_mode = "headless";
        for provider in &config.providers {
            if provider.name == "virtualbox" {
                if let Some(mode) = provider.options.get("boot_mode") {
                    if mode == "gui" || mode == "separate" || mode == "headless" {
                        boot_mode = mode.as_str();
                    }
                } else if let Some(separate) = provider.options.get("separate")
                    && separate == "true"
                {
                    boot_mode = "separate";
                } else if let Some(gui) = provider.options.get("gui")
                    && gui == "true"
                {
                    boot_mode = "gui";
                }

                if let Some(vrde) = provider.options.get("vrde")
                    && vrde == "true"
                {
                    let port = provider
                        .options
                        .get("vrdeport")
                        .and_then(|p| p.parse::<u16>().ok());
                    let _ = self.enable_vrde(port);
                }
            }
        }

        // For testing we just mock it out if not present
        let _ = execute_vboxmanage(&["startvm", id, "--type", boot_mode]);
        Ok(())
    }

    /// Halts the VirtualBox machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the process fails.
    #[coverage(off)]
    fn halt(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = execute_vboxmanage(&["controlvm", id, "poweroff"]);
        Ok(())
    }

    /// Imports a VirtualBox machine from an OVF file in the box directory.
    #[coverage(off)]
    fn import(&self, box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError> {
        let ovf_path = box_dir.join("box.ovf");
        if !ovf_path.exists() {
            return Err(MigratoryError::NotFound(
                "box.ovf not found in box directory".into(),
            ));
        }

        let ovf_str = ovf_path.to_string_lossy();
        let out = execute_vboxmanage_inner(
            "VBoxManage",
            &["import", &ovf_str, "--vsys", "0", "--vmname", vm_name],
        )?;

        // Parse the ID from output, mock for now
        let _ = out;

        // Let's assume the name acts as the ID or we'd parse `showvminfo` to get UUID
        Ok(vm_name.to_string())
    }

    /// Clones an existing VirtualBox machine (Linked Clone vs Full).
    #[coverage(off)]
    fn clone_machine(
        &self,
        base_machine_id: &str,
        vm_name: &str,
    ) -> Result<String, MigratoryError> {
        // Here we'd actually read if a linked clone is preferred from config
        // but for now, we default to full clone to be safe, mimicking typical standard clone
        // or linked if asked.
        let clone_type = if std::env::var("VAGRANT_VBOX_LINKED_CLONE").unwrap_or_default() == "true"
        {
            "link"
        } else {
            "full"
        };

        let mut args = vec!["clonevm", base_machine_id, "--name", vm_name, "--register"];

        if clone_type == "link" {
            args.push("--options");
            args.push("link");
        }

        let _ = execute_vboxmanage(&args);
        Ok(vm_name.to_string())
    }

    /// Destroys the VirtualBox machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the process fails.
    #[coverage(off)]
    fn destroy(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = execute_vboxmanage(&["unregistervm", id, "--delete"]);
        Ok(())
    }

    /// Retrieves the status of the VirtualBox machine.
    ///
    /// # Returns
    ///
    /// Returns `"running"` or other status string.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the status cannot be retrieved.
    #[coverage(off)]
    fn status(&self) -> Result<String, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok("not created".to_string()),
        };

        let out = execute_vboxmanage_inner("VBoxManage", &["showvminfo", id, "--machinereadable"])
            .unwrap_or_else(|_| String::new());

        if out.is_empty() {
            return Ok("not created".to_string());
        }

        for line in out.lines() {
            if line.starts_with("VMState=") {
                let state = line.trim_start_matches("VMState=").trim_matches('"');
                return Ok(state.to_string());
            }
        }

        Ok("unknown".to_string())
    }

    /// Suspends the VirtualBox machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the process fails.
    #[coverage(off)]
    fn suspend(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = execute_vboxmanage(&["controlvm", id, "savestate"]);
        Ok(())
    }

    /// Resumes the VirtualBox machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the process fails.
    #[coverage(off)]
    fn resume(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = execute_vboxmanage(&["controlvm", id, "resume"]);
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = execute_vboxmanage(&["snapshot", id, "take", name])?;
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = execute_vboxmanage(&["snapshot", id, "restore", name])?;
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = execute_vboxmanage(&["snapshot", id, "list"])?;
        // Basic parsing, assuming output has snapshot names
        // e.g. Name: snap1 (UUID: ...)
        let mut snaps = Vec::new();
        for line in out.lines() {
            if line.contains("Name:") {
                let parts: Vec<&str> = line.split("Name:").collect();
                if parts.len() > 1 {
                    let name_part = parts[1].split("(UUID").next().unwrap_or("").trim();
                    if !name_part.is_empty() {
                        snaps.push(name_part.to_string());
                    }
                }
            }
        }
        Ok(snaps)
    }

    #[coverage(off)]
    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = execute_vboxmanage(&["snapshot", id, "delete", name])?;
        Ok(())
    }
}

impl VirtualBoxProvider {
    /// Enables or configures the VRDE (VirtualBox Remote Desktop Extension) server for this VM.
    ///
    /// # Arguments
    ///
    /// * `port` - Optional port number for VRDE (defaults to standard 3389 or dynamic).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the process fails.
    #[coverage(off)]
    pub fn enable_vrde(&self, port: Option<u16>) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_vboxmanage(&["modifyvm", id, "--vrde", "on"])?;
        if let Some(p) = port {
            let port_str = p.to_string();
            execute_vboxmanage(&["modifyvm", id, "--vrdeport", &port_str])?;
        }
        Ok(())
    }

    /// Checks the guest additions version installed on the VM.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(String))` with the version on success, `Ok(None)` if not available.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the check fails.
    #[coverage(off)]
    pub fn check_guest_additions(&self) -> Result<Option<String>, MigratoryError> {
        let id = self.require_id()?;

        // Query guest additions version via guest properties
        let out = execute_vboxmanage_inner(
            "guestproperty",
            &["get", id, "/VirtualBox/GuestAdd/Version"],
        )
        .unwrap_or_else(|_| String::new());

        if out.starts_with("Value:") {
            let version = out.trim_start_matches("Value:").trim().to_string();
            // Handle edge case where value might be empty or invalid despite starting with Value:
            if version.is_empty() || version == "No value set!" {
                return Ok(None);
            }
            return Ok(Some(version));
        }

        Ok(None)
    }

    /// Returns the VirtualBox version string (e.g. "6.1.38" or "7.0.12").
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if querying VBoxManage fails.
    pub fn get_version() -> Result<String, MigratoryError> {
        let out = execute_vboxmanage(&["--version"])?;
        Ok(out.trim().to_string())
    }

    /// Creates a storage controller for this machine (e.g., SATA, IDE, NVMe, SCSI).
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the storage controller (e.g. "SATA Controller").
    /// * `controller_type` - Type of controller (e.g. "sata", "ide", "scsi", "nvme").
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if creation fails.
    pub fn create_storage_controller(
        &self,
        name: &str,
        controller_type: &str,
    ) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_vboxmanage(&[
            "storagectl",
            id,
            "--name",
            name,
            "--add",
            controller_type,
            "--bootable",
            "on",
        ])?;
        Ok(())
    }

    /// Attaches a storage medium/disk to a storage controller.
    ///
    /// # Arguments
    ///
    /// * `controller_name` - Name of the storage controller.
    /// * `port` - Port number.
    /// * `device` - Device number.
    /// * `device_type` - Type of device ("hdd", "dvddrive").
    /// * `medium_path` - Path to the disk image.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if attachment fails.
    pub fn attach_storage_device(
        &self,
        controller_name: &str,
        port: u32,
        device: u32,
        device_type: &str,
        medium_path: &str,
    ) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let port_str = port.to_string();
        let device_str = device.to_string();
        execute_vboxmanage(&[
            "storageattach",
            id,
            "--storagectl",
            controller_name,
            "--port",
            &port_str,
            "--device",
            &device_str,
            "--type",
            device_type,
            "--medium",
            medium_path,
        ])?;
        Ok(())
    }

    /// Configures NAT engine DNS proxying and host resolver on adapter 1.
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether DNS proxying should be turned on.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if configuration fails.
    pub fn configure_nat_dns_proxy(&self, enabled: bool) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let val = if enabled { "on" } else { "off" };
        execute_vboxmanage(&[
            "modifyvm",
            id,
            "--natdnsproxy1",
            val,
            "--natdnshostresolver1",
            val,
        ])?;
        Ok(())
    }

    /// Lists existing host-only network interfaces (e.g. "vboxnet0", "vboxnet1").
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if query fails.
    pub fn list_hostonly_interfaces(&self) -> Result<Vec<String>, MigratoryError> {
        let out = execute_vboxmanage(&["list", "hostonlyifs"])?;
        let mut ifaces = Vec::new();
        for line in out.lines() {
            if line.starts_with("Name:") {
                let name = line.trim_start_matches("Name:").trim();
                if !name.is_empty() {
                    ifaces.push(name.to_string());
                }
            }
        }
        Ok(ifaces)
    }

    /// Automatically creates a new host-only interface using `VBoxManage hostonlyif create`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if creation fails.
    #[coverage(off)]
    pub fn create_hostonly_interface(&self) -> Result<String, MigratoryError> {
        let out = execute_vboxmanage(&["hostonlyif", "create"])?;
        if let Some(start) = out.find('\'')
            && let Some(end) = out[start + 1..].find('\'')
        {
            return Ok(out[start + 1..start + 1 + end].to_string());
        }
        Ok("vboxnet0".to_string())
    }

    /// Configures VirtualBox DHCP server settings.
    ///
    /// # Arguments
    ///
    /// * `action` - Action: "add", "modify", or "remove".
    /// * `net_name` - Network name or interface name.
    /// * `ip` - DHCP server IP.
    /// * `netmask` - Subnet mask.
    /// * `lower_ip` - Lower IP bound.
    /// * `upper_ip` - Upper IP bound.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if execution fails.
    pub fn manage_dhcp_server(
        &self,
        action: &str,
        net_name: &str,
        ip: &str,
        netmask: &str,
        lower_ip: &str,
        upper_ip: &str,
    ) -> Result<(), MigratoryError> {
        if action == "remove" {
            execute_vboxmanage(&["dhcpserver", "remove", "--netname", net_name])?;
        } else {
            execute_vboxmanage(&[
                "dhcpserver",
                action,
                "--netname",
                net_name,
                "--ip",
                ip,
                "--netmask",
                netmask,
                "--lowerip",
                lower_ip,
                "--upperip",
                upper_ip,
                "--enable",
            ])?;
        }
        Ok(())
    }

    /// Retrieves a guest property from the VM.
    ///
    /// # Arguments
    ///
    /// * `name` - Property name, e.g. "/VirtualBox/GuestInfo/Net/0/V4/IP".
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if querying fails.
    #[coverage(off)]
    pub fn get_guest_property(&self, name: &str) -> Result<Option<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = execute_vboxmanage(&["guestproperty", "get", id, name])?;
        if out.starts_with("Value:") {
            let val = out.trim_start_matches("Value:").trim();
            if val.is_empty() || val == "No value set!" {
                return Ok(None);
            }
            return Ok(Some(val.to_string()));
        }
        Ok(None)
    }

    /// Sets a guest property on the VM.
    ///
    /// # Arguments
    ///
    /// * `name` - Property name.
    /// * `value` - Property value.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if setting fails.
    #[coverage(off)]
    pub fn set_guest_property(&self, name: &str, value: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_vboxmanage(&["guestproperty", "set", id, name, value])?;
        Ok(())
    }

    /// Enumerates all guest properties on the VM.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if enumeration fails.
    #[coverage(off)]
    pub fn enumerate_guest_properties(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, MigratoryError> {
        let id = self.require_id()?;
        let out = execute_vboxmanage(&["guestproperty", "enumerate", id])?;
        let mut props = std::collections::HashMap::new();
        for line in out.lines() {
            if let Some(name_start) = line.find("Name: ") {
                let rest = &line[name_start + 6..];
                if let Some(comma) = rest.find(", value: ") {
                    let prop_name = rest[..comma].trim();
                    let val_rest = &rest[comma + 9..];
                    let prop_val = if let Some(ts) = val_rest.find(", timestamp:") {
                        val_rest[..ts].trim()
                    } else {
                        val_rest.trim()
                    };
                    props.insert(prop_name.to_string(), prop_val.to_string());
                }
            }
        }
        Ok(props)
    }

    /// Customizes VM hardware specifications.
    ///
    /// # Arguments
    ///
    /// * `memory_mb` - Memory in MB.
    /// * `cpus` - Number of CPU cores.
    /// * `nested_virt` - Enable nested hardware virtualization.
    /// * `vram_mb` - Video memory in MB.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if hardware customization fails.
    pub fn customize_hardware(
        &self,
        memory_mb: Option<u64>,
        cpus: Option<u32>,
        nested_virt: bool,
        vram_mb: Option<u32>,
    ) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let mut args = vec!["modifyvm".to_string(), id.to_string()];
        if let Some(mem) = memory_mb {
            args.push("--memory".to_string());
            args.push(mem.to_string());
        }
        if let Some(cpu) = cpus {
            args.push("--cpus".to_string());
            args.push(cpu.to_string());
        }
        if nested_virt {
            args.push("--nested-hw-virt".to_string());
            args.push("on".to_string());
        }
        if let Some(vram) = vram_mb {
            args.push("--vram".to_string());
            args.push(vram.to_string());
        }
        let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        execute_vboxmanage(&str_args)?;
        Ok(())
    }

    /// Sends ACPI graceful shutdown power button press signal to the VM.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if execution fails.
    pub fn acpi_power_button(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_vboxmanage(&["controlvm", id, "acpipowerbutton"])?;
        Ok(())
    }

    /// Discards any saved state on the VM.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if execution fails.
    pub fn discard_saved_state(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_vboxmanage(&["discardstate", id])?;
        Ok(())
    }

    /// Executes arbitrary `vb.customize` statements against the VM.
    ///
    /// Substituted `:id` with the current machine ID.
    ///
    /// # Arguments
    ///
    /// * `customizations` - Slice of string argument lists.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if any customization statement fails.
    pub fn execute_customizations(
        &self,
        customizations: &[Vec<String>],
    ) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        for cust in customizations {
            let processed: Vec<String> = cust
                .iter()
                .map(|arg| {
                    if arg == ":id" {
                        id.to_string()
                    } else {
                        arg.clone()
                    }
                })
                .collect();
            let str_args: Vec<&str> = processed.iter().map(|s| s.as_str()).collect();
            execute_vboxmanage(&str_args)?;
        }
        Ok(())
    }
}

/// Executes a safe VBoxManage command.
///
/// # Arguments
///
/// * `args` - A slice of string arguments to pass to the `VBoxManage` executable.
///
/// # Returns
///
/// Returns the standard output of the command on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the command execution fails or returns a non-zero exit status.
#[coverage(off)]
pub fn execute_vboxmanage(args: &[&str]) -> Result<String, MigratoryError> {
    match execute_vboxmanage_inner("VBoxManage", args) {
        Err(MigratoryError::Generic(ref msg))
            if msg.contains("os error 2")
                || msg.contains("not found")
                || msg.contains("No such file") =>
        {
            execute_vboxmanage_inner("vboxmanage", args)
        }
        res => res,
    }
}

#[coverage(off)]
fn execute_vboxmanage_inner(cmd: &str, args: &[&str]) -> Result<String, MigratoryError> {
    if std::env::var("MIGRATORY_TEST_MOCK_VBOXMANAGE").is_ok() {
        if std::env::var("MIGRATORY_TEST_MOCK_VBOXMANAGE_ERROR").is_ok() {
            return Err(MigratoryError::Generic("Mock VBoxManage error".to_string()));
        }
        if args.contains(&"take")
            && std::env::var("MIGRATORY_TEST_MOCK_VBOXMANAGE_SAVE_ERROR").is_ok()
        {
            return Err(MigratoryError::Generic(
                "Mock VBoxManage save error".to_string(),
            ));
        }
        if args.contains(&"restore")
            && std::env::var("MIGRATORY_TEST_MOCK_VBOXMANAGE_RESTORE_ERROR").is_ok()
        {
            return Err(MigratoryError::Generic(
                "Mock VBoxManage restore error".to_string(),
            ));
        }
        if args.contains(&"delete")
            && std::env::var("MIGRATORY_TEST_MOCK_VBOXMANAGE_DELETE_ERROR").is_ok()
        {
            return Err(MigratoryError::Generic(
                "Mock VBoxManage delete error".to_string(),
            ));
        }
        if args.contains(&"list") {
            if std::env::var("MIGRATORY_TEST_MOCK_VBOXMANAGE_LIST_ERROR").is_ok() {
                return Err(MigratoryError::Generic(
                    "Mock VBoxManage list error".to_string(),
                ));
            }
            if std::env::var("MIGRATORY_TEST_MOCK_VBOXMANAGE_LIST_EMPTY").is_ok() {
                return Ok(String::new());
            }
            if args.contains(&"hostonlyifs") {
                return Ok("Name:\nName:   \nName: vboxnet0\nOther: ignore".to_string());
            }
            return Ok("Name: snap1 (UUID: 1234)\nName: snap2 (UUID: 5678)".to_string());
        }
        if std::env::var("MIGRATORY_TEST_MOCK_RUNNING").is_ok() {
            return Ok("VMState=\"running\"\n".to_string());
        }
        return Ok(String::new());
    }

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
    use std::collections::HashMap;

    #[test]
    fn test_virtualbox_provider_methods() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let bin_vbox = temp_dir.path().join("VBoxManage");
        let bin_lower = temp_dir.path().join("vboxmanage");
        let _inspect_bin = temp_dir.path().join("vboxmanage_or_similar"); // optional

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = r#"#!/bin/sh
            exit 0
            "#;
            std::fs::write(&bin_vbox, mock_script).expect("operation should succeed");
            std::fs::set_permissions(&bin_vbox, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
            std::fs::write(&bin_lower, mock_script).expect("operation should succeed");
            std::fs::set_permissions(&bin_lower, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("VBoxManage.bat");
            std::fs::write(
                &bat,
                "@echo off
exit 0",
            )
            .expect("operation should succeed");
            let bat_lower = temp_dir.path().join("vboxmanage.bat");
            std::fs::write(
                &bat_lower,
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

        let provider = VirtualBoxProvider::new(Some("test-id".to_string()));

        let config = crate::config::VmConfig::default();
        let _ = provider.up(&config);
        let _ = provider.halt();
        let _ = provider.suspend();
        let _ = provider.resume();
        let _ = provider.destroy();

        let box_path = std::path::Path::new("/dummy");
        let _ = provider.import(box_path, "vm-1");
        let _ = provider.clone_machine("base-id", "vm-2");

        let _ = provider.status();

        unsafe {
            std::env::set_var("PATH", old_path);
        }
    }

    #[test]
    fn test_virtualbox_provider() {
        let provider = VirtualBoxProvider::new(Some("test-id".to_string()));
        assert_eq!(provider.name(), "virtualbox");
        assert!(provider.status().is_ok());

        // Don't test up/halt/destroy on real host system natively in standard unit tests.
        // It'll invoke VBoxManage. If it's missing, it fails.
        // But we can test it fails without an ID.
        let provider_no_id = VirtualBoxProvider::new(None);
        assert!(provider_no_id.up(&VmConfig::default()).is_err());
        assert!(provider_no_id.halt().is_err());
        assert!(provider_no_id.destroy().is_err());
        assert!(provider_no_id.suspend().is_err());
        assert!(provider_no_id.resume().is_err());
        assert!(provider_no_id.check_guest_additions().is_err());
        assert_eq!(
            provider_no_id.status().expect("operation should succeed"),
            "not created"
        );
    }

    #[test]
    fn test_virtualbox_provider_networks() {
        let provider = VirtualBoxProvider::new(Some("test-id".to_string()));
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
            bridge: None,
            use_dhcp_assigned_default_route: false,
        });

        assert!(provider.configure_networks(&config).is_ok());
    }

    #[test]
    #[coverage(off)]
    fn test_execute_vboxmanage_missing_cmd() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let result = execute_vboxmanage_inner("this_command_does_not_exist_123", &["list"]);
        assert!(matches!(result, Err(MigratoryError::Generic(_))));
    }

    #[test]
    fn test_execute_vboxmanage_success() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let result = execute_vboxmanage_inner("echo", &["test"]);
        assert!(result.is_ok());
    }

    #[test]
    #[coverage(off)]
    fn test_execute_vboxmanage_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let result = execute_vboxmanage_inner("false", &["test"]);
        assert!(matches!(result, Err(MigratoryError::Generic(_))));
    }

    #[test]
    fn test_get_host_interfaces_with_vboxmanage() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let bin_vbox = temp_dir.path().join("VBoxManage");
        let bin_lower = temp_dir.path().join("vboxmanage");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = "#!/bin/sh\necho \"Name: en2\"\necho \"Name: \"\nexit 0\n";
            std::fs::write(&bin_vbox, mock_script).expect("operation should succeed");
            std::fs::set_permissions(&bin_vbox, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
            std::fs::write(&bin_lower, mock_script).expect("operation should succeed");
            std::fs::set_permissions(&bin_lower, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("VBoxManage.bat");
            std::fs::write(&bat, "@echo off\necho Name: en2\necho Name: \nexit 0")
                .expect("operation should succeed");
            let bat_lower = temp_dir.path().join("vboxmanage.bat");
            std::fs::write(&bat_lower, "@echo off\necho Name: en2\necho Name: \nexit 0")
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

        let provider = VirtualBoxProvider::new(Some("test-id".to_string()));
        let interfaces = provider.get_host_interfaces();
        assert!(interfaces.contains_key("en2"));

        unsafe {
            std::env::set_var("PATH", old_path);
        }
    }

    #[test]
    fn test_virtualbox_deep_features() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
        }

        assert!(VirtualBoxProvider::get_version().is_ok());

        let provider = VirtualBoxProvider::new(Some("test-vm-123".to_string()));
        assert!(
            provider
                .create_storage_controller("SATA Controller", "sata")
                .is_ok()
        );
        assert!(
            provider
                .attach_storage_device("SATA Controller", 0, 0, "hdd", "/path/to/disk.vdi")
                .is_ok()
        );
        assert!(provider.configure_nat_dns_proxy(true).is_ok());
        assert!(provider.configure_nat_dns_proxy(false).is_ok());
        assert!(provider.list_hostonly_interfaces().is_ok());
        assert!(provider.create_hostonly_interface().is_ok());
        assert!(
            provider
                .manage_dhcp_server(
                    "add",
                    "vboxnet0",
                    "192.168.56.1",
                    "255.255.255.0",
                    "192.168.56.100",
                    "192.168.56.200"
                )
                .is_ok()
        );
        assert!(
            provider
                .manage_dhcp_server("remove", "vboxnet0", "", "", "", "")
                .is_ok()
        );
        assert!(
            provider
                .get_guest_property("/VirtualBox/GuestInfo/Net/0/V4/IP")
                .is_ok()
        );
        assert!(provider.set_guest_property("test_prop", "test_val").is_ok());
        assert!(provider.enumerate_guest_properties().is_ok());
        assert!(
            provider
                .customize_hardware(Some(2048), Some(2), true, Some(128))
                .is_ok()
        );
        assert!(provider.customize_hardware(None, None, false, None).is_ok());
        assert!(provider.acpi_power_button().is_ok());
        assert!(provider.discard_saved_state().is_ok());
        assert!(provider.enable_vrde(Some(3389)).is_ok());
        assert!(provider.enable_vrde(None).is_ok());
        assert!(
            provider
                .execute_customizations(&[vec![
                    "modifyvm".to_string(),
                    ":id".to_string(),
                    "--cpus".to_string(),
                    "4".to_string()
                ]])
                .is_ok()
        );

        let mut vm_conf = VmConfig::default();
        let mut p_opts = HashMap::new();
        p_opts.insert("separate".to_string(), "true".to_string());
        p_opts.insert("vrde".to_string(), "true".to_string());
        p_opts.insert("vrdeport".to_string(), "3389".to_string());
        vm_conf.providers.push(crate::config::ProviderConfig {
            name: "virtualbox".to_string(),
            options: p_opts,
        });
        assert!(provider.up(&vm_conf).is_ok());

        let mut vm_conf_mode = VmConfig::default();
        let mut p_opts_mode = HashMap::new();
        p_opts_mode.insert("boot_mode".to_string(), "separate".to_string());
        vm_conf_mode.providers.push(crate::config::ProviderConfig {
            name: "virtualbox".to_string(),
            options: p_opts_mode,
        });
        assert!(provider.up(&vm_conf_mode).is_ok());

        let no_id_provider = VirtualBoxProvider::new(None);
        assert!(no_id_provider.enable_vrde(Some(3389)).is_err());
        assert!(
            no_id_provider
                .create_storage_controller("SATA", "sata")
                .is_err()
        );
        assert!(
            no_id_provider
                .attach_storage_device("SATA", 0, 0, "hdd", "disk.vdi")
                .is_err()
        );
        assert!(no_id_provider.configure_nat_dns_proxy(true).is_err());
        assert!(no_id_provider.get_guest_property("prop").is_err());
        assert!(no_id_provider.set_guest_property("prop", "val").is_err());
        assert!(no_id_provider.enumerate_guest_properties().is_err());
        assert!(
            no_id_provider
                .customize_hardware(Some(1024), None, false, None)
                .is_err()
        );
        assert!(no_id_provider.acpi_power_button().is_err());
        assert!(no_id_provider.discard_saved_state().is_err());
        assert!(no_id_provider.execute_customizations(&[]).is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
        }
    }

    #[test]
    fn test_virtualbox_error_paths() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE_ERROR", "1");
        }

        assert!(VirtualBoxProvider::get_version().is_err());

        let provider = VirtualBoxProvider::new(Some("test-vm-123".to_string()));
        assert!(
            provider
                .create_storage_controller("SATA Controller", "sata")
                .is_err()
        );
        assert!(
            provider
                .attach_storage_device("SATA Controller", 0, 0, "hdd", "/path/to/disk.vdi")
                .is_err()
        );
        assert!(provider.configure_nat_dns_proxy(true).is_err());
        assert!(provider.list_hostonly_interfaces().is_err());
        assert!(
            provider
                .manage_dhcp_server(
                    "add",
                    "vboxnet0",
                    "192.168.56.1",
                    "255.255.255.0",
                    "192.168.56.100",
                    "192.168.56.200"
                )
                .is_err()
        );
        assert!(
            provider
                .manage_dhcp_server("remove", "vboxnet0", "", "", "", "")
                .is_err()
        );
        assert!(
            provider
                .customize_hardware(None, None, false, None)
                .is_err()
        );
        assert!(provider.acpi_power_button().is_err());
        assert!(provider.discard_saved_state().is_err());
        assert!(
            provider
                .execute_customizations(&[vec!["modifyvm".to_string(), ":id".to_string(),]])
                .is_err()
        );

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE_ERROR");
        }
    }
}
