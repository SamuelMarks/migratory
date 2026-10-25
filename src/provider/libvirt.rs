//! Libvirt Provider implementation.
//!
//! Provides the virtualization driver for Libvirt / KVM,
//! interfacing with the `virsh` CLI to manage domain lifecycles, configure hardware,
//! and orchestrate domain snapshots.

use super::{CommandRunner, Provider, SystemCommandRunner};
use crate::config::VmConfig;
use crate::error::MigratoryError;
use std::sync::Arc;

/// Libvirt / KVM hypervisor provider.
///
/// Implements the [`Provider`] trait to control KVM-backed virtual machines managed by Libvirt.
pub struct LibvirtProvider {
    /// Domain name or UUID of the machine in Libvirt.
    machine_id: Option<String>,
    /// Injectable command runner used to execute `virsh`.
    runner: Arc<dyn CommandRunner>,
}

impl LibvirtProvider {
    /// Creates a new `LibvirtProvider` instance using the default [`SystemCommandRunner`].
    ///
    /// # Arguments
    ///
    /// * `machine_id` - Optional domain name or UUID of the machine in Libvirt.
    ///
    /// # Returns
    ///
    /// Returns a new `LibvirtProvider`.
    pub fn new(machine_id: Option<String>) -> Self {
        Self::with_runner(machine_id, Arc::new(SystemCommandRunner))
    }

    /// Creates a new `LibvirtProvider` instance with an injected [`CommandRunner`].
    ///
    /// # Arguments
    ///
    /// * `machine_id` - Optional domain name or UUID of the machine in Libvirt.
    /// * `runner` - Command runner implementation.
    ///
    /// # Returns
    ///
    /// Returns a new `LibvirtProvider`.
    pub fn with_runner(machine_id: Option<String>, runner: Arc<dyn CommandRunner>) -> Self {
        Self { machine_id, runner }
    }

    /// Helper to get the required machine ID.
    ///
    /// # Returns
    ///
    /// Returns the machine ID string slice on success.
    ///
    /// # Errors
    ///
    /// Returns [`MigratoryError::Generic`] if the machine is not created.
    fn require_id(&self) -> Result<&str, MigratoryError> {
        self.machine_id.as_deref().ok_or_else(|| {
            MigratoryError::Generic("Machine not created or ID not found".to_string())
        })
    }

    /// Executes `virsh` via the configured [`CommandRunner`].
    ///
    /// # Arguments
    ///
    /// * `args` - Arguments passed to `virsh`.
    ///
    /// # Returns
    ///
    /// Returns captured stdout on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if execution fails.
    fn run_virsh(&self, args: &[&str]) -> Result<String, MigratoryError> {
        self.runner.run("virsh", args)
    }

    /// Checks if Libvirt is installed and available on the host system.
    ///
    /// # Returns
    ///
    /// Returns `true` if `virsh` binary is available, `false` otherwise.
    pub fn is_available() -> bool {
        Self::is_available_with("virsh")
    }

    /// Checks if the specified Libvirt `virsh` binary is available on the host system.
    ///
    /// # Arguments
    ///
    /// * `bin` - Executable name or path to check.
    ///
    /// # Returns
    ///
    /// Returns `true` if `bin` can be spawned, `false` otherwise.
    pub fn is_available_with(bin: &str) -> bool {
        Self::is_available_with_runner(bin, &SystemCommandRunner)
    }

    /// Checks if the specified `virsh` binary is available using an injected [`CommandRunner`].
    ///
    /// # Arguments
    ///
    /// * `bin` - Executable name or path to check.
    /// * `runner` - Command runner used to probe availability.
    ///
    /// # Returns
    ///
    /// Returns `true` if available, `false` otherwise.
    pub fn is_available_with_runner(bin: &str, runner: &dyn CommandRunner) -> bool {
        runner.is_available(bin, "-v")
    }
}

impl Provider for LibvirtProvider {
    fn name(&self) -> &'static str {
        "libvirt"
    }

    fn up(&self, _config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_virsh(&["start", id])?;
        Ok(())
    }

    fn halt(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_virsh(&["shutdown", id])?;
        Ok(())
    }

    fn import(&self, _box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError> {
        Ok(vm_name.to_string())
    }

    fn clone_machine(
        &self,
        base_machine_id: &str,
        vm_name: &str,
    ) -> Result<String, MigratoryError> {
        self.run_virsh(&[
            "clone",
            "--original",
            base_machine_id,
            "--name",
            vm_name,
            "--auto-clone",
        ])?;
        Ok(vm_name.to_string())
    }

    fn suspend(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_virsh(&["suspend", id])?;
        Ok(())
    }

    fn resume(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_virsh(&["resume", id])?;
        Ok(())
    }

    fn destroy(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = self.run_virsh(&["destroy", id]);
        self.run_virsh(&["undefine", id, "--remove-all-storage"])?;
        Ok(())
    }

    fn status(&self) -> Result<crate::provider::MachineState, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok(crate::provider::MachineState::NotCreated),
        };

        let out = self
            .run_virsh(&["domstate", id])
            .unwrap_or_else(|_| String::new());
        let trimmed = out.trim();
        if trimmed == "running" {
            Ok(crate::provider::MachineState::Running)
        } else if trimmed == "shut off" {
            Ok(crate::provider::MachineState::PowerOff)
        } else if trimmed == "paused" {
            Ok(crate::provider::MachineState::Saved)
        } else if trimmed.is_empty() {
            Ok(crate::provider::MachineState::NotCreated)
        } else {
            Ok(crate::provider::MachineState::from_raw(trimmed))
        }
    }

    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_virsh(&["snapshot-create-as", id, name])?;
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
        let list = out
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        Ok(list)
    }

    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_virsh(&["snapshot-delete", id, name])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::FnCommandRunner;

    #[test]
    fn test_libvirt_provider_methods() -> Result<(), MigratoryError> {
        let ok_runner: Arc<dyn CommandRunner> = Arc::new(FnCommandRunner::new(|_cmd, args| {
            if args.contains(&"domstate") {
                return Ok("running
"
                .to_string());
            }
            if args.contains(&"snapshot-list") {
                return Ok("snap1
snap2
"
                .to_string());
            }
            Ok(String::new())
        }));

        let p_none = LibvirtProvider::with_runner(None, Arc::clone(&ok_runner));
        assert_eq!(p_none.name(), "libvirt");
        assert!(LibvirtProvider::is_available_with_runner(
            "virsh",
            ok_runner.as_ref()
        ));
        assert_eq!(p_none.status()?, "not created");
        assert!(p_none.up(&VmConfig::default()).is_err());
        assert!(p_none.halt().is_err());
        assert!(p_none.suspend().is_err());
        assert!(p_none.resume().is_err());
        assert!(p_none.destroy().is_err());
        assert!(p_none.snapshot_save("s1").is_err());
        assert!(p_none.snapshot_restore("s1").is_err());
        assert!(p_none.snapshot_list().is_err());
        assert!(p_none.snapshot_delete("s1").is_err());

        let p = LibvirtProvider::with_runner(Some("test-domain".to_string()), ok_runner);
        assert!(p.up(&VmConfig::default()).is_ok());
        assert!(p.halt().is_ok());
        assert!(p.suspend().is_ok());
        assert!(p.resume().is_ok());
        assert!(p.destroy().is_ok());
        assert_eq!(p.import(std::path::Path::new("/dummy"), "vm1")?, "vm1");
        assert_eq!(p.clone_machine("base", "vm2")?, "vm2");
        assert_eq!(p.status()?, "running");

        // Status variants via injected runners
        let p_off = LibvirtProvider::with_runner(
            Some("test-domain".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| {
                Ok("shut off
"
                .to_string())
            })),
        );
        assert_eq!(p_off.status()?, "poweroff");

        let p_saved = LibvirtProvider::with_runner(
            Some("test-domain".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| {
                Ok("paused
"
                .to_string())
            })),
        );
        assert_eq!(p_saved.status()?, "saved");

        let p_empty = LibvirtProvider::with_runner(
            Some("test-domain".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| Ok(String::new()))),
        );
        assert_eq!(p_empty.status()?, "not created");

        let p_other = LibvirtProvider::with_runner(
            Some("test-domain".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| {
                Ok("crashed
"
                .to_string())
            })),
        );
        assert_eq!(p_other.status()?, "crashed");

        // Snapshots
        assert!(p.snapshot_save("snap1").is_ok());
        assert!(p.snapshot_restore("snap1").is_ok());
        assert!(p.snapshot_delete("snap1").is_ok());
        let list = p.snapshot_list()?;
        assert_eq!(list, vec!["snap1", "snap2"]);

        let list_empty = p_empty.snapshot_list()?;
        assert!(list_empty.is_empty());

        let err_runner: Arc<dyn CommandRunner> =
            Arc::new(FnCommandRunner::with_availability(false, |_, _| {
                Err(MigratoryError::Generic("Mock virsh error".to_string()))
            }));
        let p_err =
            LibvirtProvider::with_runner(Some("test-domain".to_string()), Arc::clone(&err_runner));
        assert_eq!(p_err.status()?, "not created");
        assert!(p_err.up(&VmConfig::default()).is_err());
        assert!(p_err.halt().is_err());
        assert!(p_err.suspend().is_err());
        assert!(p_err.resume().is_err());
        assert!(p_err.destroy().is_err());
        assert!(p_err.clone_machine("base", "vm").is_err());
        assert!(p_err.snapshot_save("s1").is_err());
        assert!(p_err.snapshot_restore("s1").is_err());
        assert!(p_err.snapshot_list().is_err());
        assert!(p_err.snapshot_delete("s1").is_err());
        assert!(!LibvirtProvider::is_available_with_runner(
            "virsh",
            err_runner.as_ref()
        ));

        // Test real execution branches via SystemCommandRunner
        let _ = LibvirtProvider::new(None);
        assert!(SystemCommandRunner.run("echo", &["test"]).is_ok());
        assert!(SystemCommandRunner.run("false", &["test"]).is_err());
        assert!(
            SystemCommandRunner
                .run("this_command_does_not_exist_xyz_123", &["test"])
                .is_err()
        );
        assert!(LibvirtProvider::is_available_with("echo"));
        assert!(!LibvirtProvider::is_available_with(
            "this_command_does_not_exist_xyz_123"
        ));
        assert_eq!(
            LibvirtProvider::is_available(),
            LibvirtProvider::is_available_with("virsh")
        );
        Ok(())
    }
}
