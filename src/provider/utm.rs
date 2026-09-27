//! UTM Provider implementation.
//!
//! Provides the virtualization driver for UTM on macOS / Apple Silicon,
//! interfacing with the `utmctl` CLI to manage VM lifecycles, configure hardware,
//! and orchestrate APFS copy-on-write snapshots.

use super::Provider;
use crate::config::VmConfig;
use crate::error::MigratoryError;
use std::process::Command;

/// UTM hypervisor provider.
///
/// Implements the [`Provider`] trait to control Apple Silicon and QEMU-backed
/// virtual machines managed by UTM.
pub struct UtmProvider {
    /// Identifier or name of the VM in UTM.
    machine_id: Option<String>,
}

impl UtmProvider {
    /// Creates a new `UtmProvider` instance.
    ///
    /// # Arguments
    ///
    /// * `machine_id` - Optional identifier or name of the VM in UTM.
    ///
    /// # Returns
    ///
    /// Returns a new `UtmProvider`.
    ///
    /// # Panics
    ///
    /// Never panics.
    pub fn new(machine_id: Option<String>) -> Self {
        Self { machine_id }
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
    ///
    /// # Panics
    ///
    /// Never panics.
    fn require_id(&self) -> Result<&str, MigratoryError> {
        self.machine_id.as_deref().ok_or_else(|| {
            MigratoryError::Generic("Machine not created or ID not found".to_string())
        })
    }

    /// Checks if UTM is installed and available on the host system.
    ///
    /// # Returns
    ///
    /// Returns `true` if UTM app or `utmctl` binary is found, `false` otherwise.
    ///
    /// # Panics
    ///
    /// Never panics.
    pub fn is_available() -> bool {
        if std::env::var("MIGRATORY_TEST_MOCK_UTMCTL").is_ok() {
            return std::env::var("MIGRATORY_TEST_MOCK_UTMCTL_UNAVAILABLE").is_err();
        }
        Command::new("utmctl").arg("--help").output().is_ok()
    }
}

/// Helper function to execute `utmctl` commands with mock support.
///
/// # Arguments
///
/// * `args` - Command-line arguments to pass to `utmctl`.
///
/// # Returns
///
/// Returns the standard output string on success.
///
/// # Errors
///
/// Returns [`MigratoryError`] if command execution fails.
///
/// # Panics
///
/// Never panics.
pub(crate) fn execute_utmctl(args: &[&str]) -> Result<String, MigratoryError> {
    execute_utmctl_inner("utmctl", args)
}

fn execute_utmctl_inner(cmd: &str, args: &[&str]) -> Result<String, MigratoryError> {
    if std::env::var("MIGRATORY_TEST_MOCK_UTMCTL").is_ok() {
        if std::env::var("MIGRATORY_TEST_MOCK_UTMCTL_ERROR").is_ok() {
            return Err(MigratoryError::Generic("Mock utmctl error".to_string()));
        }
        if args.contains(&"clone")
            && std::env::var("MIGRATORY_TEST_MOCK_UTMCTL_CLONE_ERROR").is_ok()
        {
            return Err(MigratoryError::Generic("Mock clone error".to_string()));
        }
        if args.contains(&"list") {
            if std::env::var("MIGRATORY_TEST_MOCK_UTMCTL_LIST_EMPTY").is_ok() {
                return Ok(String::new());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_UTMCTL_SNAPSHOT_LIST").is_ok() {
                return Ok("test-vm-snapshot-snap1 (started)
test-vm-snapshot-snap2 (stopped)
test-vm-snapshot- (stopped)
other-vm (stopped)
"
                .to_string());
            }
            return Ok("test-vm (started)
stopped-vm (stopped)
suspended-vm (suspended)
"
            .to_string());
        }
        if args.contains(&"status") {
            if std::env::var("MIGRATORY_TEST_MOCK_RUNNING").is_ok() {
                return Ok("started".to_string());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_POWEROFF").is_ok() {
                return Ok("stopped".to_string());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_SAVED").is_ok() {
                return Ok("suspended".to_string());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_UTMCTL_STATUS_EMPTY").is_ok() {
                return Ok(String::new());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_UTMCTL_STATUS_OTHER").is_ok() {
                return Ok("crashed".to_string());
            }
            return Ok("started".to_string());
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

impl Provider for UtmProvider {
    fn name(&self) -> &'static str {
        "utm"
    }

    fn up(&self, _config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_utmctl(&["start", id])?;
        Ok(())
    }

    fn halt(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_utmctl(&["stop", id])?;
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
        execute_utmctl(&["clone", base_machine_id, "--name", vm_name])?;
        Ok(vm_name.to_string())
    }

    fn suspend(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_utmctl(&["suspend", id])?;
        Ok(())
    }

    fn resume(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_utmctl(&["start", id])?;
        Ok(())
    }

    fn destroy(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = execute_utmctl(&["stop", id]);
        execute_utmctl(&["delete", id])?;
        Ok(())
    }

    fn status(&self) -> Result<String, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok("not created".to_string()),
        };

        let out = execute_utmctl(&["status", id]).unwrap_or_else(|_| String::new());
        let trimmed = out.trim();
        if trimmed == "started" {
            Ok("running".to_string())
        } else if trimmed == "stopped" {
            Ok("poweroff".to_string())
        } else if trimmed == "suspended" {
            Ok("saved".to_string())
        } else if trimmed.is_empty() {
            Ok("not created".to_string())
        } else {
            Ok(trimmed.to_string())
        }
    }

    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let clone_name = format!("{}-snapshot-{}", id, name);
        execute_utmctl(&["clone", id, "--name", &clone_name])?;
        Ok(())
    }

    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let clone_name = format!("{}-snapshot-{}", id, name);
        let _ = execute_utmctl(&["stop", id]);
        execute_utmctl(&["delete", id])?;
        execute_utmctl(&["clone", &clone_name, "--name", id])?;
        Ok(())
    }

    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let prefix = format!("{}-snapshot-", id);
        let out = execute_utmctl(&["list"])?;
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
        execute_utmctl(&["delete", &clone_name])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_utm_provider_methods() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_UTMCTL", "1");
        }

        let p_none = UtmProvider::new(None);
        assert_eq!(p_none.name(), "utm");
        assert!(UtmProvider::is_available());
        assert_eq!(p_none.status().expect("status ok"), "not created");
        assert!(p_none.up(&VmConfig::default()).is_err());
        assert!(p_none.halt().is_err());
        assert!(p_none.suspend().is_err());
        assert!(p_none.resume().is_err());
        assert!(p_none.destroy().is_err());
        assert!(p_none.snapshot_save("s1").is_err());
        assert!(p_none.snapshot_restore("s1").is_err());
        assert!(p_none.snapshot_list().is_err());
        assert!(p_none.snapshot_delete("s1").is_err());

        let p = UtmProvider::new(Some("test-vm".to_string()));
        assert!(p.up(&VmConfig::default()).is_ok());
        assert!(p.halt().is_ok());
        assert!(p.suspend().is_ok());
        assert!(p.resume().is_ok());
        assert!(p.destroy().is_ok());
        assert_eq!(
            p.import(Path::new("/dummy"), "vm1").expect("import ok"),
            "vm1"
        );
        assert_eq!(p.clone_machine("base", "vm2").expect("clone ok"), "vm2");

        // Status tests - fallback started without explicit env var
        assert_eq!(p.status().expect("status ok"), "running");

        // Test list default output
        let list_default = execute_utmctl(&["list"]).expect("list ok");
        assert!(list_default.contains("test-vm (started)"));

        // Test status mappings
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }
        assert_eq!(p.status().expect("status ok"), "running");

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
            std::env::set_var("MIGRATORY_TEST_MOCK_POWEROFF", "1");
        }
        assert_eq!(p.status().expect("status ok"), "poweroff");

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_POWEROFF");
            std::env::set_var("MIGRATORY_TEST_MOCK_SAVED", "1");
        }
        assert_eq!(p.status().expect("status ok"), "saved");

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_SAVED");
            std::env::set_var("MIGRATORY_TEST_MOCK_UTMCTL_STATUS_EMPTY", "1");
        }
        assert_eq!(p.status().expect("status ok"), "not created");

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_UTMCTL_STATUS_EMPTY");
            std::env::set_var("MIGRATORY_TEST_MOCK_UTMCTL_STATUS_OTHER", "1");
        }
        assert_eq!(p.status().expect("status ok"), "crashed");

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_UTMCTL_STATUS_OTHER");
        }

        // Test snapshots
        assert!(p.snapshot_save("snap1").is_ok());
        assert!(p.snapshot_restore("snap1").is_ok());
        assert!(p.snapshot_delete("snap1").is_ok());

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_UTMCTL_CLONE_ERROR", "1");
        }
        assert!(p.snapshot_restore("snap1").is_err());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_UTMCTL_CLONE_ERROR");
        }

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_UTMCTL_SNAPSHOT_LIST", "1");
        }
        let list = p.snapshot_list().expect("list ok");
        assert_eq!(list, vec!["snap1", "snap2"]);

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_UTMCTL_SNAPSHOT_LIST");
            std::env::set_var("MIGRATORY_TEST_MOCK_UTMCTL_LIST_EMPTY", "1");
        }
        let list_empty = p.snapshot_list().expect("list ok");
        assert!(list_empty.is_empty());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_UTMCTL_LIST_EMPTY");
            std::env::set_var("MIGRATORY_TEST_MOCK_UTMCTL_ERROR", "1");
        }
        assert_eq!(p.status().expect("status ok"), "not created");
        assert!(p.up(&VmConfig::default()).is_err());
        assert!(p.halt().is_err());
        assert!(p.suspend().is_err());
        assert!(p.resume().is_err());
        assert!(p.destroy().is_err());
        assert!(p.clone_machine("base", "vm").is_err());
        assert!(p.snapshot_save("s1").is_err());
        assert!(p.snapshot_restore("s1").is_err());
        assert!(p.snapshot_list().is_err());
        assert!(p.snapshot_delete("s1").is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_UTMCTL_ERROR");
            std::env::set_var("MIGRATORY_TEST_MOCK_UTMCTL_UNAVAILABLE", "1");
        }
        assert!(!UtmProvider::is_available());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_UTMCTL_UNAVAILABLE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_UTMCTL");
        }

        // Test real execution branches when mock is unset
        assert!(execute_utmctl_inner("echo", &["test"]).is_ok());
        assert!(execute_utmctl_inner("false", &["test"]).is_err());
        assert!(execute_utmctl_inner("this_command_does_not_exist_xyz_123", &["test"]).is_err());
        let _ = UtmProvider::is_available();
    }
}
