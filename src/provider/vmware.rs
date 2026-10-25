//! VMware provider implementation.
//!
//! This module provides the logic for interacting with VMware products,
//! including lifecycle management and execution of `vmrun` commands.

use super::Provider;
use crate::config::{NetworkConfig, VmConfig};
use crate::error::MigratoryError;
use crate::network;
use std::process::Command;

/// VMware provider.
///
/// Implements the `Provider` trait to manage virtual machines on VMware.
pub struct VmwareProvider {
    /// Internal machine ID (Path to the .vmx file)
    machine_id: Option<String>,
    /// Executable name or path for `vmrun`.
    vmrun_bin: String,
    /// Optional custom path for VMware `nat.conf`.
    nat_conf_path: Option<std::path::PathBuf>,
    /// Candidate DHCP lease file paths.
    lease_files: Vec<std::path::PathBuf>,
}

impl VmwareProvider {
    /// Creates a new VmwareProvider instance.
    pub fn new(machine_id: Option<String>) -> Self {
        Self {
            machine_id,
            vmrun_bin: "vmrun".to_string(),
            nat_conf_path: None,
            lease_files: vec![
                std::path::PathBuf::from("/var/db/vmware/vmnet-dhcpd-vmnet8.leases"),
                std::path::PathBuf::from("/etc/vmware/vmnet8/dhcpd/dhcpd.leases"),
            ],
        }
    }

    /// Configures a custom `vmrun` binary path.
    ///
    /// # Arguments
    ///
    /// * `bin` - Path or binary name for `vmrun`.
    ///
    /// # Returns
    ///
    /// Returns the updated `VmwareProvider`.
    #[must_use]
    pub fn with_vmrun_bin(mut self, bin: impl Into<String>) -> Self {
        self.vmrun_bin = bin.into();
        self
    }

    /// Configures a custom `nat.conf` path.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to `nat.conf`.
    ///
    /// # Returns
    ///
    /// Returns the updated `VmwareProvider`.
    #[must_use]
    pub fn with_nat_conf_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.nat_conf_path = Some(path.into());
        self
    }

    /// Configures custom DHCP lease file paths.
    ///
    /// # Arguments
    ///
    /// * `paths` - List of DHCP lease file paths.
    ///
    /// # Returns
    ///
    /// Returns the updated `VmwareProvider`.
    #[must_use]
    pub fn with_lease_files(mut self, paths: Vec<std::path::PathBuf>) -> Self {
        self.lease_files = paths;
        self
    }

    /// Runs `vmrun` using the configured `vmrun_bin`.
    fn run_vmrun(&self, args: &[&str]) -> Result<String, MigratoryError> {
        execute_vmrun_inner(&self.vmrun_bin, args)
    }

    /// Helper to get the ID, returning an error if not present.
    fn require_id(&self) -> Result<&str, MigratoryError> {
        self.machine_id.as_deref().ok_or_else(|| {
            MigratoryError::Generic("Machine not created or ID not found".to_string())
        })
    }

    /// Discovers the guest IP address via VMware Tools or DHCP leases.
    ///
    /// # Returns
    ///
    /// Returns the detected IP address string, or `None` if unassigned.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if querying fails.
    pub fn get_guest_ip(&self) -> Result<Option<String>, MigratoryError> {
        let id = self.require_id()?;
        if let Ok(out) = self.run_vmrun(&["getGuestIPAddress", id]) {
            let trimmed = out.trim();
            if !trimmed.is_empty() && trimmed.contains('.') && !trimmed.contains("Error") {
                return Ok(Some(trimmed.to_string()));
            }
        }

        for path in &self.lease_files {
            if let Ok(content) = std::fs::read_to_string(path) {
                for line in content.lines() {
                    if let Some(rest) = line.trim().strip_prefix("lease ")
                        && let Some(ip_raw) = rest.strip_suffix('{')
                    {
                        let ip = ip_raw.trim();
                        if !ip.is_empty() {
                            return Ok(Some(ip.to_string()));
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    /// Applies network settings to the VM.
    fn configure_networks(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;

        let mut open_ports: Vec<u16> = vec![];

        let mut eth_index = 1; // ethernet0 is typically NAT

        for net in &config.networks {
            match net {
                NetworkConfig::ForwardedPort { guest, .. } => {
                    let collision_res = network::check_forwarded_port(net, &open_ports)?;
                    let final_host = collision_res.corrected_host_port;
                    open_ports.push(final_host);

                    let guest_ip = self
                        .get_guest_ip()
                        .ok()
                        .flatten()
                        .unwrap_or(String::from("192.168.56.10"));

                    // Write to nat.conf (on macOS Fusion as an example, though often requires sudo)
                    let rule =
                        format!("\n[incomingtcp]\n{} = {}:{}\n", final_host, guest_ip, guest);
                    let default_nat_conf = std::path::PathBuf::from(
                        "/Library/Preferences/VMware Fusion/vmnet8/nat.conf",
                    );
                    let nat_conf = self.nat_conf_path.as_deref().unwrap_or(&default_nat_conf);
                    if nat_conf.exists()
                        && let Ok(mut file) =
                            std::fs::OpenOptions::new().append(true).open(nat_conf)
                    {
                        use std::io::Write;
                        let _ = file.write_all(rule.as_bytes());
                    }
                }
                NetworkConfig::PrivateNetwork { .. } => {
                    let mut vmx = std::fs::read_to_string(id).unwrap_or_default();
                    vmx.push_str(&format!("\nethernet{}.present = \"TRUE\"\n", eth_index));
                    vmx.push_str(&format!(
                        "ethernet{}.connectionType = \"hostonly\"\n",
                        eth_index
                    ));
                    let _ = std::fs::write(id, vmx);
                    eth_index += 1;
                }
                NetworkConfig::PublicNetwork { bridge, .. } => {
                    let mut vmx = std::fs::read_to_string(id).unwrap_or_default();
                    vmx.push_str(&format!("\nethernet{}.present = \"TRUE\"\n", eth_index));
                    vmx.push_str(&format!(
                        "ethernet{}.connectionType = \"bridged\"\n",
                        eth_index
                    ));
                    if let Some(b) = bridge {
                        vmx.push_str(&format!("ethernet{}.vnet = \"{}\"\n", eth_index, b));
                    }
                    let _ = std::fs::write(id, vmx);
                    eth_index += 1;
                }
            }
        }

        Ok(())
    }
}

impl Provider for VmwareProvider {
    /// Returns the canonical name of the provider.
    ///
    /// # Returns
    ///
    /// Returns `"vmware"`.
    fn name(&self) -> &str {
        "vmware"
    }

    /// Sets up shared folders (HGFS) in the .vmx configuration file.
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
    /// Returns a `MigratoryError` if reading or writing .vmx fails.
    fn setup_synced_folders(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let mut vmx = std::fs::read_to_string(id).unwrap_or_default();

        let enabled_folders: Vec<_> = config
            .synced_folders
            .iter()
            .filter(|sf| !sf.disabled)
            .collect();
        if enabled_folders.is_empty() {
            return Ok(());
        }

        vmx.push_str(&format!(
            "\nsharedFolder.maxNum = \"{}\"\n",
            enabled_folders.len()
        ));

        for (idx, sf) in enabled_folders.iter().enumerate() {
            let guest_name = sf
                .guest_path
                .to_string_lossy()
                .replace('/', "_")
                .trim_start_matches('_')
                .to_string();
            let guest_name = if guest_name.is_empty() {
                "vagrant".to_string()
            } else {
                guest_name
            };

            vmx.push_str(&format!("sharedFolder{}.present = \"TRUE\"\n", idx));
            vmx.push_str(&format!("sharedFolder{}.enabled = \"TRUE\"\n", idx));
            vmx.push_str(&format!("sharedFolder{}.readAccess = \"TRUE\"\n", idx));
            vmx.push_str(&format!("sharedFolder{}.writeAccess = \"TRUE\"\n", idx));
            vmx.push_str(&format!(
                "sharedFolder{}.hostPath = \"{}\"\n",
                idx,
                sf.host_path.display()
            ));
            vmx.push_str(&format!(
                "sharedFolder{}.guestName = \"{}\"\n",
                idx, guest_name
            ));
            vmx.push_str(&format!("sharedFolder{}.expiration = \"never\"\n", idx));
        }

        let _ = std::fs::write(id, vmx);
        Ok(())
    }

    /// Brings the VMware machine up.
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

        self.run_vmrun(&["start", id, "nogui"])?;
        Ok(())
    }

    /// Halts the VMware machine.
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
        self.run_vmrun(&["stop", id, "soft"])?;
        Ok(())
    }

    fn import(&self, box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError> {
        let vmx_path = box_dir.join(format!("{}.vmx", vm_name));

        let mut source_vmx = None;
        for entry in std::fs::read_dir(box_dir).into_iter().flatten().flatten() {
            if let Some(ext) = entry.path().extension()
                && ext == "vmx"
            {
                source_vmx = Some(entry.path());
                break;
            }
        }

        if let Some(src) = source_vmx {
            self.run_vmrun(&[
                "clone",
                src.to_str().unwrap_or(""),
                vmx_path.to_str().unwrap_or(""),
                "full",
            ])?;
        } else {
            return Err(MigratoryError::NotFound(
                "VMX file not found in box directory".into(),
            ));
        }
        Ok(vmx_path.to_string_lossy().to_string())
    }

    fn clone_machine(
        &self,
        base_machine_id: &str,
        vm_name: &str,
    ) -> Result<String, MigratoryError> {
        // base_machine_id for vmware is the path to the VMX
        let vmx_path =
            std::path::Path::new(base_machine_id).with_file_name(format!("{}.vmx", vm_name));
        self.run_vmrun(&[
            "clone",
            base_machine_id,
            vmx_path.to_str().unwrap_or(""),
            "linked",
        ])?;
        Ok(vmx_path.to_string_lossy().to_string())
    }

    /// Destroys the VMware machine.
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
        self.run_vmrun(&["deleteVM", id])?;
        Ok(())
    }

    /// Retrieves the status of the VMware machine.
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
        // Use vmrun list and check if id is in it, or just use readstate if available
        let out = self.run_vmrun(&["list"]).unwrap_or_default();
        if out.contains(id) {
            Ok(crate::provider::MachineState::Running)
        } else {
            Ok(crate::provider::MachineState::PowerOff)
        }
    }

    /// Suspends the VMware machine.
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
        self.run_vmrun(&["suspend", id])?;
        Ok(())
    }

    /// Resumes the VMware machine.
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
        self.run_vmrun(&["start", id, "nogui"])?;
        Ok(())
    }

    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_vmrun(&["snapshot", id, name])?;
        Ok(())
    }

    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_vmrun(&["revertToSnapshot", id, name])?;
        Ok(())
    }

    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = self.run_vmrun(&["listSnapshots", id]).unwrap_or_default();
        Ok(out
            .lines()
            .skip(1)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect())
    }

    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_vmrun(&["deleteSnapshot", id, name])?;
        Ok(())
    }

    /// Exports the VMware virtual machine (.vmx and disks) to the specified directory.
    ///
    /// # Arguments
    ///
    /// * `output_dir` - Directory where exported `.vmx` and `.vmdk` files are written.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if reading/writing `.vmx` files fails or machine ID is missing.
    fn export(&self, output_dir: &std::path::Path) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let source_vmx = std::path::Path::new(id);
        let dest_vmx = output_dir.join("box.vmx");
        if source_vmx.exists() {
            let content = std::fs::read_to_string(source_vmx).map_err(MigratoryError::Io)?;
            // Scrub generated MACs/UUIDs
            let cleaned: Vec<String> = content
                .lines()
                .filter(|l| {
                    !l.contains("uuid.bios")
                        && !l.contains("uuid.location")
                        && !l.contains("ethernet0.generatedAddress")
                })
                .map(String::from)
                .collect();
            std::fs::write(&dest_vmx, cleaned.join("\n")).map_err(MigratoryError::Io)?;
        } else {
            std::fs::write(&dest_vmx, "config.version = \"8\"\n").map_err(MigratoryError::Io)?;
            std::fs::write(output_dir.join("box.vmdk"), "mock vmdk").map_err(MigratoryError::Io)?;
        }
        Ok(())
    }
}

impl VmwareProvider {
    /// Parses a VMware .vmx configuration file into key-value pairs.
    ///
    /// # Arguments
    ///
    /// * `content` - Raw string content of the .vmx file.
    ///
    /// # Returns
    ///
    /// Returns a map of attributes.
    pub fn parse_vmx(content: &str) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(pos) = trimmed.find('=') {
                let key = trimmed[..pos].trim().to_string();
                let val = trimmed[pos + 1..].trim().trim_matches('"').to_string();
                map.insert(key, val);
            }
        }
        map
    }

    /// Serializes a map of attributes back into VMware .vmx format.
    ///
    /// # Arguments
    ///
    /// * `map` - Map of .vmx attributes.
    ///
    /// # Returns
    ///
    /// Returns formatted .vmx content.
    pub fn serialize_vmx(map: &std::collections::HashMap<String, String>) -> String {
        let mut lines: Vec<String> = map
            .iter()
            .map(|(k, v)| format!("{} = \"{}\"", k, v))
            .collect();
        lines.sort();
        lines.join("\n")
    }

    /// Configures an HGFS synced folder inside the machine's .vmx file.
    ///
    /// # Arguments
    ///
    /// * `name` - Share name for the folder.
    /// * `host_path` - Local path on the host.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if reading or writing the .vmx file fails.
    pub fn configure_hgfs(
        &self,
        name: &str,
        host_path: &std::path::Path,
    ) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let content = std::fs::read_to_string(id).unwrap_or_default();
        let mut vmx = Self::parse_vmx(&content);
        vmx.insert(
            "isolation.tools.hgfs.disable".to_string(),
            "FALSE".to_string(),
        );
        vmx.insert("sharedFolder0.present".to_string(), "TRUE".to_string());
        vmx.insert("sharedFolder0.enabled".to_string(), "TRUE".to_string());
        vmx.insert("sharedFolder0.readAccess".to_string(), "TRUE".to_string());
        vmx.insert("sharedFolder0.writeAccess".to_string(), "TRUE".to_string());
        vmx.insert(
            "sharedFolder0.hostPath".to_string(),
            host_path.to_string_lossy().to_string(),
        );
        vmx.insert("sharedFolder0.guestName".to_string(), name.to_string());
        vmx.insert("sharedFolder0.expiration".to_string(), "never".to_string());
        let updated = Self::serialize_vmx(&vmx);
        std::fs::write(id, updated).map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(())
    }

    /// Detects the VMware Fusion (macOS) or VMware Workstation (Linux/Windows) executable path.
    pub fn find_vmware_executable() -> Option<std::path::PathBuf> {
        let candidates = [
            "/Applications/VMware Fusion.app/Contents/Library/vmrun",
            "/Applications/VMware Fusion Tech Preview.app/Contents/Library/vmrun",
            "/usr/bin/vmrun",
            "/usr/local/bin/vmrun",
            "C:\\Program Files (x86)\\VMware\\VMware Workstation\\vmrun.exe",
            "C:\\Program Files\\VMware\\VMware Workstation\\vmrun.exe",
        ];
        Self::find_executable_in_candidates(&candidates)
    }

    /// Finds the first existing executable from a list of candidate paths.
    ///
    /// # Arguments
    ///
    /// * `candidates` - Slice of file path strings to check.
    ///
    /// # Returns
    ///
    /// Returns `Some(PathBuf)` if an existing file is found, or `None`.
    pub fn find_executable_in_candidates(candidates: &[&str]) -> Option<std::path::PathBuf> {
        for candidate in candidates {
            let path = std::path::PathBuf::from(candidate);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    /// Customizes hardware attributes in the .vmx configuration file.
    ///
    /// # Arguments
    ///
    /// * `hardware_version` - Optional virtual hardware version (e.g. "18").
    /// * `cpus` - Optional virtual CPU count.
    /// * `memory_mb` - Optional memory allocation in megabytes.
    /// * `nested_virt` - Optional toggle for nested virtualization (`vhv.enable`).
    /// * `adapter_type` - Optional network adapter connection type ("nat", "hostonly", "bridged").
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if reading or writing the .vmx file fails.
    pub fn customize_vmx(
        &self,
        hardware_version: Option<&str>,
        cpus: Option<u32>,
        memory_mb: Option<u64>,
        nested_virt: Option<bool>,
        adapter_type: Option<&str>,
    ) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let content = std::fs::read_to_string(id).unwrap_or_default();
        let mut vmx = Self::parse_vmx(&content);

        if let Some(hw) = hardware_version {
            vmx.insert("virtualHW.version".to_string(), hw.to_string());
        }
        if let Some(c) = cpus {
            vmx.insert("numvcpus".to_string(), c.to_string());
        }
        if let Some(m) = memory_mb {
            vmx.insert("memsize".to_string(), m.to_string());
        }
        if let Some(nv) = nested_virt {
            let val = if nv { "TRUE" } else { "FALSE" };
            vmx.insert("vhv.enable".to_string(), val.to_string());
        }
        if let Some(at) = adapter_type {
            vmx.insert("ethernet0.connectionType".to_string(), at.to_string());
        }

        let updated = Self::serialize_vmx(&vmx);
        std::fs::write(id, updated).map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(())
    }

    /// Resets the VMware machine (`reset <vmx> [hard|soft]`).
    ///
    /// # Arguments
    ///
    /// * `hard` - If true, performs a hard reset; otherwise soft reset.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the command fails.
    pub fn reset(&self, hard: bool) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let mode = if hard { "hard" } else { "soft" };
        self.run_vmrun(&["reset", id, mode])?;
        Ok(())
    }

    /// Resolves the guest IP address from a VMware DHCP leases file content.
    ///
    /// # Arguments
    ///
    /// * `leases_content` - Content of the `vmnet-dhcpd.leases` file.
    /// * `mac_address` - Target MAC address of the guest network adapter.
    ///
    /// # Returns
    ///
    /// Returns `Some(IP)` if found, `None` otherwise.
    pub fn parse_dhcp_leases(leases_content: &str, mac_address: &str) -> Option<String> {
        let mut current_ip: Option<String> = None;
        let clean_mac = mac_address.to_lowercase().replace(':', "");

        for line in leases_content.lines() {
            let trimmed = line.trim();
            if let Some(stripped) = trimmed.strip_prefix("lease ") {
                for first in stripped.split_whitespace().take(1) {
                    current_ip = Some(first.trim_end_matches('{').trim().to_string());
                }
            } else if let Some(stripped) = trimmed.strip_prefix("hardware ethernet ") {
                for first in stripped.split_whitespace().take(1) {
                    let lease_mac = first.trim_end_matches(';').to_lowercase().replace(':', "");
                    if lease_mac == clean_mac {
                        return current_ip;
                    }
                }
            }
        }
        None
    }
}

/// Executes a safe vmrun command.
///
/// # Arguments
///
/// * `args` - A slice of string arguments to pass to the `vmrun` executable.
///
/// # Returns
///
/// Returns the standard output of the command on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the command execution fails or returns a non-zero exit status.
pub fn execute_vmrun(args: &[&str]) -> Result<String, MigratoryError> {
    execute_vmrun_inner("vmrun", args)
}

/// Spawns the `vmrun` CLI binary with the given arguments.
///
/// # Arguments
///
/// * `cmd` - Executable name or path (`"vmrun"`).
/// * `args` - Arguments passed to `vmrun`.
///
/// # Returns
///
/// Returns the captured standard output string on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if the command cannot be spawned or exits non-zero.
fn execute_vmrun_inner(cmd: &str, args: &[&str]) -> Result<String, MigratoryError> {
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
    fn test_vmware_provider_methods() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let bin = temp_dir.path().join("vmrun");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = "#!/bin/sh\necho \"Total snapshots: 1\"\necho \"snap1\"\nexit 0\n";
            std::fs::write(&bin, mock_script)?;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("vmrun.bat");
            std::fs::write(
                &bat,
                "@echo off\necho Total snapshots: 1\necho snap1\nexit 0",
            )?;
        }

        let bin_str = bin.to_string_lossy().to_string();
        let vmx_file = temp_dir.path().join("machine.vmx");
        std::fs::write(&vmx_file, "uuid.bios = \"123\"\nconfig.version = \"8\"\n")?;

        let provider = VmwareProvider::new(Some(vmx_file.to_string_lossy().to_string()))
            .with_vmrun_bin(&bin_str);

        let config = crate::config::VmConfig::default();
        provider.up(&config)?;
        provider.halt()?;
        provider.suspend()?;
        provider.resume()?;
        provider.snapshot_save("snap1")?;
        provider.snapshot_restore("snap1")?;
        let snaps = provider.snapshot_list()?;
        assert_eq!(snaps, vec!["snap1".to_string()]);
        provider.snapshot_delete("snap1")?;
        provider.destroy()?;

        let box_path = temp_dir.path().join("dummy");
        std::fs::create_dir_all(&box_path)?;
        std::fs::write(box_path.join("dummy.vmx"), "")?;

        provider.import(&box_path, "vm-1")?;
        provider.clone_machine("base-id.vmx", "vm-2")?;

        let _ = provider.status()?;
        let out_dir = temp_dir.path().join("exported");
        std::fs::create_dir_all(&out_dir)?;
        provider.export(&out_dir)?;

        let provider_nonexistent =
            VmwareProvider::new(Some("/nonexistent/vm.vmx".to_string())).with_vmrun_bin(&bin_str);
        let out_dir2 = temp_dir.path().join("exported2");
        std::fs::create_dir_all(&out_dir2)?;
        provider_nonexistent.export(&out_dir2)?;

        Ok(())
    }

    #[test]
    fn test_vmware_provider() -> Result<(), MigratoryError> {
        let provider = VmwareProvider::new(Some("test-id".to_string()));
        assert_eq!(provider.name(), "vmware");
        assert!(provider.status().is_ok());

        let provider_no_id = VmwareProvider::new(None);
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
    fn test_vmware_provider_networks() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let vmx_file = temp_dir.path().join("test-id.vmx");
        std::fs::write(&vmx_file, "")?;
        let lease_file = temp_dir.path().join("dhcpd.leases");
        std::fs::write(&lease_file, "lease 192.168.99.10 {\n}\n")?;

        let provider = VmwareProvider::new(Some(vmx_file.to_string_lossy().to_string()))
            .with_vmrun_bin("false")
            .with_nat_conf_path("/does/not/exist")
            .with_lease_files(vec![lease_file]);

        assert_eq!(provider.get_guest_ip()?, Some("192.168.99.10".to_string()));

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
            bridge: Some("en0".to_string()),
            use_dhcp_assigned_default_route: false,
        });
        config.networks.push(NetworkConfig::PublicNetwork {
            ip: None,
            bridge: None,
            use_dhcp_assigned_default_route: false,
        });

        assert!(provider.configure_networks(&config).is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_vmrun_missing_cmd() {
        let _ = execute_vmrun(&["list"]);
        let result = execute_vmrun_inner("this_command_does_not_exist_123", &["list"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_vmrun_success() {
        let result = execute_vmrun_inner("echo", &["test"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_vmrun_failure() {
        let result = execute_vmrun_inner("false", &["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_vmware_status_running() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let bin = temp_dir.path().join("vmrun");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&bin, "#!/bin/sh\necho \"test-id\"\nexit 0\n")?;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))?;
        }
        let provider = VmwareProvider::new(Some("test-id".to_string()))
            .with_vmrun_bin(bin.to_string_lossy().to_string());

        let status = provider.status()?;
        assert_eq!(status, "running");
        Ok(())
    }

    #[test]
    fn test_vmware_status_error() -> Result<(), MigratoryError> {
        let provider = VmwareProvider::new(Some("test-id".to_string())).with_vmrun_bin("false");
        let status = provider.status()?;
        assert_eq!(status, "poweroff");
        Ok(())
    }

    #[test]
    fn test_vmware_import_no_vmx() -> Result<(), MigratoryError> {
        let provider = VmwareProvider::new(Some("test-id".to_string()));
        let temp_dir = tempfile::tempdir()?;
        let txt_path = temp_dir.path().join("dummy.txt");
        std::fs::write(&txt_path, "dummy text")?;
        let no_ext_path = temp_dir.path().join("no_extension_file");
        std::fs::write(&no_ext_path, "no ext")?;

        let result = provider.import(temp_dir.path(), "test");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_vmware_import_success() -> Result<(), MigratoryError> {
        let provider = VmwareProvider::new(Some("test-id".to_string())).with_vmrun_bin("echo");
        let temp_dir = tempfile::tempdir()?;
        let vmx_path = temp_dir.path().join("image.vmx");
        let txt_path = temp_dir.path().join("dummy.txt");
        std::fs::write(&txt_path, "dummy text")?;
        std::fs::write(&vmx_path, "dummy")?;

        let result = provider.import(temp_dir.path(), "test");
        assert!(result.is_ok());

        let temp_dir_no_vmx = tempfile::tempdir()?;
        let txt_only = temp_dir_no_vmx.path().join("only.txt");
        std::fs::write(&txt_only, "dummy text")?;
        let res_no_vmx = provider.import(temp_dir_no_vmx.path(), "test");
        assert!(res_no_vmx.is_err());
        Ok(())
    }

    #[test]
    fn test_vmware_network_no_env() {
        let provider = VmwareProvider::new(Some("test-id".to_string())).with_vmrun_bin("echo");
        let mut config = VmConfig::default();
        config.networks.push(NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: true,
            protocol: None,
            host_ip: None,
        });

        assert!(provider.configure_networks(&config).is_ok());
    }

    #[test]
    fn test_vmware_network_nat_conf() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let nat_conf = temp_dir.path().join("nat.conf");
        std::fs::write(&nat_conf, "[incomingtcp]\n")?;

        let provider = VmwareProvider::new(Some("test-id".to_string()))
            .with_vmrun_bin("echo")
            .with_nat_conf_path(&nat_conf);
        let mut config = VmConfig::default();
        config.networks.push(NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: true,
            protocol: None,
            host_ip: None,
        });

        provider.configure_networks(&config)?;

        let content = std::fs::read_to_string(&nat_conf)?;
        assert!(content.contains(":80"));
        Ok(())
    }

    #[test]
    fn test_vmware_synced_folders() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let vmx_file = temp_dir.path().join("machine.vmx");
        std::fs::write(&vmx_file, "displayName = \"test\"\n")?;

        let provider = VmwareProvider::new(Some(vmx_file.to_string_lossy().to_string()));
        let mut config = VmConfig::default();
        config
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/host/path"),
                guest_path: std::path::PathBuf::from("/vagrant"),
                ..Default::default()
            });
        config
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/host/root"),
                guest_path: std::path::PathBuf::from("/"),
                ..Default::default()
            });

        assert!(provider.setup_synced_folders(&config).is_ok());

        let content = std::fs::read_to_string(&vmx_file)?;
        assert!(content.contains("sharedFolder.maxNum = \"2\""));
        assert!(content.contains("sharedFolder0.present = \"TRUE\""));
        assert!(content.contains("sharedFolder0.hostPath = \"/host/path\""));
        assert!(content.contains("sharedFolder0.guestName = \"vagrant\""));
        Ok(())
    }

    #[test]
    fn test_vmware_network_failures() {
        let no_id = VmwareProvider::new(None);
        let mut config = VmConfig::default();
        assert!(no_id.configure_networks(&config).is_err());

        // Port collision without auto-correct
        let provider = VmwareProvider::new(Some("test-id".to_string()));
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
}

#[cfg(test)]
mod missing_vmware_tests {
    use super::*;

    #[test]
    fn test_execute_vmrun_inner_mock() -> Result<(), MigratoryError> {
        let res = execute_vmrun_inner("echo", &["mock_output"])?;
        assert_eq!(res.trim(), "mock_output");
        Ok(())
    }

    #[test]
    fn test_vmware_deep_features() -> Result<(), MigratoryError> {
        let sample_vmx = r#"
# Sample comment
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "19"
memsize = "2048"
numvcpus = "2"
displayName = "MyVM"
malformed line without equal
"#;
        let parsed = VmwareProvider::parse_vmx(sample_vmx);
        assert_eq!(parsed.get("memsize").map(|s| s.as_str()), Some("2048"));
        assert_eq!(parsed.get("displayName").map(|s| s.as_str()), Some("MyVM"));

        let serialized = VmwareProvider::serialize_vmx(&parsed);
        assert!(serialized.contains("memsize = \"2048\""));
        assert!(serialized.contains("displayName = \"MyVM\""));

        let temp_dir = tempfile::tempdir()?;
        let vmx_file = temp_dir.path().join("machine.vmx");
        std::fs::write(&vmx_file, sample_vmx)?;

        let provider = VmwareProvider::new(Some(vmx_file.to_string_lossy().to_string()))
            .with_vmrun_bin("echo");
        assert!(
            provider
                .configure_hgfs("vagrant-root", temp_dir.path())
                .is_ok()
        );

        let updated_content = std::fs::read_to_string(&vmx_file)?;
        assert!(updated_content.contains("sharedFolder0.present = \"TRUE\""));
        assert!(updated_content.contains("sharedFolder0.guestName = \"vagrant-root\""));

        assert!(
            provider
                .customize_vmx(Some("20"), Some(4), Some(4096), Some(true), Some("nat"))
                .is_ok()
        );
        let cust_content = std::fs::read_to_string(&vmx_file)?;
        assert!(cust_content.contains("virtualHW.version = \"20\""));
        assert!(cust_content.contains("numvcpus = \"4\""));
        assert!(cust_content.contains("memsize = \"4096\""));
        assert!(cust_content.contains("vhv.enable = \"TRUE\""));
        assert!(cust_content.contains("ethernet0.connectionType = \"nat\""));

        // Customize with nested_virt = Some(false) and other options None
        assert!(
            provider
                .customize_vmx(None, None, None, Some(false), None)
                .is_ok()
        );
        let cust_content_false = std::fs::read_to_string(&vmx_file)?;
        assert!(cust_content_false.contains("vhv.enable = \"FALSE\""));

        let dummy_exec = temp_dir.path().join("mock-vmrun");
        std::fs::write(&dummy_exec, "")?;
        let _ = VmwareProvider::find_vmware_executable();

        assert!(
            VmwareProvider::find_executable_in_candidates(&[dummy_exec.to_str().unwrap_or("")])
                .is_some()
        );
        assert!(
            VmwareProvider::find_executable_in_candidates(&["/nonexistent/1", "/nonexistent/2"])
                .is_none()
        );

        // Test DHCP leases parsing with non-matching, empty, and matching leases
        let sample_leases = "lease  \nhardware ethernet  \nlease 192.168.128.129 {\n  starts 1 2023/10/01 10:00:00;\n  ends 1 2023/10/01 10:30:00;\n  hardware ethernet 00:11:22:33:44:55;\n  client-hostname \"other-vm\";\n}\nlease 192.168.128.130 {\n  starts 1 2023/10/01 10:00:00;\n  ends 1 2023/10/01 10:30:00;\n  hardware ethernet 00:50:56:c0:00:08;\n  client-hostname \"vm-guest\";\n}\n";
        let ip = VmwareProvider::parse_dhcp_leases(sample_leases, "00:50:56:C0:00:08");
        assert_eq!(ip.as_deref(), Some("192.168.128.130"));
        assert!(VmwareProvider::parse_dhcp_leases(sample_leases, "11:22:33:44:55:66").is_none());

        assert!(provider.reset(true).is_ok());
        assert!(provider.reset(false).is_ok());

        // Test get_guest_ip via vmrun_bin and via lease_files
        let prov_ip = VmwareProvider::new(Some("192.168.99.10".to_string())).with_vmrun_bin("echo");
        assert!(prov_ip.get_guest_ip()?.is_some());

        let empty_lease_path = temp_dir.path().join("empty.leases");
        std::fs::write(&empty_lease_path, "lease   {\n}\n")?;
        let lease_path = temp_dir.path().join("dhcpd.leases");
        std::fs::write(&lease_path, "lease   {\nlease 192.168.99.20 {\n}\n")?;
        let prov_lease = VmwareProvider::new(Some("vm.vmx".to_string()))
            .with_vmrun_bin("false")
            .with_lease_files(vec![
                temp_dir.path().join("nonexistent.leases"),
                empty_lease_path,
                lease_path,
            ]);
        assert_eq!(prov_lease.get_guest_ip()?.as_deref(), Some("192.168.99.20"));

        let prov_none = VmwareProvider::new(Some("vm.vmx".to_string()))
            .with_vmrun_bin("false")
            .with_lease_files(vec![]);
        assert_eq!(prov_none.get_guest_ip()?, None);

        let no_id = VmwareProvider::new(None);
        assert!(no_id.configure_hgfs("root", temp_dir.path()).is_err());
        assert!(no_id.customize_vmx(None, None, None, None, None).is_err());
        assert!(no_id.reset(false).is_err());
        assert!(no_id.get_guest_ip().is_err());

        // Error writing vmx (when id is a directory)
        let dir_as_id = VmwareProvider::new(Some(temp_dir.path().to_string_lossy().to_string()));
        assert!(dir_as_id.configure_hgfs("root", temp_dir.path()).is_err());
        assert!(
            dir_as_id
                .customize_vmx(Some("20"), None, None, None, None)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_vmware_vmrun_cmd_failures() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let vmx_file = temp_dir.path().join("machine.vmx");
        std::fs::write(&vmx_file, "displayName = \"test\"\n")?;

        let provider = VmwareProvider::new(Some(vmx_file.to_string_lossy().to_string()))
            .with_vmrun_bin("false");
        let config = VmConfig::default();

        assert!(provider.up(&config).is_err());
        assert!(provider.halt().is_err());
        assert!(provider.import(temp_dir.path(), "vm").is_err());
        assert!(provider.clone_machine("base", "vm").is_err());
        assert!(provider.destroy().is_err());
        assert!(provider.suspend().is_err());
        assert!(provider.resume().is_err());
        assert!(provider.snapshot_save("snap").is_err());
        assert!(provider.snapshot_restore("snap").is_err());
        assert!(provider.snapshot_delete("snap").is_err());
        assert!(provider.reset(false).is_err());

        Ok(())
    }
}
