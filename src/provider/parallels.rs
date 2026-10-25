//! Parallels Desktop Provider implementation.
//!
//! Provides the virtualization driver for Parallels Desktop on macOS,
//! interfacing with the `prlctl` CLI to manage VM lifecycles, configure hardware,
//! and orchestrate VM snapshots.

use super::{CommandRunner, Provider, SystemCommandRunner};
use crate::config::VmConfig;
use crate::error::MigratoryError;
use std::sync::Arc;

/// Parallels Desktop hypervisor provider.
///
/// Implements the [`Provider`] trait to control virtual machines running under Parallels Desktop.
pub struct ParallelsProvider {
    /// Machine identifier (VM name or UUID) in Parallels Desktop.
    machine_id: Option<String>,
    /// Injectable command runner used to execute `prlctl`.
    runner: Arc<dyn CommandRunner>,
}

impl ParallelsProvider {
    /// Creates a new `ParallelsProvider` instance using the default [`SystemCommandRunner`].
    ///
    /// # Arguments
    ///
    /// * `machine_id` - Optional VM name or UUID in Parallels Desktop.
    ///
    /// # Returns
    ///
    /// Returns a new `ParallelsProvider`.
    pub fn new(machine_id: Option<String>) -> Self {
        Self::with_runner(machine_id, Arc::new(SystemCommandRunner))
    }

    /// Creates a new `ParallelsProvider` instance with an injected [`CommandRunner`].
    ///
    /// # Arguments
    ///
    /// * `machine_id` - Optional VM name or UUID in Parallels Desktop.
    /// * `runner` - Command runner implementation.
    ///
    /// # Returns
    ///
    /// Returns a new `ParallelsProvider`.
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

    /// Executes `prlctl` via the configured [`CommandRunner`].
    ///
    /// # Arguments
    ///
    /// * `args` - Arguments passed to `prlctl`.
    ///
    /// # Returns
    ///
    /// Returns captured stdout on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if execution fails.
    fn run_prlctl(&self, args: &[&str]) -> Result<String, MigratoryError> {
        self.runner.run("prlctl", args)
    }

    /// Checks if Parallels Desktop is installed and available on the host system.
    ///
    /// # Returns
    ///
    /// Returns `true` if `prlctl` binary is available, `false` otherwise.
    pub fn is_available() -> bool {
        Self::is_available_with("prlctl")
    }

    /// Checks if the specified Parallels `prlctl` binary is available on the host system.
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

    /// Checks if the specified `prlctl` binary is available using an injected [`CommandRunner`].
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
        runner.is_available(bin, "--version")
    }
}

impl Provider for ParallelsProvider {
    fn name(&self) -> &'static str {
        "parallels"
    }

    fn up(&self, _config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_prlctl(&["start", id])?;
        Ok(())
    }

    fn halt(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_prlctl(&["stop", id])?;
        Ok(())
    }

    fn import(&self, box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError> {
        self.run_prlctl(&["register", &box_dir.to_string_lossy()])?;
        Ok(vm_name.to_string())
    }

    fn clone_machine(
        &self,
        base_machine_id: &str,
        vm_name: &str,
    ) -> Result<String, MigratoryError> {
        self.run_prlctl(&["clone", base_machine_id, "--name", vm_name, "--linked"])?;
        Ok(vm_name.to_string())
    }

    fn suspend(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_prlctl(&["suspend", id])?;
        Ok(())
    }

    fn resume(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_prlctl(&["resume", id])?;
        Ok(())
    }

    fn destroy(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = self.run_prlctl(&["stop", id, "--kill"]);
        self.run_prlctl(&["delete", id])?;
        Ok(())
    }

    fn status(&self) -> Result<crate::provider::MachineState, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok(crate::provider::MachineState::NotCreated),
        };

        let out = self
            .run_prlctl(&["status", id])
            .unwrap_or_else(|_| String::new());
        let trimmed = out.trim();
        if trimmed == "running" {
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
        self.run_prlctl(&["snapshot", id, "-n", name])?;
        Ok(())
    }

    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_prlctl(&["snapshot-switch", id, "-i", name])?;
        Ok(())
    }

    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = self.run_prlctl(&["snapshot-list", id, "-t"])?;
        let mut list = Vec::new();
        for line in out.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(name) = parts.last()
                && !name.starts_with('{')
            {
                list.push((*name).to_string());
            }
        }
        Ok(list)
    }

    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_prlctl(&["snapshot-delete", id, "-i", name])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::FnCommandRunner;

    #[test]
    fn test_parallels_provider_methods() -> Result<(), MigratoryError> {
        let ok_runner: Arc<dyn CommandRunner> = Arc::new(FnCommandRunner::new(|_cmd, args| {
            if args.contains(&"status") {
                return Ok("running
"
                .to_string());
            }
            if args.contains(&"snapshot-list") {
                return Ok(" {uuid-1} * snap1
 {uuid-2}   snap2
"
                .to_string());
            }
            Ok(String::new())
        }));

        let p_none = ParallelsProvider::with_runner(None, Arc::clone(&ok_runner));
        assert_eq!(p_none.name(), "parallels");
        assert!(ParallelsProvider::is_available_with_runner(
            "prlctl",
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

        let p = ParallelsProvider::with_runner(Some("test-prl".to_string()), ok_runner);
        assert!(p.up(&VmConfig::default()).is_ok());
        assert!(p.halt().is_ok());
        assert!(p.suspend().is_ok());
        assert!(p.resume().is_ok());
        assert!(p.destroy().is_ok());
        assert_eq!(p.import(std::path::Path::new("/dummy"), "vm1")?, "vm1");
        assert_eq!(p.clone_machine("base", "vm2")?, "vm2");
        assert_eq!(p.status()?, "running");

        // Status variants via injected runners
        let p_stopped = ParallelsProvider::with_runner(
            Some("test-prl".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| {
                Ok("stopped
"
                .to_string())
            })),
        );
        assert_eq!(p_stopped.status()?, "poweroff");

        let p_suspended = ParallelsProvider::with_runner(
            Some("test-prl".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| {
                Ok("suspended
"
                .to_string())
            })),
        );
        assert_eq!(p_suspended.status()?, "saved");

        let p_empty = ParallelsProvider::with_runner(
            Some("test-prl".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| Ok(String::new()))),
        );
        assert_eq!(p_empty.status()?, "not created");

        let p_other = ParallelsProvider::with_runner(
            Some("test-prl".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| {
                Ok("paused
"
                .to_string())
            })),
        );
        assert_eq!(p_other.status()?, "paused");

        // Snapshots
        assert!(p.snapshot_save("snap1").is_ok());
        assert!(p.snapshot_restore("snap1").is_ok());
        assert!(p.snapshot_delete("snap1").is_ok());
        let list = p.snapshot_list()?;
        assert_eq!(list, vec!["snap1", "snap2"]);

        let list_empty = p_empty.snapshot_list()?;
        assert!(list_empty.is_empty());

        let p_skip = ParallelsProvider::with_runner(
            Some("test-prl".to_string()),
            Arc::new(FnCommandRunner::new(|_, _| {
                Ok(" {uuid-only}
 
"
                .to_string())
            })),
        );
        assert!(p_skip.snapshot_list()?.is_empty());

        let err_runner: Arc<dyn CommandRunner> =
            Arc::new(FnCommandRunner::with_availability(false, |_, _| {
                Err(MigratoryError::Generic("Mock prlctl error".to_string()))
            }));
        let p_err =
            ParallelsProvider::with_runner(Some("test-prl".to_string()), Arc::clone(&err_runner));
        assert_eq!(p_err.status()?, "not created");
        assert!(p_err.up(&VmConfig::default()).is_err());
        assert!(p_err.halt().is_err());
        assert!(p_err.suspend().is_err());
        assert!(p_err.resume().is_err());
        assert!(p_err.destroy().is_err());
        assert!(p_err.import(std::path::Path::new("/dummy"), "vm").is_err());
        assert!(p_err.clone_machine("base", "vm").is_err());
        assert!(p_err.snapshot_save("s1").is_err());
        assert!(p_err.snapshot_restore("s1").is_err());
        assert!(p_err.snapshot_list().is_err());
        assert!(p_err.snapshot_delete("s1").is_err());
        assert!(!ParallelsProvider::is_available_with_runner(
            "prlctl",
            err_runner.as_ref()
        ));

        // Test real execution branches via SystemCommandRunner
        let _ = ParallelsProvider::new(None);
        assert!(SystemCommandRunner.run("echo", &["test"]).is_ok());
        assert!(SystemCommandRunner.run("false", &["test"]).is_err());
        assert!(
            SystemCommandRunner
                .run("this_command_does_not_exist_xyz_123", &["test"])
                .is_err()
        );
        assert!(ParallelsProvider::is_available_with("echo"));
        assert!(!ParallelsProvider::is_available_with(
            "this_command_does_not_exist_xyz_123"
        ));
        assert_eq!(
            ParallelsProvider::is_available(),
            ParallelsProvider::is_available_with("prlctl")
        );
        Ok(())
    }
}
