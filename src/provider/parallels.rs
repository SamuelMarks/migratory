//! Parallels Desktop Provider implementation.
//!
//! Provides the virtualization driver for Parallels Desktop on macOS,
//! interfacing with the `prlctl` CLI to manage VM lifecycles, configure hardware,
//! and orchestrate VM snapshots.

use super::Provider;
use crate::config::VmConfig;
use crate::error::MigratoryError;
use std::process::Command;

/// Parallels Desktop hypervisor provider.
///
/// Implements the [`Provider`] trait to control virtual machines running under Parallels Desktop.
pub struct ParallelsProvider {
    /// Machine identifier (VM name or UUID) in Parallels Desktop.
    machine_id: Option<String>,
}

impl ParallelsProvider {
    /// Creates a new `ParallelsProvider` instance.
    ///
    /// # Arguments
    ///
    /// * `machine_id` - Optional VM name or UUID in Parallels Desktop.
    ///
    /// # Returns
    ///
    /// Returns a new `ParallelsProvider`.
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

    /// Checks if Parallels Desktop is installed and available on the host system.
    ///
    /// # Returns
    ///
    /// Returns `true` if `prlctl` binary is available, `false` otherwise.
    ///
    /// # Panics
    ///
    /// Never panics.
    pub fn is_available() -> bool {
        if std::env::var("MIGRATORY_TEST_MOCK_PRLCTL").is_ok() {
            return std::env::var("MIGRATORY_TEST_MOCK_PRLCTL_UNAVAILABLE").is_err();
        }
        Command::new("prlctl").arg("--version").output().is_ok()
    }
}

/// Helper function to execute `prlctl` commands with mock support.
///
/// # Arguments
///
/// * `args` - Command-line arguments to pass to `prlctl`.
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
pub(crate) fn execute_prlctl(args: &[&str]) -> Result<String, MigratoryError> {
    execute_prlctl_inner("prlctl", args)
}

fn execute_prlctl_inner(cmd: &str, args: &[&str]) -> Result<String, MigratoryError> {
    if std::env::var("MIGRATORY_TEST_MOCK_PRLCTL").is_ok() {
        if std::env::var("MIGRATORY_TEST_MOCK_PRLCTL_ERROR").is_ok() {
            return Err(MigratoryError::Generic("Mock prlctl error".to_string()));
        }
        if args.contains(&"status") {
            if std::env::var("MIGRATORY_TEST_MOCK_RUNNING").is_ok() {
                return Ok("running
"
                .to_string());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_POWEROFF").is_ok() {
                return Ok("stopped
"
                .to_string());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_SAVED").is_ok() {
                return Ok("suspended
"
                .to_string());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_PRLCTL_STATUS_EMPTY").is_ok() {
                return Ok(String::new());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_PRLCTL_STATUS_OTHER").is_ok() {
                return Ok("paused
"
                .to_string());
            }
            return Ok("running
"
            .to_string());
        }
        if args.contains(&"snapshot-list") {
            if std::env::var("MIGRATORY_TEST_MOCK_PRLCTL_SNAPSHOT_EMPTY").is_ok() {
                return Ok(String::new());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_PRLCTL_SNAPSHOT_SKIP").is_ok() {
                return Ok(" {uuid-only}
 
"
                .to_string());
            }
            return Ok(" {uuid-1} * snap1
 {uuid-2}   snap2
"
            .to_string());
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

impl Provider for ParallelsProvider {
    fn name(&self) -> &'static str {
        "parallels"
    }

    fn up(&self, _config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_prlctl(&["start", id])?;
        Ok(())
    }

    fn halt(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_prlctl(&["stop", id])?;
        Ok(())
    }

    fn import(&self, box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError> {
        execute_prlctl(&["register", &box_dir.to_string_lossy()])?;
        Ok(vm_name.to_string())
    }

    fn clone_machine(
        &self,
        base_machine_id: &str,
        vm_name: &str,
    ) -> Result<String, MigratoryError> {
        execute_prlctl(&["clone", base_machine_id, "--name", vm_name, "--linked"])?;
        Ok(vm_name.to_string())
    }

    fn suspend(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_prlctl(&["suspend", id])?;
        Ok(())
    }

    fn resume(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_prlctl(&["resume", id])?;
        Ok(())
    }

    fn destroy(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = execute_prlctl(&["stop", id, "--kill"]);
        execute_prlctl(&["delete", id])?;
        Ok(())
    }

    fn status(&self) -> Result<String, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok("not created".to_string()),
        };

        let out = execute_prlctl(&["status", id]).unwrap_or_else(|_| String::new());
        let trimmed = out.trim();
        if trimmed == "running" {
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
        execute_prlctl(&["snapshot", id, "-n", name])?;
        Ok(())
    }

    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_prlctl(&["snapshot-switch", id, "-i", name])?;
        Ok(())
    }

    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = execute_prlctl(&["snapshot-list", id, "-t"])?;
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
        execute_prlctl(&["snapshot-delete", id, "-i", name])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallels_provider_methods() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PRLCTL", "1");
        }

        let p_none = ParallelsProvider::new(None);
        assert_eq!(p_none.name(), "parallels");
        assert!(ParallelsProvider::is_available());
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

        let p = ParallelsProvider::new(Some("test-prl".to_string()));
        assert!(p.up(&VmConfig::default()).is_ok());
        assert!(p.halt().is_ok());
        assert!(p.suspend().is_ok());
        assert!(p.resume().is_ok());
        assert!(p.destroy().is_ok());
        assert_eq!(
            p.import(std::path::Path::new("/dummy"), "vm1")
                .expect("import ok"),
            "vm1"
        );
        assert_eq!(p.clone_machine("base", "vm2").expect("clone ok"), "vm2");

        // Status tests - fallback running without explicit env var
        assert_eq!(p.status().expect("status ok"), "running");

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
            std::env::set_var("MIGRATORY_TEST_MOCK_PRLCTL_STATUS_EMPTY", "1");
        }
        assert_eq!(p.status().expect("status ok"), "not created");

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PRLCTL_STATUS_EMPTY");
            std::env::set_var("MIGRATORY_TEST_MOCK_PRLCTL_STATUS_OTHER", "1");
        }
        assert_eq!(p.status().expect("status ok"), "paused");

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PRLCTL_STATUS_OTHER");
        }

        // Snapshots
        assert!(p.snapshot_save("snap1").is_ok());
        assert!(p.snapshot_restore("snap1").is_ok());
        assert!(p.snapshot_delete("snap1").is_ok());
        let list = p.snapshot_list().expect("list ok");
        assert_eq!(list, vec!["snap1", "snap2"]);

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PRLCTL_SNAPSHOT_EMPTY", "1");
        }
        let list_empty = p.snapshot_list().expect("list ok");
        assert!(list_empty.is_empty());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PRLCTL_SNAPSHOT_EMPTY");
            std::env::set_var("MIGRATORY_TEST_MOCK_PRLCTL_SNAPSHOT_SKIP", "1");
        }
        let list_skip = p.snapshot_list().expect("list ok");
        assert!(list_skip.is_empty());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PRLCTL_SNAPSHOT_SKIP");
            std::env::set_var("MIGRATORY_TEST_MOCK_PRLCTL_ERROR", "1");
        }
        assert_eq!(p.status().expect("status ok"), "not created");
        assert!(p.up(&VmConfig::default()).is_err());
        assert!(p.halt().is_err());
        assert!(p.suspend().is_err());
        assert!(p.resume().is_err());
        assert!(p.destroy().is_err());
        assert!(p.import(std::path::Path::new("/dummy"), "vm").is_err());
        assert!(p.clone_machine("base", "vm").is_err());
        assert!(p.snapshot_save("s1").is_err());
        assert!(p.snapshot_restore("s1").is_err());
        assert!(p.snapshot_list().is_err());
        assert!(p.snapshot_delete("s1").is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PRLCTL_ERROR");
            std::env::set_var("MIGRATORY_TEST_MOCK_PRLCTL_UNAVAILABLE", "1");
        }
        assert!(!ParallelsProvider::is_available());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PRLCTL_UNAVAILABLE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_PRLCTL");
        }

        // Test real execution branches when mock is unset
        assert!(execute_prlctl_inner("echo", &["test"]).is_ok());
        assert!(execute_prlctl_inner("false", &["test"]).is_err());
        assert!(execute_prlctl_inner("this_command_does_not_exist_xyz_123", &["test"]).is_err());
        let _ = ParallelsProvider::is_available();
    }
}
