//! Libvirt Provider implementation.
//!
//! Provides the virtualization driver for Libvirt / KVM,
//! interfacing with the `virsh` CLI to manage domain lifecycles, configure hardware,
//! and orchestrate domain snapshots.

use super::Provider;
use crate::config::VmConfig;
use crate::error::MigratoryError;
use std::process::Command;

/// Libvirt / KVM hypervisor provider.
///
/// Implements the [`Provider`] trait to control KVM-backed virtual machines managed by Libvirt.
pub struct LibvirtProvider {
    /// Domain name or UUID of the machine in Libvirt.
    machine_id: Option<String>,
}

impl LibvirtProvider {
    /// Creates a new `LibvirtProvider` instance.
    ///
    /// # Arguments
    ///
    /// * `machine_id` - Optional domain name or UUID of the machine in Libvirt.
    ///
    /// # Returns
    ///
    /// Returns a new `LibvirtProvider`.
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

    /// Checks if Libvirt is installed and available on the host system.
    ///
    /// # Returns
    ///
    /// Returns `true` if `virsh` binary is available, `false` otherwise.
    ///
    /// # Panics
    ///
    /// Never panics.
    pub fn is_available() -> bool {
        if std::env::var("MIGRATORY_TEST_MOCK_VIRSH").is_ok() {
            return std::env::var("MIGRATORY_TEST_MOCK_VIRSH_UNAVAILABLE").is_err();
        }
        Command::new("virsh").arg("-v").output().is_ok()
    }
}

/// Helper function to execute `virsh` commands with mock support.
///
/// # Arguments
///
/// * `args` - Command-line arguments to pass to `virsh`.
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
pub(crate) fn execute_virsh(args: &[&str]) -> Result<String, MigratoryError> {
    execute_virsh_inner("virsh", args)
}

fn execute_virsh_inner(cmd: &str, args: &[&str]) -> Result<String, MigratoryError> {
    if std::env::var("MIGRATORY_TEST_MOCK_VIRSH").is_ok() {
        if std::env::var("MIGRATORY_TEST_MOCK_VIRSH_ERROR").is_ok() {
            return Err(MigratoryError::Generic("Mock virsh error".to_string()));
        }
        if args.contains(&"domstate") {
            if std::env::var("MIGRATORY_TEST_MOCK_RUNNING").is_ok() {
                return Ok("running
"
                .to_string());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_POWEROFF").is_ok() {
                return Ok("shut off
"
                .to_string());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_SAVED").is_ok() {
                return Ok("paused
"
                .to_string());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_VIRSH_STATUS_EMPTY").is_ok() {
                return Ok(String::new());
            }
            if std::env::var("MIGRATORY_TEST_MOCK_VIRSH_STATUS_OTHER").is_ok() {
                return Ok("crashed
"
                .to_string());
            }
            return Ok("running
"
            .to_string());
        }
        if args.contains(&"snapshot-list") {
            if std::env::var("MIGRATORY_TEST_MOCK_VIRSH_SNAPSHOT_EMPTY").is_ok() {
                return Ok(String::new());
            }
            return Ok("snap1
snap2
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

impl Provider for LibvirtProvider {
    fn name(&self) -> &'static str {
        "libvirt"
    }

    fn up(&self, _config: &VmConfig) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["start", id])?;
        Ok(())
    }

    fn halt(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["shutdown", id])?;
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
        execute_virsh(&[
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
        execute_virsh(&["suspend", id])?;
        Ok(())
    }

    fn resume(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["resume", id])?;
        Ok(())
    }

    fn destroy(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let _ = execute_virsh(&["destroy", id]);
        execute_virsh(&["undefine", id, "--remove-all-storage"])?;
        Ok(())
    }

    fn status(&self) -> Result<String, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok("not created".to_string()),
        };

        let out = execute_virsh(&["domstate", id]).unwrap_or_else(|_| String::new());
        let trimmed = out.trim();
        if trimmed == "running" {
            Ok("running".to_string())
        } else if trimmed == "shut off" {
            Ok("poweroff".to_string())
        } else if trimmed == "paused" {
            Ok("saved".to_string())
        } else if trimmed.is_empty() {
            Ok("not created".to_string())
        } else {
            Ok(trimmed.to_string())
        }
    }

    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["snapshot-create-as", id, name])?;
        Ok(())
    }

    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["snapshot-revert", id, name])?;
        Ok(())
    }

    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = execute_virsh(&["snapshot-list", id, "--name"])?;
        let list = out
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        Ok(list)
    }

    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_virsh(&["snapshot-delete", id, name])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_libvirt_provider_methods() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VIRSH", "1");
        }

        let p_none = LibvirtProvider::new(None);
        assert_eq!(p_none.name(), "libvirt");
        assert!(LibvirtProvider::is_available());
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

        let p = LibvirtProvider::new(Some("test-domain".to_string()));
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
            std::env::set_var("MIGRATORY_TEST_MOCK_VIRSH_STATUS_EMPTY", "1");
        }
        assert_eq!(p.status().expect("status ok"), "not created");

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VIRSH_STATUS_EMPTY");
            std::env::set_var("MIGRATORY_TEST_MOCK_VIRSH_STATUS_OTHER", "1");
        }
        assert_eq!(p.status().expect("status ok"), "crashed");

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VIRSH_STATUS_OTHER");
        }

        // Snapshots
        assert!(p.snapshot_save("snap1").is_ok());
        assert!(p.snapshot_restore("snap1").is_ok());
        assert!(p.snapshot_delete("snap1").is_ok());
        let list = p.snapshot_list().expect("list ok");
        assert_eq!(list, vec!["snap1", "snap2"]);

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VIRSH_SNAPSHOT_EMPTY", "1");
        }
        let list_empty = p.snapshot_list().expect("list ok");
        assert!(list_empty.is_empty());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VIRSH_SNAPSHOT_EMPTY");
            std::env::set_var("MIGRATORY_TEST_MOCK_VIRSH_ERROR", "1");
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
            std::env::remove_var("MIGRATORY_TEST_MOCK_VIRSH_ERROR");
            std::env::set_var("MIGRATORY_TEST_MOCK_VIRSH_UNAVAILABLE", "1");
        }
        assert!(!LibvirtProvider::is_available());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VIRSH_UNAVAILABLE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_VIRSH");
        }

        // Test real execution branches when mock is unset
        assert!(execute_virsh_inner("echo", &["test"]).is_ok());
        assert!(execute_virsh_inner("false", &["test"]).is_err());
        assert!(execute_virsh_inner("this_command_does_not_exist_xyz_123", &["test"]).is_err());
        let _ = LibvirtProvider::is_available();
    }
}
