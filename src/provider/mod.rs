//! Provider module for hypervisor interactions.
//!
//! This module defines the core `Provider` trait which must be implemented
//! by all supported virtual machine hypervisors, and manages state tracking
//! for machines within the `.vagrant` directory equivalent.

use crate::config::VmConfig;
use crate::error::MigratoryError;
use std::fs;
use std::path::{Path, PathBuf};

pub mod docker;
pub mod hyperv;
pub mod qemu;
pub mod virtualbox;
pub mod vmware;

/// Core provider interface.
///
/// Any supported hypervisor (VirtualBox, QEMU, Hyper-V, VMware) must
/// implement this trait to manage VM lifecycles.
pub trait Provider {
    /// Name of the provider.
    ///
    /// # Returns
    ///
    /// A string slice containing the provider's canonical name.
    fn name(&self) -> &str;

    /// Brings the machine up.
    ///
    /// # Arguments
    ///
    /// * `config` - The virtual machine configuration settings.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if bringing up the VM fails.
    fn up(&self, config: &VmConfig) -> Result<(), MigratoryError>;

    /// Halts the machine gracefully.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if halting the VM fails.
    fn halt(&self) -> Result<(), MigratoryError>;

    /// Imports a machine from a base box.
    ///
    /// # Arguments
    ///
    /// * `box_dir` - The directory containing the unpacked box.
    /// * `vm_name` - The desired name for the newly created VM.
    ///
    /// # Returns
    ///
    /// Returns `Ok(String)` containing the new machine ID on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if importing fails.
    fn import(&self, box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError>;

    /// Clones an existing machine (Linked Clone).
    ///
    /// # Arguments
    ///
    /// * `base_machine_id` - The ID of the machine to clone.
    /// * `vm_name` - The desired name for the newly created VM.
    ///
    /// # Returns
    ///
    /// Returns `Ok(String)` containing the new machine ID on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if cloning fails.
    fn clone_machine(&self, base_machine_id: &str, vm_name: &str)
    -> Result<String, MigratoryError>;

    /// Destroys the machine, removing all associated resources.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if destroying the VM fails.
    fn destroy(&self) -> Result<(), MigratoryError>;

    /// Returns the current state/status of the machine.
    ///
    /// # Returns
    ///
    /// A string describing the state (e.g., "running", "poweroff", "not created").
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if querying the state fails.
    fn status(&self) -> Result<String, MigratoryError>;

    /// Suspends the machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if suspending the VM fails.
    fn suspend(&self) -> Result<(), MigratoryError>;

    /// Resumes the suspended machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if resuming the VM fails.
    fn resume(&self) -> Result<(), MigratoryError>;

    /// Mounts synced folders configured for the machine.
    ///
    /// # Arguments
    ///
    /// * `config` - The VM configuration.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if mounting fails.
    fn setup_synced_folders(&self, _config: &VmConfig) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Saves a snapshot of the machine.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the snapshot.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if saving fails.
    fn snapshot_save(&self, _name: &str) -> Result<(), MigratoryError> {
        Err(MigratoryError::Generic(format!(
            "Snapshot save not supported by provider: {}",
            self.name()
        )))
    }

    /// Restores a snapshot of the machine.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the snapshot to restore.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if restoring fails.
    fn snapshot_restore(&self, _name: &str) -> Result<(), MigratoryError> {
        Err(MigratoryError::Generic(format!(
            "Snapshot restore not supported by provider: {}",
            self.name()
        )))
    }

    /// Lists snapshots of the machine.
    ///
    /// # Returns
    ///
    /// Returns a list of snapshot names on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if listing fails.
    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        Err(MigratoryError::Generic(format!(
            "Snapshot list not supported by provider: {}",
            self.name()
        )))
    }

    /// Deletes a snapshot of the machine.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the snapshot to delete.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if deletion fails.
    fn snapshot_delete(&self, _name: &str) -> Result<(), MigratoryError> {
        Err(MigratoryError::Generic(format!(
            "Snapshot delete not supported by provider: {}",
            self.name()
        )))
    }

    /// Exports the machine to the specified directory.
    ///
    /// # Arguments
    ///
    /// * `output_dir` - The directory where the OVF/box files should be exported.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if exporting fails or is not supported.
    fn export(&self, _output_dir: &std::path::Path) -> Result<(), MigratoryError> {
        Err(MigratoryError::Generic(format!(
            "Exporting not supported by provider: {}",
            self.name()
        )))
    }
}

/// Returns an instantiated provider based on its name.
///
/// # Arguments
///
/// * `name` - The canonical name of the provider.
/// * `machine_id` - The stored ID of the machine, if any.
///
/// # Returns
///
/// Returns a boxed provider implementation.
///
/// # Errors
///
/// Returns a `MigratoryError` if the provider name is unknown.
pub fn get_provider(
    name: &str,
    machine_id: Option<String>,
) -> Result<Box<dyn Provider>, MigratoryError> {
    match name {
        "virtualbox" => Ok(Box::new(virtualbox::VirtualBoxProvider::new(machine_id))),
        "qemu" => Ok(Box::new(qemu::QemuProvider::new(machine_id))),
        "hyperv" => Ok(Box::new(hyperv::HypervProvider::new(machine_id))),
        "vmware" => Ok(Box::new(vmware::VmwareProvider::new(machine_id))),
        "docker" => Ok(Box::new(docker::DockerProvider::new(machine_id))),
        _ => Err(MigratoryError::Generic(format!(
            "Unknown provider: {}",
            name
        ))),
    }
}

/// Manages `.vagrant/` state tracking.
///
/// Responsible for maintaining state files such as the hypervisor-specific
/// machine IDs used to identify the running instances.
pub struct StateManager {
    dir: PathBuf,
}

impl StateManager {
    /// Creates a new state manager pointing to a `.vagrant` dir (or equivalent).
    ///
    /// # Arguments
    ///
    /// * `dir` - The path to the state directory.
    ///
    /// # Returns
    ///
    /// Returns a new instance of `StateManager`.
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    #[coverage(off)]
    fn create_parent_dir(path: &Path) -> Result<(), MigratoryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        Ok(())
    }

    /// Creates a lock file for the state directory to prevent concurrent access.
    ///
    /// # Returns
    ///
    /// Returns a new `RwLock<std::fs::File>` on success.
    pub fn create_lock_file(&self) -> Result<fd_lock::RwLock<std::fs::File>, MigratoryError> {
        let lock_path = self.dir.join("action.lock");
        Self::create_parent_dir(&lock_path)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&lock_path)
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

        Ok(fd_lock::RwLock::new(file))
    }

    /// Reads the stored ID for a specific machine.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(String))` containing the ID if it exists, or `Ok(None)` if it doesn't.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if an unexpected I/O error occurs reading the file.
    pub fn read_id(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<Option<String>, MigratoryError> {
        let path = self
            .dir
            .join("machines")
            .join(machine_name)
            .join(provider_name)
            .join("id");
        if path.exists() {
            let id =
                fs::read_to_string(path).map_err(|e| MigratoryError::Generic(e.to_string()))?;
            Ok(Some(id.trim().to_string()))
        } else {
            Ok(None)
        }
    }

    /// Writes the ID for a specific machine.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    /// * `id` - The machine ID string to write.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if an I/O error occurs creating the directories or writing the file.
    pub fn write_id(
        &self,
        machine_name: &str,
        provider_name: &str,
        id: &str,
    ) -> Result<(), MigratoryError> {
        let path = self
            .dir
            .join("machines")
            .join(machine_name)
            .join(provider_name)
            .join("id");
        Self::create_parent_dir(&path)?;
        fs::write(path, id).map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(())
    }

    /// Clears the state for a specific machine by removing its directory.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if an I/O error occurs while removing the directory.
    pub fn clear_machine_state(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<(), MigratoryError> {
        let path = self
            .dir
            .join("machines")
            .join(machine_name)
            .join(provider_name);
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        Ok(())
    }

    /// Checks if a machine has been provisioned.
    pub fn has_provisioned(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<bool, MigratoryError> {
        let path = self
            .dir
            .join("machines")
            .join(machine_name)
            .join(provider_name)
            .join("action_provision");
        Ok(path.exists())
    }

    /// Marks a machine as provisioned.
    pub fn mark_provisioned(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<(), MigratoryError> {
        let path = self
            .dir
            .join("machines")
            .join(machine_name)
            .join(provider_name)
            .join("action_provision");
        Self::create_parent_dir(&path)?;
        fs::write(path, "").map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(())
    }

    /// Clears the provisioned state.
    pub fn clear_provisioned(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<(), MigratoryError> {
        let path = self
            .dir
            .join("machines")
            .join(machine_name)
            .join(provider_name)
            .join("action_provision");
        if path.exists() {
            fs::remove_file(&path).map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_state_manager_provisioned() {
        let dir = tempdir().expect("operation should succeed");
        let manager = StateManager::new(dir.path().to_path_buf());

        // Initial state
        assert_eq!(
            manager
                .has_provisioned("test", "virtualbox")
                .expect("operation should succeed"),
            false
        );

        // Mark provisioned
        manager
            .mark_provisioned("test", "virtualbox")
            .expect("operation should succeed");
        assert_eq!(
            manager
                .has_provisioned("test", "virtualbox")
                .expect("operation should succeed"),
            true
        );

        // Clear provisioned
        manager
            .clear_provisioned("test", "virtualbox")
            .expect("operation should succeed");
        assert_eq!(
            manager
                .has_provisioned("test", "virtualbox")
                .expect("operation should succeed"),
            false
        );

        // Clear when doesn't exist
        assert!(manager.clear_provisioned("test", "virtualbox").is_ok());

        // Mark and then clear to hit the exists branch
        manager
            .mark_provisioned("test2", "virtualbox")
            .expect("operation should succeed");
        assert_eq!(
            manager
                .has_provisioned("test2", "virtualbox")
                .expect("operation should succeed"),
            true
        );
        manager
            .clear_provisioned("test2", "virtualbox")
            .expect("operation should succeed");
        assert_eq!(
            manager
                .has_provisioned("test2", "virtualbox")
                .expect("operation should succeed"),
            false
        );
    }

    #[test]
    fn test_state_manager() {
        let dir = tempdir().expect("operation should succeed");
        let state = StateManager::new(dir.path().to_path_buf());

        let id = state.read_id("default", "virtualbox");
        assert!(id.is_ok());
        assert!(id.expect("read_id should return Ok").is_none());

        let write_result = state.write_id("default", "virtualbox", "12345");
        assert!(write_result.is_ok());

        let new_id = state
            .read_id("default", "virtualbox")
            .expect("read_id should return Ok");
        assert_eq!(new_id.expect("id should be Some"), "12345");
    }

    #[test]
    fn test_state_manager_write_error() {
        let dir = tempdir().expect("operation should succeed");
        let state = StateManager::new(dir.path().to_path_buf());

        // Simulating a write error by attempting to write into a file treated as a directory.
        let machines_dir = dir.path().join("machines");
        std::fs::create_dir(&machines_dir).expect("operation should succeed");

        let target_machine = machines_dir.join("error_machine");
        std::fs::write(&target_machine, "not a dir").expect("operation should succeed");

        let write_result = state.write_id("error_machine", "virtualbox", "12345");
        assert!(write_result.is_err());
    }

    #[test]
    fn test_state_manager_read_error() {
        let dir = tempdir().expect("operation should succeed");
        let state = StateManager::new(dir.path().to_path_buf());

        let path = dir
            .path()
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&path).expect("operation should succeed");
        // Create `id` as a directory instead of a file, causing read_to_string to fail
        let id_path = path.join("id");
        std::fs::create_dir(&id_path).expect("operation should succeed");

        let read_result = state.read_id("default", "virtualbox");
        assert!(read_result.is_err());
    }

    #[test]
    fn test_state_manager_lock_error() {
        let dir = tempdir().expect("operation should succeed");
        let state = StateManager::new(dir.path().to_path_buf());

        // Success path
        assert!(state.create_lock_file().is_ok());

        // Failure path when lock file is a directory
        let lock_path = dir.path().join("sub/action.lock");
        std::fs::create_dir_all(&lock_path).expect("operation should succeed");
        let state_err = StateManager::new(dir.path().join("sub"));
        let lock_result = state_err.create_lock_file();
        assert!(lock_result.is_err());
    }

    #[test]
    fn test_state_manager_clear_machine_state() {
        let dir = tempdir().expect("operation should succeed");
        let state = StateManager::new(dir.path().to_path_buf());

        // Test false branch
        assert!(
            state
                .clear_machine_state("non_existent", "virtualbox")
                .is_ok()
        );

        let write_result = state.write_id("default", "virtualbox", "12345");
        assert!(write_result.is_ok());

        assert!(
            state
                .read_id("default", "virtualbox")
                .expect("operation should succeed")
                .is_some()
        );

        let clear_result = state.clear_machine_state("default", "virtualbox");
        assert!(clear_result.is_ok());

        assert!(
            state
                .read_id("default", "virtualbox")
                .expect("operation should succeed")
                .is_none()
        );
    }

    #[test]
    fn test_state_manager_io_failures() {
        let dir = tempdir().expect("operation should succeed");
        let state = StateManager::new(dir.path().to_path_buf());

        // 1. write_id failure when id is a directory
        let id_dir = dir.path().join("machines/m1/p1/id");
        std::fs::create_dir_all(&id_dir).expect("operation should succeed");
        assert!(state.write_id("m1", "p1", "123").is_err());

        // 2. mark_provisioned failure when action_provision is a directory
        let prov_dir = dir.path().join("machines/m2/p2/action_provision");
        std::fs::create_dir_all(&prov_dir).expect("operation should succeed");
        assert!(state.mark_provisioned("m2", "p2").is_err());

        // 3. clear_provisioned failure when action_provision is a non-empty directory
        let sub_file = prov_dir.join("subfile");
        std::fs::write(&sub_file, "sub").expect("operation should succeed");
        assert!(state.clear_provisioned("m2", "p2").is_err());

        // 4. clear_machine_state failure when path is a regular file
        let file_path = dir.path().join("machines/m3/p3");
        std::fs::create_dir_all(dir.path().join("machines/m3")).expect("operation should succeed");
        std::fs::write(&file_path, "regular_file").expect("operation should succeed");
        #[cfg(unix)]
        {
            assert!(state.clear_machine_state("m3", "p3").is_err());
        }

        // 5. create_parent_dir failure in create_lock_file
        let file_as_dir = dir.path().join("file_lock_parent");
        std::fs::write(&file_as_dir, "blocking").expect("operation should succeed");
        let state_blocked_lock = StateManager::new(file_as_dir.join("sub"));
        assert!(state_blocked_lock.create_lock_file().is_err());

        // 6. create_parent_dir failure in mark_provisioned
        let state_blocked_prov = StateManager::new(file_as_dir);
        assert!(state_blocked_prov.mark_provisioned("m", "p").is_err());
    }

    struct DefaultProvider;
    #[coverage(off)]
    impl Provider for DefaultProvider {
        fn name(&self) -> &str {
            "default"
        }
        fn up(&self, _: &crate::config::VmConfig) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn halt(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn import(&self, _: &std::path::Path, _: &str) -> Result<String, MigratoryError> {
            Ok("id".to_string())
        }
        fn clone_machine(&self, _: &str, _: &str) -> Result<String, MigratoryError> {
            Ok("id".to_string())
        }
        fn destroy(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn status(&self) -> Result<String, MigratoryError> {
            Ok("running".to_string())
        }
        fn suspend(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn resume(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_provider_default_methods() {
        let p = DefaultProvider;

        assert!(p.resume().is_ok());
        let cfg = crate::config::VmConfig::default();
        assert!(p.setup_synced_folders(&cfg).is_ok());

        assert!(p.snapshot_save("test").is_err());
        assert!(p.snapshot_restore("test").is_err());
        assert!(p.snapshot_list().is_err());
        assert!(p.snapshot_delete("test").is_err());
        assert!(p.export(std::path::Path::new("/dummy")).is_err());
    }

    #[test]
    fn test_get_provider() {
        assert!(get_provider("virtualbox", None).is_ok());
        assert!(get_provider("qemu", None).is_ok());
        assert!(get_provider("hyperv", None).is_ok());
        assert!(get_provider("vmware", None).is_ok());
        assert!(get_provider("docker", None).is_ok());
        assert!(get_provider("unknown_provider", None).is_err());
    }
}
