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
    /// Executable name or path for `powershell`.
    powershell_bin: String,
}

impl HypervProvider {
    /// Creates a new HypervProvider instance.
    pub fn new(machine_id: Option<String>) -> Self {
        Self {
            machine_id,
            powershell_bin: "powershell".to_string(),
        }
    }

    /// Configures a custom `powershell` binary path.
    ///
    /// # Arguments
    ///
    /// * `bin` - Path or binary name for `powershell`.
    ///
    /// # Returns
    ///
    /// Returns the updated `HypervProvider`.
    #[must_use]
    pub fn with_powershell_bin(mut self, bin: impl Into<String>) -> Self {
        self.powershell_bin = bin.into();
        self
    }

    /// Executes a PowerShell command using the configured `powershell_bin`.
    fn run_powershell(&self, cmdlet: &str) -> Result<String, MigratoryError> {
        execute_powershell_inner(&self.powershell_bin, cmdlet)
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

                    let guest_ip = self
                        .get_guest_ip()
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| "192.168.56.10".to_string());

                    let nat_name = "MigratoryNAT";
                    let rule = format!(
                        "Add-NetNatStaticMapping -NatName {} -Protocol {} -ExternalIPAddress {} -InternalIPAddress {} -ExternalPort {} -InternalPort {}",
                        nat_name, proto, host_ip_str, guest_ip, final_host, guest
                    );
                    let _ = self.run_powershell(&rule);
                }
                NetworkConfig::PrivateNetwork { .. } => {
                    let switch_name = format!("Migratory_Private_{}", i);
                    let create_switch =
                        format!("New-VMSwitch -Name {} -SwitchType Internal", switch_name);
                    let attach = format!(
                        "Add-VMNetworkAdapter -VMName {} -SwitchName {}",
                        id, switch_name
                    );
                    let _ = self.run_powershell(&create_switch);
                    let _ = self.run_powershell(&attach);
                }
                NetworkConfig::PublicNetwork { bridge, .. } => {
                    let switch_name = if let Some(b) = bridge {
                        b.clone()
                    } else {
                        // Automatic detection of the Hyper-V virtual switch
                        let out = self.run_powershell(
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
                    let _ = self.run_powershell(&attach);
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
    fn setup_synced_folders(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let _id = self.require_id()?;

        for sf in &config.synced_folders {
            if sf.disabled {
                continue;
            }

            // Automatically fallback to SMB for synced folders on Hyper-V
            if let Some(ref t) = sf.folder_type
                && *t != crate::config::SyncedFolderType::Smb
                && *t != crate::config::SyncedFolderType::Rsync
            {
                // Only skip if explicitly not smb or rsync (and rsync is handled by rsync)
                // If it's vboxsf, nfs, we fallback to smb for Hyper-V.
            }

            let share_name = sf
                .guest_path
                .to_string_lossy()
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
                share_name,
                sf.host_path.display()
            );
            self.run_powershell(&create_share)?;
        }
        Ok(())
    }

    fn up(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.configure_networks(config)?;
        self.setup_synced_folders(config)?;

        self.run_powershell(&format!("Start-VM -Name '{}'", id))?;
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
        self.run_powershell(&format!("Stop-VM -Name '{}' -Force", id))?;
        Ok(())
    }

    fn import(&self, box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError> {
        let export_dir = box_dir.to_string_lossy();
        self.run_powershell(&format!(
            "$imported = Import-VM -Path '{}/Virtual Machines/*.vmcx' -Copy -GenerateNewId -VhdDestinationPath '{}/Virtual Hard Disks' -VirtualMachinePath '{}'; Rename-VM -VM $imported -NewName '{}'",
            export_dir, export_dir, export_dir, vm_name
        ))?;
        Ok(vm_name.to_string())
    }

    fn clone_machine(
        &self,
        base_machine_id: &str,
        vm_name: &str,
    ) -> Result<String, MigratoryError> {
        let export_base =
            std::env::temp_dir().join(format!("migratory_export_{}", uuid::Uuid::new_v4()));
        let export_dir = export_base.to_string_lossy();
        self.run_powershell(&format!(
            "Export-VM -Name '{}' -Path '{}'",
            base_machine_id, export_dir
        ))?;
        self.run_powershell(&format!(
            "Import-VM -Path '{}\\{}\\Virtual Machines\\*.vmcx' -Copy -GenerateNewId",
            export_dir, base_machine_id
        ))?;
        self.run_powershell(&format!(
            "Rename-VM -Name (Get-VM -Name '{}' | Select-Object -Skip 1).Name -NewName '{}'",
            base_machine_id, vm_name
        ))?;
        let _ = std::fs::remove_dir_all(&export_base);
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
        let _ = self.run_powershell(&format!("Stop-VM -Name '{}' -TurnOff -Force", id));
        self.run_powershell(&format!("Remove-VM -Name '{}' -Force", id))?;
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
    fn status(&self) -> Result<crate::provider::MachineState, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok(crate::provider::MachineState::NotCreated),
        };
        let out = self
            .run_powershell(&format!("(Get-VM -Name '{}').State", id))
            .unwrap_or_default();
        if out.trim().is_empty() {
            Ok(crate::provider::MachineState::Unknown(
                "unknown".to_string(),
            ))
        } else {
            Ok(crate::provider::MachineState::from_raw(
                &out.trim().to_lowercase(),
            ))
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
        self.run_powershell(&format!("Suspend-VM -Name '{}'", id))?;
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
        self.run_powershell(&format!("Resume-VM -Name '{}'", id))?;
        Ok(())
    }

    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_powershell(&format!(
            "Checkpoint-VM -Name '{}' -SnapshotName '{}'",
            id, name
        ))?;
        Ok(())
    }

    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_powershell(&format!(
            "Restore-VMSnapshot -VMName '{}' -Name '{}' -Confirm:$false",
            id, name
        ))?;
        Ok(())
    }

    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = self
            .run_powershell(&format!(
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

    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_powershell(&format!(
            "Remove-VMSnapshot -VMName '{}' -Name '{}' -Confirm:$false",
            id, name
        ))?;
        Ok(())
    }

    /// Exports the Hyper-V virtual machine and disks to the specified directory.
    ///
    /// # Arguments
    ///
    /// * `output_dir` - Directory where exported VM and disk files are written.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if PowerShell export fails or machine ID is missing.
    fn export(&self, output_dir: &std::path::Path) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let out_dir_str = output_dir.to_string_lossy();
        let cmd = format!("Export-VM -Name '{}' -Path '{}'", id, out_dir_str);
        let _ = self.run_powershell(&cmd)?;
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
        let out = self.run_powershell(&script)?;
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
        self.run_powershell(&script)?;
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
        Self::create_differencing_vhd_with("powershell", parent_path, diff_path)
    }

    /// Creates a differencing VHDX virtual hard disk using the specified PowerShell binary.
    ///
    /// # Arguments
    ///
    /// * `powershell_bin` - Path or binary name for `powershell`.
    /// * `parent_path` - Path to the parent base VHDX disk.
    /// * `diff_path` - Path where the differencing disk should be created.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if creation fails.
    pub fn create_differencing_vhd_with(
        powershell_bin: &str,
        parent_path: &str,
        diff_path: &str,
    ) -> Result<(), MigratoryError> {
        let script = format!(
            "New-VHD -ParentPath '{}' -Path '{}' -Differencing",
            parent_path, diff_path
        );
        execute_powershell_inner(powershell_bin, &script)?;
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
        self.run_powershell(&script)?;
        Ok(())
    }

    /// Saves the VM state to disk (`Stop-VM -Save`).
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if saving fails.
    pub fn save_vm_state(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_powershell(&format!("Stop-VM -Name '{}' -Save", id))?;
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
pub fn execute_powershell(cmdlet: &str) -> Result<String, MigratoryError> {
    execute_powershell_inner("powershell", cmdlet)
}

/// Spawns the `powershell` binary with `-NoProfile -NonInteractive -Command <cmdlet>`.
///
/// # Arguments
///
/// * `cmd` - Executable name or path (`"powershell"`).
/// * `cmdlet` - PowerShell script or cmdlet string to execute.
///
/// # Returns
///
/// Returns the captured standard output string on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if the command cannot be spawned or exits non-zero.
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
    fn test_hyperv_provider_methods() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let bin = temp_dir.path().join("powershell");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = "#!/bin/sh\necho \"snap1\"\nexit 0\n";
            std::fs::write(&bin, mock_script)?;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("powershell.bat");
            std::fs::write(&bat, "@echo off\necho snap1\nexit 0")?;
        }

        let bin_str = bin.to_string_lossy().to_string();
        let provider =
            HypervProvider::new(Some("test-id".to_string())).with_powershell_bin(&bin_str);

        let mut config = crate::config::VmConfig::default();
        config
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/host"),
                guest_path: std::path::PathBuf::from("/"),
                disabled: false,
                folder_type: Some(crate::config::SyncedFolderType::VirtualBox),
                ..Default::default()
            });
        config
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/host2"),
                guest_path: std::path::PathBuf::from("/guest2"),
                disabled: true,
                folder_type: None,
                ..Default::default()
            });

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

        let box_path = std::path::Path::new("/dummy");
        provider.import(box_path, "vm-1")?;
        provider.clone_machine("base-id", "vm-2")?;

        let _ = provider.status()?;

        Ok(())
    }

    #[test]
    fn test_hyperv_status_running() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let bin = temp_dir.path().join("powershell");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = r#"#!/bin/sh
            echo "Running"
            exit 0
            "#;
            std::fs::write(&bin, mock_script)?;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("powershell.bat");
            std::fs::write(&bat, "@echo off\necho Running\nexit 0")?;
        }

        let bin_str = bin.to_string_lossy().to_string();
        let provider =
            HypervProvider::new(Some("test-id".to_string())).with_powershell_bin(&bin_str);
        assert_eq!(provider.status()?, "running");
        provider.export(temp_dir.path())?;

        Ok(())
    }

    #[test]
    fn test_hyperv_provider() -> Result<(), MigratoryError> {
        let provider = HypervProvider::new(Some("test-id".to_string()));
        assert_eq!(provider.name(), "hyperv");
        assert!(provider.status().is_ok());

        let provider_no_id = HypervProvider::new(None);
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
    fn test_hyperv_provider_networks() {
        let provider = HypervProvider::new(Some("test-id".to_string())).with_powershell_bin("echo");
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
        let _ = execute_powershell("echo 1");
        let result = execute_powershell_inner("this_command_does_not_exist_123", "echo 1");
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_powershell_success() {
        let result = execute_powershell_inner("echo", "test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_powershell_failure() {
        let result = execute_powershell_inner("false", "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_hyperv_deep_features() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let bin = temp_dir.path().join("powershell");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = "#!/bin/sh\necho \"192.168.1.50\"\nexit 0\n";
            std::fs::write(&bin, mock_script)?;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("powershell.bat");
            std::fs::write(&bat, "@echo off\necho 192.168.1.50\nexit 0")?;
        }

        let bin_str = bin.to_string_lossy().to_string();
        let provider =
            HypervProvider::new(Some("test-vm".to_string())).with_powershell_bin(&bin_str);
        let ip = provider.get_guest_ip()?;
        assert_eq!(ip, Some("192.168.1.50".to_string()));

        assert!(
            provider
                .create_vm("gen2-vm", 2, 2048, "Default Switch")
                .is_ok()
        );
        let _ = HypervProvider::create_differencing_vhd("base.vhdx", "diff.vhdx");
        assert!(
            HypervProvider::create_differencing_vhd_with(&bin_str, "base.vhdx", "diff.vhdx")
                .is_ok()
        );
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

        Ok(())
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
    fn test_hyperv_empty_outputs() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let bin = temp_dir.path().join("powershell");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = "#!/bin/sh\nexit 0\n";
            std::fs::write(&bin, mock_script)?;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("powershell.bat");
            std::fs::write(&bat, "@echo off\nexit 0")?;
        }

        let bin_str = bin.to_string_lossy().to_string();
        let provider =
            HypervProvider::new(Some("test-vm".to_string())).with_powershell_bin(&bin_str);
        let ip = provider.get_guest_ip()?;
        assert_eq!(ip, None);

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

        Ok(())
    }

    #[test]
    fn test_hyperv_powershell_cmd_failures() -> Result<(), MigratoryError> {
        let temp_dir = tempfile::tempdir()?;
        let bin = temp_dir.path().join("powershell");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mock_script = "#!/bin/sh\nexit 1\n";
            std::fs::write(&bin, mock_script)?;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(windows)]
        {
            let bat = temp_dir.path().join("powershell.bat");
            std::fs::write(&bat, "@echo off\nexit 1")?;
        }

        let bin_str = bin.to_string_lossy().to_string();
        let provider =
            HypervProvider::new(Some("test-vm".to_string())).with_powershell_bin(&bin_str);
        let config = VmConfig::default();

        assert!(provider.up(&config).is_err());

        // Also test up failing in setup_synced_folders
        let mut sf_config = VmConfig::default();
        sf_config
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/host"),
                guest_path: std::path::PathBuf::from("/guest"),
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
        assert!(provider.snapshot_save("snap").is_err());
        assert!(provider.snapshot_restore("snap").is_err());
        assert!(provider.snapshot_delete("snap").is_err());
        assert!(provider.export(temp_dir.path()).is_err());
        assert!(provider.get_guest_ip().is_err());
        assert!(provider.create_vm("vm", 1, 1024, "Switch").is_err());
        assert!(HypervProvider::create_differencing_vhd_with(&bin_str, "base", "diff").is_err());
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
            std::fs::write(&bin, rename_fail_script)?;
        }
        #[cfg(windows)]
        {
            let rename_fail_bat = "@echo off\necho %* | findstr /i \"Rename-VM\" >nul\nif %errorlevel% equ 0 exit /b 1\nexit /b 0\n";
            std::fs::write(temp_dir.path().join("powershell.bat"), rename_fail_bat)?;
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
            std::fs::write(&bin, import_fail_script)?;
        }
        #[cfg(windows)]
        {
            let import_fail_bat = "@echo off\necho %* | findstr /i \"Import-VM\" >nul\nif %errorlevel% equ 0 exit /b 1\nexit /b 0\n";
            std::fs::write(temp_dir.path().join("powershell.bat"), import_fail_bat)?;
        }
        assert!(provider.clone_machine("base", "vm").is_err());

        Ok(())
    }
}
