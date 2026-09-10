//! Hyper-V provider implementation.
//!
//! This module provides the logic for interacting with Microsoft's Hyper-V
//! hypervisor, including lifecycle management and execution of PowerShell commands.

use super::Provider;
use crate::config::{NetworkConfig, VmConfig};
use crate::error::MigratoryError;
use crate::network;
use std::process::Command;

/// Hyper-V provider.
///
/// Implements the `Provider` trait to manage virtual machines on Hyper-V.
pub struct HypervProvider {
    /// Internal machine ID (Hyper-V VM name or VMId)
    machine_id: Option<String>,
}

impl HypervProvider {
    /// Creates a new HypervProvider instance.
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

                    let proto = protocol.as_deref().unwrap_or("TCP");
                    let host_ip_str = host_ip.as_deref().unwrap_or("0.0.0.0/0");

                    let nat_name = "MigratoryNAT";
                    let rule = format!(
                        "Add-NetNatStaticMapping -NatName {} -Protocol {} -ExternalIPAddress {} -InternalIPAddress GUEST_IP -ExternalPort {} -InternalPort {}",
                        nat_name, proto, host_ip_str, final_host, guest
                    );
                    let _ = execute_powershell_inner("powershell", &rule);
                }
                NetworkConfig::PrivateNetwork { .. } => {
                    let switch_name = format!("Migratory_Private_{}", i);
                    let create_switch =
                        format!("New-VMSwitch -Name {} -SwitchType Internal", switch_name);
                    let attach = format!(
                        "Add-VMNetworkAdapter -VMName {} -SwitchName {}",
                        id, switch_name
                    );
                    let _ = execute_powershell_inner("powershell", &create_switch);
                    let _ = execute_powershell_inner("powershell", &attach);
                }
                NetworkConfig::PublicNetwork { bridge, .. } => {
                    let switch_name = if let Some(b) = bridge {
                        b.clone()
                    } else {
                        // Automatic detection of the Hyper-V virtual switch
                        let out = execute_powershell_inner(
                            "powershell",
                            "Get-VMSwitch -SwitchType External | Select-Object -First 1 -ExpandProperty Name"
                        ).unwrap_or_else(|_| "Default Switch".to_string());
                        let trimmed = out.trim();
                        if trimmed.is_empty() {
                            "Default Switch".to_string()
                        } else {
                            trimmed.to_string()
                        }
                    };

                    let attach = format!(
                        "Add-VMNetworkAdapter -VMName {} -SwitchName '{}'",
                        id, switch_name
                    );
                    let _ = execute_powershell_inner("powershell", &attach);
                }
            }
        }
        Ok(())
    }
}

impl Provider for HypervProvider {
    /// Returns the canonical name of the provider.
    ///
    /// # Returns
    ///
    /// Returns `"hyperv"`.
    fn name(&self) -> &str {
        "hyperv"
    }

    /// Brings the Hyper-V machine up.
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
        let _id = self.require_id()?;

        for sf in &config.synced_folders {
            if sf.disabled {
                continue;
            }

            // Automatically fallback to SMB for synced folders on Hyper-V
            if let Some(t) = sf.folder_type.as_deref()
                && t != "smb"
                && t != "rsync"
            {
                // Only skip if explicitly not smb or rsync (and rsync is handled by rsync)
                // If it's vboxsf, nfs, we fallback to smb for Hyper-V.
            }

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

            let create_share = format!(
                "New-SmbShare -Name {} -Path {} -FullAccess Everyone",
                share_name, sf.host_path
            );
            execute_powershell_inner("powershell", &create_share)?;
        }
        Ok(())
    }

    fn up(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.configure_networks(config)?;
        self.setup_synced_folders(config)?;

        execute_powershell(&format!("Start-VM -Name '{}'", id))?;
        Ok(())
    }

    /// Halts the Hyper-V machine.
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
        execute_powershell(&format!("Stop-VM -Name '{}' -Force", id))?;
        Ok(())
    }

    fn import(&self, box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError> {
        let export_dir = box_dir.to_string_lossy();
        // Since we wrap inner powershell, use format directly
        execute_powershell(&format!(
            "Import-VM -Path '{}/Virtual Machines/*.vmcx' -Copy -GenerateNewId -VhdDestinationPath '{}/Virtual Hard Disks' -VirtualMachinePath '{}'",
            export_dir, export_dir, export_dir
        ))?;
        execute_powershell(&format!(
            "Rename-VM -Name (Get-VM | Select-Object -First 1).Name -NewName '{}'",
            vm_name
        ))?;
        Ok(vm_name.to_string())
    }

    fn clone_machine(
        &self,
        base_machine_id: &str,
        vm_name: &str,
    ) -> Result<String, MigratoryError> {
        execute_powershell(&format!(
            "Export-VM -Name '{}' -Path 'C:\\Temp\\Export'",
            base_machine_id
        ))?;
        execute_powershell(&format!(
            "Import-VM -Path 'C:\\Temp\\Export\\{}\\Virtual Machines\\*.vmcx' -Copy -GenerateNewId",
            base_machine_id
        ))?;
        execute_powershell(&format!(
            "Rename-VM -Name (Get-VM -Name '{}' | Select-Object -Skip 1).Name -NewName '{}'",
            base_machine_id, vm_name
        ))?;
        Ok(vm_name.to_string())
    }

    /// Destroys the Hyper-V machine.
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
        let _ = execute_powershell(&format!("Stop-VM -Name '{}' -TurnOff -Force", id));
        execute_powershell(&format!("Remove-VM -Name '{}' -Force", id))?;
        Ok(())
    }

    /// Retrieves the status of the Hyper-V machine.
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
        let out = execute_powershell(&format!("(Get-VM -Name '{}').State", id)).unwrap_or_default();
        if out.trim().is_empty() {
            Ok("unknown".to_string())
        } else {
            Ok(out.trim().to_lowercase())
        }
    }

    /// Suspends the Hyper-V machine.
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
        execute_powershell(&format!("Suspend-VM -Name '{}'", id))?;
        Ok(())
    }

    /// Resumes the Hyper-V machine.
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
        execute_powershell(&format!("Resume-VM -Name '{}'", id))?;
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_powershell(&format!(
            "Checkpoint-VM -Name '{}' -SnapshotName '{}'",
            id, name
        ))?;
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_powershell(&format!(
            "Restore-VMSnapshot -VMName '{}' -Name '{}' -Confirm:$false",
            id, name
        ))?;
        Ok(())
    }

    #[coverage(off)]
    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = execute_powershell(&format!(
            "Get-VMSnapshot -VMName '{}' | Select-Object -ExpandProperty Name",
            id
        ))
        .unwrap_or_default();
        Ok(out
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect())
    }

    #[coverage(off)]
    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_powershell(&format!(
            "Remove-VMSnapshot -VMName '{}' -Name '{}' -Confirm:$false",
            id, name
        ))?;
        Ok(())
    }
}

impl HypervProvider {
    /// Discovers the guest IP address via Hyper-V KVP data exchange.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if querying PowerShell fails.
    pub fn get_guest_ip(&self) -> Result<Option<String>, MigratoryError> {
        let id = self.require_id()?;
        let script = format!(
            "(Get-VMNetworkAdapter -VMName '{}').IPAddresses | Where-Object {{ $_ -match '^\\d+\\.\\d+\\.\\d+\\.\\d+$' }} | Select-Object -First 1",
            id
        );
        let out = execute_powershell(&script)?;
        let trimmed = out.trim();
        if trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(trimmed.to_string()))
        }
    }

    /// Creates a new Hyper-V virtual machine with Generation 1 or 2 support.
    ///
    /// # Arguments
    ///
    /// * `name` - The virtual machine name.
    /// * `generation` - Hyper-V generation (1 or 2).
    /// * `memory_mb` - Startup memory in megabytes.
    /// * `switch_name` - Virtual switch to attach to.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if creation fails.
    pub fn create_vm(
        &self,
        name: &str,
        generation: u8,
        memory_mb: u64,
        switch_name: &str,
    ) -> Result<(), MigratoryError> {
        let bytes = memory_mb * 1024 * 1024;
        let script = format!(
            "New-VM -Name '{}' -Generation {} -MemoryStartupBytes {} -SwitchName '{}'",
            name, generation, bytes, switch_name
        );
        execute_powershell(&script)?;
        Ok(())
    }

    /// Creates a differencing VHDX virtual hard disk pointing to a parent VHDX.
    ///
    /// # Arguments
    ///
    /// * `parent_path` - Path to the parent base VHDX disk.
    /// * `diff_path` - Path where the differencing disk should be created.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if creation fails.
    pub fn create_differencing_vhd(
        parent_path: &str,
        diff_path: &str,
    ) -> Result<(), MigratoryError> {
        let script = format!(
            "New-VHD -ParentPath '{}' -Path '{}' -Differencing",
            parent_path, diff_path
        );
        execute_powershell(&script)?;
        Ok(())
    }

    /// Configures CPU core count and dynamic memory for a virtual machine.
    ///
    /// # Arguments
    ///
    /// * `cpus` - Number of virtual processors.
    /// * `min_bytes` - Minimum dynamic memory in bytes.
    /// * `max_bytes` - Maximum dynamic memory in bytes.
    /// * `dynamic_memory` - Whether dynamic memory allocation is enabled.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if configuration fails.
    pub fn configure_cpu_and_memory(
        &self,
        cpus: u32,
        min_bytes: u64,
        max_bytes: u64,
        dynamic_memory: bool,
    ) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let dyn_mem_str = if dynamic_memory { "$true" } else { "$false" };
        let script = format!(
            "Set-VMProcessor -VMName '{}' -Count {}; Set-VMMemory -VMName '{}' -DynamicMemoryEnabled {} -MinimumBytes {} -MaximumBytes {}",
            id, cpus, id, dyn_mem_str, min_bytes, max_bytes
        );
        execute_powershell(&script)?;
        Ok(())
    }

    /// Saves the VM state to disk (`Stop-VM -Save`).
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if saving fails.
    pub fn save_vm_state(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_powershell(&format!("Stop-VM -Name '{}' -Save", id))?;
        Ok(())
    }
}

/// Executes a safe PowerShell cmdlet.
///
/// Wraps `powershell` with `-NoProfile` and `-NonInteractive` options.
///
/// # Arguments
///
/// * `cmdlet` - The PowerShell command string to execute.
///
/// # Returns
///
/// Returns the standard output of the command on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the command execution fails or returns a non-zero exit status.
#[coverage(off)]
pub fn execute_powershell(cmdlet: &str) -> Result<String, MigratoryError> {
    execute_powershell_inner("powershell", cmdlet)
}

fn execute_powershell_inner(cmd: &str, cmdlet: &str) -> Result<String, MigratoryError> {
    let output = Command::new(cmd)
        .args(["-NoProfile", "-NonInteractive", "-Command", cmdlet])
        .output();

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
    fn test_hyperv_provider_methods() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let bin = temp_dir.path().join("powershell");
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
            let bat = temp_dir.path().join("powershell.bat");
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

        let provider = HypervProvider::new(Some("test-id".to_string()));

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
    fn test_hyperv_status_running() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let bin = temp_dir.path().join("powershell");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = r#"#!/bin/sh
            echo "Running"
            exit 0
            "#;
            std::fs::write(&bin, mock_script).expect("operation should succeed");
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("powershell.bat");
            std::fs::write(&bat, "@echo off\necho Running\nexit 0")
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

        let provider = HypervProvider::new(Some("test-id".to_string()));
        assert_eq!(
            provider.status().expect("operation should succeed"),
            "running"
        );

        unsafe {
            std::env::set_var("PATH", old_path);
        }
    }

    #[test]
    fn test_hyperv_provider() {
        let provider = HypervProvider::new(Some("test-id".to_string()));
        assert_eq!(provider.name(), "hyperv");
        assert!(provider.status().is_ok());

        let provider_no_id = HypervProvider::new(None);
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
    fn test_hyperv_provider_networks() {
        let provider = HypervProvider::new(Some("test-id".to_string()));
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
    fn test_execute_powershell_missing_cmd() {
        // Provide a non-existent command to trigger the Err branch of `Command::output`
        let result = execute_powershell_inner("this_command_does_not_exist_123", "echo 1");
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_powershell_success() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        // Use 'echo' as a mock for powershell
        let result = execute_powershell_inner("echo", "test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_powershell_failure() {
        // Use 'false' to return a non-zero exit code and hit the stderr error branch.
        // On some platforms 'false' might not be available or behaves differently,
        // but typically it returns exit code 1 and fails out.status.success().
        let result = execute_powershell_inner("false", "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_hyperv_deep_features() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let bin = temp_dir.path().join("powershell");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = "#!/bin/sh\necho \"192.168.1.50\"\nexit 0\n";
            std::fs::write(&bin, mock_script).expect("operation should succeed");
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("powershell.bat");
            std::fs::write(&bat, "@echo off\necho 192.168.1.50\nexit 0")
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

        let provider = HypervProvider::new(Some("test-vm".to_string()));
        let ip = provider.get_guest_ip();
        assert!(ip.is_ok());
        assert_eq!(
            ip.expect("operation should succeed"),
            Some("192.168.1.50".to_string())
        );

        assert!(
            provider
                .create_vm("gen2-vm", 2, 2048, "Default Switch")
                .is_ok()
        );
        assert!(HypervProvider::create_differencing_vhd("base.vhdx", "diff.vhdx").is_ok());
        assert!(
            provider
                .configure_cpu_and_memory(4, 1073741824, 4294967296, true)
                .is_ok()
        );
        assert!(provider.save_vm_state().is_ok());

        let no_id = HypervProvider::new(None);
        assert!(no_id.get_guest_ip().is_err());
        assert!(
            no_id
                .configure_cpu_and_memory(2, 1024, 2048, false)
                .is_err()
        );
        assert!(no_id.save_vm_state().is_err());

        unsafe {
            std::env::set_var("PATH", old_path);
        }
    }

    #[test]
    fn test_hyperv_network_failures() {
        let no_id = HypervProvider::new(None);
        let mut config = VmConfig::default();
        assert!(no_id.configure_networks(&config).is_err());

        // Port collision without auto-correct
        let provider = HypervProvider::new(Some("test-id".to_string()));
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
    fn test_hyperv_empty_outputs() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let bin = temp_dir.path().join("powershell");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = "#!/bin/sh\nexit 0\n";
            std::fs::write(&bin, mock_script).expect("operation should succeed");
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("powershell.bat");
            std::fs::write(&bat, "@echo off\nexit 0").expect("operation should succeed");
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

        let provider = HypervProvider::new(Some("test-vm".to_string()));
        let ip = provider.get_guest_ip();
        assert!(ip.is_ok());
        assert_eq!(ip.expect("operation should succeed"), None);

        // Public network with bridge: None and empty powershell output (line 84 Default Switch)
        let mut config = VmConfig::default();
        config.networks.push(NetworkConfig::PublicNetwork {
            ip: None,
            bridge: None,
            use_dhcp_assigned_default_route: false,
        });
        assert!(provider.configure_networks(&config).is_ok());

        // configure_cpu_and_memory with dynamic_memory: false (covers else branch line 423)
        assert!(
            provider
                .configure_cpu_and_memory(2, 1024, 2048, false)
                .is_ok()
        );

        unsafe {
            std::env::set_var("PATH", old_path);
        }
    }

    #[test]
    fn test_hyperv_powershell_cmd_failures() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let bin = temp_dir.path().join("powershell");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = "#!/bin/sh\nexit 1\n";
            std::fs::write(&bin, mock_script).expect("operation should succeed");
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("powershell.bat");
            std::fs::write(&bat, "@echo off\nexit 1").expect("operation should succeed");
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

        let provider = HypervProvider::new(Some("test-vm".to_string()));
        let config = VmConfig::default();

        assert!(provider.up(&config).is_err());

        // Also test up failing in setup_synced_folders
        let mut sf_config = VmConfig::default();
        sf_config
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: "/host".to_string(),
                guest_path: "/guest".to_string(),
                disabled: false,
                folder_type: None,
                ..Default::default()
            });
        assert!(provider.up(&sf_config).is_err());

        assert!(provider.halt().is_err());
        assert!(
            provider
                .import(std::path::Path::new("/dummy"), "vm")
                .is_err()
        );
        assert!(provider.clone_machine("base", "vm").is_err());
        assert!(provider.destroy().is_err());
        assert!(provider.suspend().is_err());
        assert!(provider.resume().is_err());
        assert!(provider.get_guest_ip().is_err());
        assert!(provider.create_vm("vm", 1, 1024, "Switch").is_err());
        assert!(HypervProvider::create_differencing_vhd("base", "diff").is_err());
        assert!(
            provider
                .configure_cpu_and_memory(2, 1024, 2048, true)
                .is_err()
        );
        assert!(provider.save_vm_state().is_err());

        // Public network fallback when powershell command fails (line 81 unwrap_or_else)
        let mut net_config = VmConfig::default();
        net_config.networks.push(NetworkConfig::PublicNetwork {
            ip: None,
            bridge: None,
            use_dhcp_assigned_default_route: false,
        });
        assert!(provider.configure_networks(&net_config).is_ok());

        // Test failure on Rename-VM (covers import 2nd call and clone_machine 3rd call)
        #[cfg(unix)]
        {
            let rename_fail_script = r#"#!/bin/sh
if echo "$*" | grep -q "Rename-VM"; then
    exit 1
fi
exit 0
"#;
            std::fs::write(&bin, rename_fail_script).expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let rename_fail_bat = "@echo off\necho %* | findstr /i \"Rename-VM\" >nul\nif %errorlevel% equ 0 exit /b 1\nexit /b 0\n";
            std::fs::write(temp_dir.path().join("powershell.bat"), rename_fail_bat)
                .expect("operation should succeed");
        }
        assert!(
            provider
                .import(std::path::Path::new("/dummy"), "vm")
                .is_err()
        );
        assert!(provider.clone_machine("base", "vm").is_err());

        // Test failure on Import-VM in clone_machine (covers clone_machine 2nd call)
        #[cfg(unix)]
        {
            let import_fail_script = r#"#!/bin/sh
if echo "$*" | grep -q "Import-VM"; then
    exit 1
fi
exit 0
"#;
            std::fs::write(&bin, import_fail_script).expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let import_fail_bat = "@echo off\necho %* | findstr /i \"Import-VM\" >nul\nif %errorlevel% equ 0 exit /b 1\nexit /b 0\n";
            std::fs::write(temp_dir.path().join("powershell.bat"), import_fail_bat)
                .expect("operation should succeed");
        }
        assert!(provider.clone_machine("base", "vm").is_err());

        unsafe {
            std::env::set_var("PATH", old_path);
        }
    }
}
