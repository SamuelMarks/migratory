//! UTM Provider implementation.
//!
//! Provides the virtualization driver for UTM on macOS / Apple Silicon,
//! interfacing with the `utmctl` CLI to manage VM lifecycles, configure hardware,
//! and orchestrate APFS copy-on-write snapshots.

use super::{CommandRunner, Provider, SystemCommandRunner};
use crate::config::VmConfig;
use crate::error::MigratoryError;
use std::sync::Arc;

/// UTM hypervisor provider.
///
/// Implements the [`Provider`] trait to control Apple Silicon and QEMU-backed
/// virtual machines managed by UTM.
pub struct UtmProvider {
    /// Identifier or name of the VM in UTM.
    machine_id: Option<String>,
    /// Injectable command runner used to execute `utmctl`.
    runner: Arc<dyn CommandRunner>,
}

impl UtmProvider {
    /// Creates a new `UtmProvider` instance using the default [`SystemCommandRunner`].
    ///
    /// # Arguments
    ///
    /// * `machine_id` - Optional identifier or name of the VM in UTM.
    ///
    /// # Returns
    ///
    /// Returns a new `UtmProvider`.
    pub fn new(machine_id: Option<String>) -> Self {
        Self::with_runner(machine_id, Arc::new(SystemCommandRunner))
    }

    /// Creates a new `UtmProvider` instance with an injected [`CommandRunner`].
    ///
    /// # Arguments
    ///
    /// * `machine_id` - Optional identifier or name of the VM in UTM.
    /// * `runner` - Command runner implementation.
    ///
    /// # Returns
    ///
    /// Returns a new `UtmProvider`.
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

    /// Executes `utmctl` via the configured [`CommandRunner`].
    ///
    /// # Arguments
    ///
    /// * `args` - Arguments passed to `utmctl`.
    ///
    /// # Returns
    ///
    /// Returns captured stdout on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if execution fails.
    fn run_utmctl(&self, args: &[&str]) -> Result<String, MigratoryError> {
        self.runner.run("utmctl", args)
    }

    /// Checks if UTM is installed and available on the host system.
    ///
    /// # Returns
    ///
    /// Returns `true` if UTM app or `utmctl` binary is found, `false` otherwise.
    pub fn is_available() -> bool {
        Self::is_available_with("utmctl")
    }

    /// Checks if the specified UTM `utmctl` binary is available on the host system.
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

    /// Checks if the specified `utmctl` binary is available using an injected [`CommandRunner`].
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
        runner.is_available(bin, "--help")
    }
}

impl Provider for UtmProvider {
    fn name(&self) -> &'static str {
        "utm"
    }

    fn up(&self, _config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_utmctl(&["start", id])?;
        Ok(())
    }

    fn halt(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_utmctl(&["stop", id])?;
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
        self.run_utmctl(&["clone", base_machine_id, "--name", vm_name])?;
        Ok(vm_name.to_string())
    }

    fn suspend(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_utmctl(&["suspend", id])?;
        Ok(())
    }

    fn resume(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_utmctl(&["start", id])?;
        Ok(())
    }

    fn destroy(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = self.run_utmctl(&["stop", id]);
        self.run_utmctl(&["delete", id])?;
        Ok(())
    }

    fn status(&self) -> Result<crate::provider::MachineState, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok(crate::provider::MachineState::NotCreated),
        };

        let out = self
            .run_utmctl(&["status", id])
            .unwrap_or_else(|_| String::new());
        let trimmed = out.trim();
        if trimmed == "started" {
            Ok(crate::provider::MachineState::Running)
        } else if trimmed == "stopped" {
            Ok(crate::provider::MachineState::PowerOff)
        } else if trimmed == "suspended" {
            Ok(crate::provider::MachineState::Saved)
        } else if trimmed.is_empty() {
            Ok(crate::provider::MachineState::NotCreated)
        } else {
            Ok(crate::provider::MachineState::from_raw(trimmed))
        }
    }

    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let clone_name = format!("{}-snapshot-{}", id, name);
        self.run_utmctl(&["clone", id, "--name", &clone_name])?;
        Ok(())
    }

    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let clone_name = format!("{}-snapshot-{}", id, name);
        let _ = self.run_utmctl(&["stop", id]);
        self.run_utmctl(&["delete", id])?;
        self.run_utmctl(&["clone", &clone_name, "--name", id])?;
        Ok(())
    }

    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let prefix = format!("{}-snapshot-", id);
        let out = self.run_utmctl(&["list"])?;
        let mut list = Vec::new();
        for line in out.lines() {
            let entry = line.split_whitespace().next().unwrap_or("");
            if let Some(snap) = entry.strip_prefix(&prefix)
                && !snap.is_empty()
            {
                list.push(snap.to_string());
            }
        }
        Ok(list)
    }

    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let clone_name = format!("{}-snapshot-{}", id, name);
        self.run_utmctl(&["delete", &clone_name])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::FnCommandRunner;
    use std::path::Path;

    #[test]
    fn test_utm_provider_methods() -> Result<(), MigratoryError> {
        let ok_runner: Arc<dyn CommandRunner> = Arc::new(FnCommandRunner::new(|_cmd, args| {
            if args.contains(&"list") {
                return Ok("test-vm-snapshot-snap1 (started)
test-vm-snapshot-snap2 (stopped)
test-vm-snapshot- (stopped)
other-vm (stopped)
"
                .to_string());
            }
            if args.contains(&"status") {
                return Ok("started".to_string());
            }
            Ok(String::new())
        }));

        let p_none = UtmProvider::with_runner(None, Arc::clone(&ok_runner));
        assert_eq!(p_none.name(), "utm");
        assert!(UtmProvider::is_available_with_runner(
            "utmctl",
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

        let p = UtmProvider::with_runner(Some("test-vm".to_string()), ok_runner);
        assert!(p.up(&VmConfig::default()).is_ok());
        assert!(p.halt().is_ok());
        assert!(p.suspend().is_ok());
        assert!(p.resume().is_ok());
        assert!(p.destroy().is_ok());
        assert_eq!(p.import(Path::new("/dummy"), "vm1")?, "vm1");
        assert_eq!(p.clone_machine("base", "vm2")?, "vm2");
        assert_eq!(p.status()?, "running");

        // Status variants via injected runners
        let p_stopped = UtmProvider::with_runner(
            Some("test-vm".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| Ok("stopped".to_string()))),
        );
        assert_eq!(p_stopped.status()?, "poweroff");

        let p_suspended = UtmProvider::with_runner(
            Some("test-vm".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| Ok("suspended".to_string()))),
        );
        assert_eq!(p_suspended.status()?, "saved");

        let p_empty = UtmProvider::with_runner(
            Some("test-vm".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| Ok(String::new()))),
        );
        assert_eq!(p_empty.status()?, "not created");

        let p_other = UtmProvider::with_runner(
            Some("test-vm".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| Ok("crashed".to_string()))),
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

        // Clone error during snapshot_restore
        let p_clone_err = UtmProvider::with_runner(
            Some("test-vm".to_string()),
            Arc::new(FnCommandRunner::new(|_, args| {
                if args.contains(&"clone") {
                    Err(MigratoryError::Generic("Mock clone error".to_string()))
                } else {
                    Ok(String::new())
                }
            })),
        );
        assert!(p_clone_err.snapshot_restore("snap1").is_err());

        // Error runner
        let err_runner: Arc<dyn CommandRunner> =
            Arc::new(FnCommandRunner::with_availability(false, |_, _| {
                Err(MigratoryError::Generic("Mock utmctl error".to_string()))
            }));
        let p_err = UtmProvider::with_runner(Some("test-vm".to_string()), Arc::clone(&err_runner));
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
        assert!(!UtmProvider::is_available_with_runner(
            "utmctl",
            err_runner.as_ref()
        ));

        // Test real execution branches via SystemCommandRunner
        let _ = UtmProvider::new(None);
        assert!(SystemCommandRunner.run("echo", &["test"]).is_ok());
        assert!(SystemCommandRunner.run("false", &["test"]).is_err());
        assert!(
            SystemCommandRunner
                .run("this_command_does_not_exist_xyz_123", &["test"])
                .is_err()
        );
        assert!(UtmProvider::is_available_with("echo"));
        assert!(!UtmProvider::is_available_with(
            "this_command_does_not_exist_xyz_123"
        ));
        assert_eq!(
            UtmProvider::is_available(),
            UtmProvider::is_available_with("utmctl")
        );
        Ok(())
    }
}
