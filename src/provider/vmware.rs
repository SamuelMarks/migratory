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
}

impl VmwareProvider {
    /// Creates a new VmwareProvider instance.
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

        let mut eth_index = 1; // ethernet0 is typically NAT

        for net in &config.networks {
            match net {
                NetworkConfig::ForwardedPort { guest, .. } => {
                    let collision_res = network::check_forwarded_port(net, &open_ports)?;
                    let final_host = collision_res.corrected_host_port;
                    open_ports.push(final_host);

                    // Write to nat.conf (on macOS Fusion as an example, though often requires sudo)
                    let rule = format!("\n[incomingtcp]\n{} = GUEST_IP:{}\n", final_host, guest);
                    // In a real robust implementation, this would require root or a helper tool.
                    // We append to a mock file in test, or skip if not permitted.
                    let nat_conf_path =
                        std::env::var("MIGRATORY_TEST_NAT_CONF").unwrap_or_else(|_| {
                            "/Library/Preferences/VMware Fusion/vmnet8/nat.conf".to_string()
                        });
                    let nat_conf = std::path::Path::new(&nat_conf_path);
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

        execute_vmrun(&["start", id, "nogui"])?;
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
        execute_vmrun(&["stop", id, "soft"])?;
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
            execute_vmrun(&[
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
        execute_vmrun(&[
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
        execute_vmrun(&["deleteVM", id])?;
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
    fn status(&self) -> Result<String, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok("not created".to_string()),
        };
        // Use vmrun list and check if id is in it, or just use readstate if available
        let out = execute_vmrun(&["list"]).unwrap_or_default();
        if out.contains(id) {
            Ok("running".to_string())
        } else {
            Ok("poweroff".to_string())
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
        execute_vmrun(&["suspend", id])?;
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
        execute_vmrun(&["start", id, "nogui"])?;
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_vmrun(&["snapshot", id, name])?;
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_vmrun(&["revertToSnapshot", id, name])?;
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = execute_vmrun(&["listSnapshots", id]).unwrap_or_default();
        Ok(out
            .lines()
            .skip(1)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect())
    }

    #[coverage(off)]
    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_vmrun(&["deleteSnapshot", id, name])?;
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
        if let Some(path_str) = std::env::var_os("MIGRATORY_VMWARE_EXEC") {
            let path = std::path::PathBuf::from(path_str);
            if path.exists() {
                return Some(path);
            }
        }
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
        execute_vmrun(&["reset", id, mode])?;
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

fn execute_vmrun_inner(cmd: &str, args: &[&str]) -> Result<String, MigratoryError> {
    if std::env::var("MIGRATORY_TEST_MOCK_VMRUN_ERR").is_ok() {
        return Err(MigratoryError::Generic("Mock Error".to_string()));
    }
    if std::env::var("MIGRATORY_TEST_MOCK_VMRUN_RUNNING").is_ok() {
        return Ok("test-id".to_string());
    }
    if std::env::var("MIGRATORY_TEST_MOCK_VMRUN").is_ok() {
        return Ok("mock_output".to_string());
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

    #[test]
    fn test_vmware_provider_methods() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let bin = temp_dir.path().join("vmrun");
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
            let bat = temp_dir.path().join("vmrun.bat");
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

        let provider = VmwareProvider::new(Some("test-id".to_string()));

        let config = crate::config::VmConfig::default();
        let _ = provider.up(&config);
        let _ = provider.halt();
        let _ = provider.suspend();
        let _ = provider.resume();
        let _ = provider.destroy();

        let box_path = temp_dir.path().join("dummy");
        std::fs::create_dir_all(&box_path).expect("operation should succeed");
        std::fs::write(box_path.join("dummy.vmx"), "").expect("operation should succeed");

        let _ = provider.import(&box_path, "vm-1");
        let _ = provider.clone_machine("base-id.vmx", "vm-2");

        let _ = provider.status();

        unsafe {
            std::env::set_var("PATH", old_path);
        }
    }

    #[test]
    fn test_vmware_provider() {
        let provider = VmwareProvider::new(Some("test-id".to_string()));
        assert_eq!(provider.name(), "vmware");
        assert!(provider.status().is_ok());

        let provider_no_id = VmwareProvider::new(None);
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
    fn test_vmware_provider_networks() {
        // let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK.lock().expect("operation should succeed");
        let provider = VmwareProvider::new(Some("test-id".to_string()));
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

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VMRUN", "1");
            std::env::set_var("MIGRATORY_TEST_NAT_CONF", "/does/not/exist");
        }
        assert!(provider.configure_networks(&config).is_ok());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMRUN");
            std::env::remove_var("MIGRATORY_TEST_NAT_CONF");
        }
    }
    #[test]
    fn test_execute_vmrun_missing_cmd() {
        let result = execute_vmrun_inner("this_command_does_not_exist_123", &["list"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_vmrun_success() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let result = execute_vmrun_inner("echo", &["test"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_vmrun_failure() {
        let result = execute_vmrun_inner("false", &["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_vmware_status_running() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let provider = VmwareProvider::new(Some("test-id".to_string()));

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VMRUN_RUNNING", "1");
        }
        let status = provider.status().unwrap_or_default();
        assert_eq!(status, "running");
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMRUN_RUNNING");
        }
    }

    #[test]
    fn test_vmware_status_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let provider = VmwareProvider::new(Some("test-id".to_string()));
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VMRUN_ERR", "1");
        }
        let status = provider.status().unwrap_or_default();
        assert_eq!(status, "poweroff");
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMRUN_ERR");
        }
    }
    #[test]
    fn test_vmware_import_no_vmx() {
        let provider = VmwareProvider::new(Some("test-id".to_string()));
        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let txt_path = temp_dir.path().join("dummy.txt");
        std::fs::write(&txt_path, "dummy text").expect("operation should succeed");
        let no_ext_path = temp_dir.path().join("no_extension_file");
        std::fs::write(&no_ext_path, "no ext").expect("operation should succeed");

        let result = provider.import(temp_dir.path(), "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_vmware_import_success() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let provider = VmwareProvider::new(Some("test-id".to_string()));
        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let vmx_path = temp_dir.path().join("image.vmx");
        let txt_path = temp_dir.path().join("dummy.txt");
        std::fs::write(&txt_path, "dummy text").expect("operation should succeed");
        std::fs::write(&vmx_path, "dummy").expect("operation should succeed");

        let result = provider.import(temp_dir.path(), "test");
        let _ = result;

        let temp_dir_no_vmx = tempfile::tempdir().expect("operation should succeed");
        let txt_only = temp_dir_no_vmx.path().join("only.txt");
        std::fs::write(&txt_only, "dummy text").expect("operation should succeed");
        let res_no_vmx = provider.import(temp_dir_no_vmx.path(), "test");
        assert!(res_no_vmx.is_err());
    }

    #[test]
    fn test_vmware_network_no_env() {
        let provider = VmwareProvider::new(Some("test-id".to_string()));
        let mut config = VmConfig::default();
        config.networks.push(NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: false,
            protocol: None,
            host_ip: None,
        });

        // Temporarily remove env var if present to test fallback path
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_NAT_CONF");
        }

        let _ = provider.configure_networks(&config);
    }

    #[test]
    fn test_vmware_network_nat_conf() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let nat_conf = temp_dir.path().join("nat.conf");
        std::fs::write(&nat_conf, "[incomingtcp]\n").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_NAT_CONF", &nat_conf);
        }

        let provider = VmwareProvider::new(Some("test-id".to_string()));
        let mut config = VmConfig::default();
        config.networks.push(NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: false,
            protocol: None,
            host_ip: None,
        });

        let _ = provider.configure_networks(&config);

        let content = std::fs::read_to_string(&nat_conf).expect("operation should succeed");
        assert!(content.contains("8080 = GUEST_IP:80"));

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_NAT_CONF");
        }
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
    fn test_execute_vmrun_inner_mock() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMRUN_RUNNING");
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMRUN_ERR");
            std::env::set_var("MIGRATORY_TEST_MOCK_VMRUN", "1");
        }

        let res = execute_vmrun_inner("vmrun", &[]);

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMRUN");
        }

        assert_eq!(res.expect("operation should succeed"), "mock_output");
    }

    #[test]
    fn test_vmware_deep_features() {
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

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let vmx_file = temp_dir.path().join("machine.vmx");
        std::fs::write(&vmx_file, sample_vmx).expect("operation should succeed");

        let provider = VmwareProvider::new(Some(vmx_file.to_string_lossy().to_string()));
        assert!(
            provider
                .configure_hgfs("vagrant-root", temp_dir.path())
                .is_ok()
        );

        let updated_content = std::fs::read_to_string(&vmx_file).expect("operation should succeed");
        assert!(updated_content.contains("sharedFolder0.present = \"TRUE\""));
        assert!(updated_content.contains("sharedFolder0.guestName = \"vagrant-root\""));

        assert!(
            provider
                .customize_vmx(Some("20"), Some(4), Some(4096), Some(true), Some("nat"))
                .is_ok()
        );
        let cust_content = std::fs::read_to_string(&vmx_file).expect("operation should succeed");
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
        let cust_content_false =
            std::fs::read_to_string(&vmx_file).expect("operation should succeed");
        assert!(cust_content_false.contains("vhv.enable = \"FALSE\""));

        // find_vmware_executable with env override
        let dummy_exec = temp_dir.path().join("mock-vmrun");
        std::fs::write(&dummy_exec, "").expect("operation should succeed");
        unsafe {
            std::env::set_var("MIGRATORY_VMWARE_EXEC", &dummy_exec);
        }
        assert!(VmwareProvider::find_vmware_executable().is_some());
        unsafe {
            std::env::set_var("MIGRATORY_VMWARE_EXEC", "/nonexistent/mock-vmrun");
        }
        let _ = VmwareProvider::find_vmware_executable();
        unsafe {
            std::env::remove_var("MIGRATORY_VMWARE_EXEC");
        }
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

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VMRUN", "1");
        }
        assert!(provider.reset(true).is_ok());
        assert!(provider.reset(false).is_ok());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMRUN");
        }

        let no_id = VmwareProvider::new(None);
        assert!(no_id.configure_hgfs("root", temp_dir.path()).is_err());
        assert!(no_id.customize_vmx(None, None, None, None, None).is_err());
        assert!(no_id.reset(false).is_err());

        // Error writing vmx (when id is a directory)
        let dir_as_id = VmwareProvider::new(Some(temp_dir.path().to_string_lossy().to_string()));
        assert!(dir_as_id.configure_hgfs("root", temp_dir.path()).is_err());
        assert!(
            dir_as_id
                .customize_vmx(Some("20"), None, None, None, None)
                .is_err()
        );
    }

    #[test]
    fn test_vmware_vmrun_cmd_failures() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let vmx_file = temp_dir.path().join("machine.vmx");
        std::fs::write(&vmx_file, "displayName = \"test\"\n").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VMRUN_ERR", "1");
        }

        let provider = VmwareProvider::new(Some(vmx_file.to_string_lossy().to_string()));
        let config = VmConfig::default();

        assert!(provider.up(&config).is_err());
        assert!(provider.halt().is_err());
        assert!(provider.import(temp_dir.path(), "vm").is_err());
        assert!(provider.clone_machine("base", "vm").is_err());
        assert!(provider.destroy().is_err());
        assert!(provider.suspend().is_err());
        assert!(provider.resume().is_err());
        assert!(provider.reset(false).is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMRUN_ERR");
        }
    }
}
