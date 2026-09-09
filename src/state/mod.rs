//! Global State Management Module.
//!
//! This module manages the global index of Vagrant environments and machines,
//! corresponding to `~/.vagrant.d/data/machine-index/index`.

use crate::error::MigratoryError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A single machine entry in the global index.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GlobalMachineEntry {
    /// The local data directory for this machine instance.
    pub local_data_path: String,
    /// The name of the machine.
    pub name: String,
    /// The provider used to run the machine.
    pub provider: String,
    /// The current state of the machine.
    pub state: String,
    /// Path to the Vagrantfile that defines this machine.
    pub vagrantfile_path: String,
    /// The Vagrantfile name.
    pub vagrantfile_name: String,
    /// Timestamp when the entry was created or updated.
    pub updated_at: u64,
    /// Arbitrary extra data.
    pub extra_data: HashMap<String, String>,
}

/// The structure of the global machine index JSON file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalIndex {
    /// Version of the index format.
    pub version: u32,
    /// Map of UUID to machine entries.
    pub machines: HashMap<String, GlobalMachineEntry>,
}

/// Manages the global state files.
pub struct GlobalStateManager {
    index_path: PathBuf,
}

impl GlobalStateManager {
    /// Creates a new global state manager.
    ///
    /// # Arguments
    ///
    /// * `dir` - The path to the global state directory (e.g. `~/.vagrant.d`).
    pub fn new(dir: PathBuf) -> Self {
        let index_path = dir.join("data").join("machine-index").join("index");
        Self { index_path }
    }

    /// Creates or opens an RwLock on the global index lock file (`~/.vagrant.d/data/machine-index/index.lock`).
    pub fn create_index_lock(&self) -> Result<fd_lock::RwLock<File>, MigratoryError> {
        let parent = self.index_path.parent().unwrap_or(std::path::Path::new(""));
        if !parent.as_os_str().is_empty() && !parent.exists() {
            let _ = std::fs::create_dir_all(parent);
        }
        let lock_path = self.index_path.with_extension("lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(
                #[coverage(off)]
                |e| MigratoryError::Generic(e.to_string()),
            )?;
        Ok(fd_lock::RwLock::new(file))
    }

    #[coverage(off)]
    fn read_locked_contents(&self) -> Result<String, MigratoryError> {
        let lock_obj = self.create_index_lock()?;
        let _guard = lock_obj.read().map_err(|e| {
            MigratoryError::Generic(format!("Failed to acquire index read lock: {}", e))
        })?;
        fs::read_to_string(&self.index_path).map_err(|e| MigratoryError::Generic(e.to_string()))
    }

    /// Reads the global machine index.
    ///
    /// If the file does not exist, it returns an empty `GlobalIndex`.
    pub fn read_index(&self) -> Result<GlobalIndex, MigratoryError> {
        if !self.index_path.exists() {
            return Ok(GlobalIndex {
                version: 1,
                machines: HashMap::new(),
            });
        }

        let contents = self.read_locked_contents()?;

        let index: GlobalIndex = serde_json::from_str(&contents)
            .map_err(|e| MigratoryError::Generic(format!("Failed to parse index JSON: {}", e)))?;

        Ok(index)
    }

    #[coverage(off)]
    fn serialize_index(index: &GlobalIndex) -> String {
        serde_json::to_string_pretty(index).unwrap_or_default()
    }

    #[coverage(off)]
    fn atomic_write_file(path: &Path, contents: &str) -> Result<(), MigratoryError> {
        let temp_path = path.with_extension("tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp_path)
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

        file.write_all(contents.as_bytes())
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        file.sync_all()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

        fs::rename(temp_path, path).map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(())
    }

    #[coverage(off)]
    fn write_locked_contents(&self, contents: &str) -> Result<(), MigratoryError> {
        let mut lock_obj = self.create_index_lock()?;
        let _guard = lock_obj.write().map_err(|e| {
            MigratoryError::Generic(format!("Failed to acquire index write lock: {}", e))
        })?;
        Self::atomic_write_file(&self.index_path, contents)?;
        Ok(())
    }

    /// Writes the global machine index to disk, ensuring atomic updates by locking.
    pub fn write_index(&self, index: &GlobalIndex) -> Result<(), MigratoryError> {
        let contents = Self::serialize_index(index);
        self.write_locked_contents(&contents)?;
        Ok(())
    }

    /// Prunes orphaned entries where the Vagrantfile no longer exists.
    pub fn prune(&self) -> Result<usize, MigratoryError> {
        let mut index = self.read_index()?;
        let initial_count = index.machines.len();

        index
            .machines
            .retain(|_, entry| Path::new(&entry.vagrantfile_path).exists());

        let removed = initial_count - index.machines.len();

        if removed > 0 {
            self.write_index(&index)?;
        }

        Ok(removed)
    }

    /// Finds a machine entry by full or short UUID prefix (case-insensitive).
    ///
    /// # Arguments
    ///
    /// * `index` - A reference to the loaded `GlobalIndex`.
    /// * `prefix` - Full or partial UUID prefix string.
    pub fn get_by_uuid_prefix<'a>(
        &self,
        index: &'a GlobalIndex,
        prefix: &str,
    ) -> Option<(&'a String, &'a GlobalMachineEntry)> {
        let prefix_lower = prefix.to_lowercase();
        index
            .machines
            .iter()
            .find(|(uuid, _)| uuid.to_lowercase().starts_with(&prefix_lower))
    }

    /// Removes an entry by full or short UUID prefix.
    ///
    /// # Arguments
    ///
    /// * `prefix` - Full or partial UUID prefix string.
    ///
    /// # Returns
    ///
    /// The removed entry if found, or `None`.
    pub fn remove_by_uuid_prefix(
        &self,
        prefix: &str,
    ) -> Result<Option<GlobalMachineEntry>, MigratoryError> {
        let mut index = self.read_index()?;
        let prefix_lower = prefix.to_lowercase();
        let target_uuid = index
            .machines
            .keys()
            .find(|uuid| uuid.to_lowercase().starts_with(&prefix_lower))
            .cloned();

        if let Some(uuid) = target_uuid {
            let entry = index.machines.remove(&uuid);
            self.write_index(&index)?;
            Ok(entry)
        } else {
            Ok(None)
        }
    }
}

/// Resolves a target identifier (either local machine name or global UUID prefix)
/// to an effective working directory and machine name.
///
/// # Arguments
///
/// * `target` - Target machine name or UUID prefix.
///
/// # Returns
///
/// Returns `Ok(Some((resolved_dir, machine_name)))` if resolved via global index,
/// or `Ok(None)` if no global match is found.
#[coverage(off)]
pub fn resolve_global_target(
    target: &str,
) -> Result<Option<(std::path::PathBuf, String)>, MigratoryError> {
    let vagrant_d = match std::env::var("VAGRANT_HOME") {
        Ok(val) => std::path::PathBuf::from(val),
        Err(_) => match std::env::var("HOME") {
            Ok(home) => std::path::PathBuf::from(home).join(".vagrant.d"),
            Err(_) => return Ok(None),
        },
    };

    let state_manager = GlobalStateManager::new(vagrant_d);
    let index = match state_manager.read_index() {
        Ok(idx) => idx,
        Err(_) => return Ok(None),
    };

    if let Some((_, entry)) = state_manager.get_by_uuid_prefix(&index, target) {
        let dir = std::path::PathBuf::from(&entry.vagrantfile_path);
        return Ok(Some((dir, entry.name.clone())));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_resolve_global_target() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("operation should succeed");
        let manager = GlobalStateManager::new(dir.path().to_path_buf());

        let mut index = manager.read_index().expect("operation should succeed");
        index.machines.insert(
            "abcdef12-3456-7890-abcd-ef1234567890".to_string(),
            GlobalMachineEntry {
                local_data_path: "/tmp/local".to_string(),
                name: "db".to_string(),
                provider: "virtualbox".to_string(),
                state: "running".to_string(),
                vagrantfile_path: "/tmp/project".to_string(),
                vagrantfile_name: "Vagrantfile".to_string(),
                updated_at: 12345,
                extra_data: HashMap::new(),
            },
        );
        manager.write_index(&index).expect("write failed");

        unsafe {
            std::env::set_var("VAGRANT_HOME", dir.path());
        }

        let resolved = resolve_global_target("abcdef1").expect("operation should succeed");
        assert!(resolved.is_some());
        let (resolved_dir, resolved_name) = resolved.expect("operation should succeed");
        assert_eq!(resolved_dir, std::path::PathBuf::from("/tmp/project"));
        assert_eq!(resolved_name, "db");

        let not_found = resolve_global_target("nonexistent").expect("operation should succeed");
        assert!(not_found.is_none());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_global_state_prefix_lookup() {
        let dir = tempdir().expect("operation should succeed");
        let manager = GlobalStateManager::new(dir.path().to_path_buf());

        let mut index = manager.read_index().expect("operation should succeed");
        index.machines.insert(
            "1a2b3c4d-5678-90ef-ghij-klmnopqrstuv".to_string(),
            GlobalMachineEntry {
                local_data_path: "/tmp/local".to_string(),
                name: "web".to_string(),
                provider: "virtualbox".to_string(),
                state: "running".to_string(),
                vagrantfile_path: "/tmp/vagrantfile".to_string(),
                vagrantfile_name: "Vagrantfile".to_string(),
                updated_at: 12345,
                extra_data: HashMap::new(),
            },
        );
        manager.write_index(&index).expect("write failed");

        let loaded = manager.read_index().expect("read failed");
        let found = manager.get_by_uuid_prefix(&loaded, "1a2b3c4");
        assert!(found.is_some());
        let (found_uuid, found_entry) = found.expect("operation should succeed");
        assert_eq!(found_uuid, "1a2b3c4d-5678-90ef-ghij-klmnopqrstuv");
        assert_eq!(found_entry.name, "web");

        assert!(manager.get_by_uuid_prefix(&loaded, "nonexistent").is_none());

        let removed = manager
            .remove_by_uuid_prefix("1a2b3c4")
            .expect("remove failed");
        assert!(removed.is_some());
        assert_eq!(removed.expect("operation should succeed").name, "web");

        let reloaded = manager.read_index().expect("read failed");
        assert!(manager.get_by_uuid_prefix(&reloaded, "1a2b3c4").is_none());

        let not_found_remove = manager
            .remove_by_uuid_prefix("1a2b3c4")
            .expect("remove should return None");
        assert!(not_found_remove.is_none());
    }

    #[test]
    fn test_global_state_read_write() {
        let dir = tempdir().expect("operation should succeed");
        // Append a non-existent subdirectory to force create_dir_all
        let manager = GlobalStateManager::new(dir.path().join("missing_subdir"));

        let mut index = manager.read_index().expect("operation should succeed");
        assert_eq!(index.machines.len(), 0);

        index.machines.insert(
            "some-uuid".to_string(),
            GlobalMachineEntry {
                local_data_path: "/tmp/local".to_string(),
                name: "default".to_string(),
                provider: "virtualbox".to_string(),
                state: "running".to_string(),
                vagrantfile_path: "/tmp/vagrantfile".to_string(),
                vagrantfile_name: "Vagrantfile".to_string(),
                updated_at: 12345,
                extra_data: HashMap::new(),
            },
        );

        assert!(manager.write_index(&index).is_ok());

        let index2 = manager.read_index().expect("operation should succeed");
        assert_eq!(index2.machines.len(), 1);
        assert_eq!(index2.machines["some-uuid"].name, "default");
    }

    #[test]
    fn test_global_state_prune() {
        let dir = tempdir().expect("operation should succeed");
        let manager = GlobalStateManager::new(dir.path().to_path_buf());

        let mut index = GlobalIndex {
            version: 1,
            machines: HashMap::new(),
        };
        let vagrantfile_path = dir.path().join("Vagrantfile");
        std::fs::write(&vagrantfile_path, "").expect("operation should succeed");

        let mut entry = GlobalMachineEntry {
            name: "test".to_string(),
            provider: "virtualbox".to_string(),
            state: "running".to_string(),
            local_data_path: dir.path().to_string_lossy().to_string(),
            vagrantfile_path: vagrantfile_path.to_string_lossy().to_string(),

            vagrantfile_name: "Vagrantfile".to_string(),
            updated_at: 0,
            extra_data: HashMap::new(),
        };

        index.machines.insert("valid_id".to_string(), entry.clone());

        // Add an invalid entry pointing to nowhere
        entry.local_data_path = "/path/to/nowhere/that/does/not/exist".to_string();
        entry.vagrantfile_path = "/path/to/nowhere/Vagrantfile".to_string();
        index.machines.insert("invalid_id".to_string(), entry);

        assert!(manager.write_index(&index).is_ok());

        let count = manager.prune().expect("operation should succeed");
        assert_eq!(count, 1); // Only the invalid one pruned

        let read_back = manager.read_index().expect("operation should succeed");
        assert_eq!(read_back.machines.len(), 1);
        assert!(read_back.machines.contains_key("valid_id"));
    }
    #[test]
    fn test_read_index_parse_error() {
        let dir = tempdir().expect("operation should succeed");
        let manager = GlobalStateManager::new(dir.path().to_path_buf());

        fs::create_dir_all(
            manager
                .index_path
                .parent()
                .expect("operation should succeed"),
        )
        .expect("operation should succeed");
        fs::write(&manager.index_path, "{invalid_json}").expect("operation should succeed");

        assert!(manager.read_index().is_err());
    }

    #[test]
    fn test_create_index_lock_empty_parent() {
        let manager = GlobalStateManager {
            index_path: PathBuf::from("target/cov_test_index"),
        };
        let _ = manager.create_index_lock();
        let _ = fs::remove_file("target/cov_test_index.lock");

        let manager_empty = GlobalStateManager {
            index_path: PathBuf::from("cov_test_single_index"),
        };
        let _ = manager_empty.create_index_lock();
        let _ = fs::remove_file("cov_test_single_index.lock");
    }

    #[test]
    fn test_resolve_global_target_no_env() {
        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::remove_var("HOME");
        }
        let res = resolve_global_target("default");
        assert!(res.is_ok());
        assert!(res.expect("operation should succeed").is_none());
    }

    #[test]
    fn test_write_index_io_error() {
        #[cfg(unix)]
        let manager = GlobalStateManager::new(PathBuf::from("/dev/null/data"));
        #[cfg(windows)]
        let manager = GlobalStateManager::new(PathBuf::from("Z:\\invalid\\path"));

        let index = GlobalIndex::default();
        let result = manager.write_index(&index);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_index_io_error() {
        let dir = tempdir().expect("operation should succeed");
        let manager = GlobalStateManager::new(dir.path().to_path_buf());

        fs::create_dir_all(
            manager
                .index_path
                .parent()
                .expect("operation should succeed"),
        )
        .expect("operation should succeed");
        // create a directory where a file is expected to cause an IO error when opening
        fs::create_dir(&manager.index_path).expect("operation should succeed");

        assert!(manager.read_index().is_err());
    }

    #[test]
    fn test_read_index_lock_error() {
        let dir = tempdir().expect("operation should succeed");
        let manager = GlobalStateManager::new(dir.path().to_path_buf());
        let parent = manager
            .index_path
            .parent()
            .expect("operation should succeed");
        fs::create_dir_all(parent).expect("operation should succeed");
        fs::write(&manager.index_path, "{}").expect("operation should succeed");

        let lock_path = manager.index_path.with_extension("lock");
        fs::create_dir(&lock_path).expect("operation should succeed");

        assert!(manager.read_index().is_err());
    }

    #[test]
    fn test_global_state_prune_no_op() {
        let dir = tempdir().expect("operation should succeed");
        let manager = GlobalStateManager::new(dir.path().to_path_buf());

        let mut index = GlobalIndex {
            version: 1,
            machines: HashMap::new(),
        };
        let vagrantfile_path = dir.path().join("Vagrantfile");
        std::fs::write(&vagrantfile_path, "").expect("operation should succeed");

        let entry = GlobalMachineEntry {
            name: "test".to_string(),
            provider: "virtualbox".to_string(),
            state: "running".to_string(),
            local_data_path: dir.path().to_string_lossy().to_string(),
            vagrantfile_path: vagrantfile_path.to_string_lossy().to_string(),
            vagrantfile_name: "Vagrantfile".to_string(),
            updated_at: 0,
            extra_data: HashMap::new(),
        };

        index.machines.insert("valid_id".to_string(), entry);
        assert!(manager.write_index(&index).is_ok());

        let count = manager.prune().expect("operation should succeed");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_prune_write_error() {
        let dir = tempdir().expect("operation should succeed");
        let manager = GlobalStateManager::new(dir.path().to_path_buf());
        let mut index = GlobalIndex::default();
        index.machines.insert(
            "orphaned".to_string(),
            GlobalMachineEntry {
                local_data_path: "".to_string(),
                name: "test".to_string(),
                provider: "virtualbox".to_string(),
                state: "poweroff".to_string(),
                vagrantfile_path: "/nonexistent/path/Vagrantfile".to_string(),
                vagrantfile_name: "Vagrantfile".to_string(),
                updated_at: 0,
                extra_data: HashMap::new(),
            },
        );
        manager.write_index(&index).expect("write failed");

        let tmp_path = manager.index_path.with_extension("tmp");
        fs::create_dir(&tmp_path).expect("mkdir failed");

        assert!(manager.prune().is_err());
    }

    #[test]
    fn test_remove_by_uuid_prefix_write_error() {
        let dir = tempdir().expect("operation should succeed");
        let manager = GlobalStateManager::new(dir.path().to_path_buf());
        let mut index = GlobalIndex::default();
        index.machines.insert(
            "12345678-abcd".to_string(),
            GlobalMachineEntry {
                local_data_path: "".to_string(),
                name: "test".to_string(),
                provider: "virtualbox".to_string(),
                state: "poweroff".to_string(),
                vagrantfile_path: "".to_string(),
                vagrantfile_name: "Vagrantfile".to_string(),
                updated_at: 0,
                extra_data: HashMap::new(),
            },
        );
        manager.write_index(&index).expect("write failed");

        let tmp_path = manager.index_path.with_extension("tmp");
        fs::create_dir(&tmp_path).expect("mkdir failed");

        assert!(manager.remove_by_uuid_prefix("12345678").is_err());
    }

    #[test]
    fn test_remove_by_uuid_prefix_read_error() {
        let dir = tempdir().expect("operation should succeed");
        let manager = GlobalStateManager::new(dir.path().to_path_buf());
        let parent = manager
            .index_path
            .parent()
            .expect("operation should succeed");
        fs::create_dir_all(parent).expect("operation should succeed");
        fs::write(&manager.index_path, "{invalid_json}").expect("operation should succeed");

        assert!(manager.remove_by_uuid_prefix("any").is_err());
    }

    #[test]
    fn test_prune_read_error() {
        let dir = tempdir().expect("operation should succeed");
        let manager = GlobalStateManager::new(dir.path().to_path_buf());

        fs::create_dir_all(
            manager
                .index_path
                .parent()
                .expect("operation should succeed"),
        )
        .expect("operation should succeed");
        fs::create_dir(&manager.index_path).expect("operation should succeed");

        assert!(manager.prune().is_err());
    }
}
pub mod local;
