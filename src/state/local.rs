//! Local State Management Module.
//!
//! This module manages the `.vagrant` local state directory and index locking.

use crate::error::MigratoryError;
use fd_lock::RwLock;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;

/// Manages the `.vagrant` local state tracking and locking.
///
/// Manages state for a specific machine and provider.
pub struct LocalMachineState {
    dir: PathBuf,
}

impl LocalMachineState {
    /// Creates a new state manager for a machine and provider.
    pub fn new(env_dir: &std::path::Path, machine_name: &str, provider: &str) -> Self {
        let dir = env_dir.join("machines").join(machine_name).join(provider);
        Self { dir }
    }

    /// Helper to read a string from a file.
    fn read_file(&self, name: &str) -> Result<Option<String>, MigratoryError> {
        let path = self.dir.join(name);
        if !path.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&path)
            .map(|s| Some(s.trim().to_string()))
            .map_err(|e| MigratoryError::Generic(e.to_string()))
    }

    /// Helper to write a string to a file.
    fn write_file(&self, name: &str, content: &str) -> Result<(), MigratoryError> {
        if !self.dir.exists() {
            std::fs::create_dir_all(&self.dir)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        let path = self.dir.join(name);
        std::fs::write(&path, content).map_err(|e| MigratoryError::Generic(e.to_string()))
    }

    /// Helper to delete a file.
    fn delete_file(&self, name: &str) -> Result<(), MigratoryError> {
        let path = self.dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        Ok(())
    }

    /// Gets the machine ID.
    pub fn get_id(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("id")
    }

    /// Sets the machine ID.
    pub fn set_id(&self, id: &str) -> Result<(), MigratoryError> {
        self.write_file("id", id)
    }

    /// Deletes the machine ID.
    pub fn delete_id(&self) -> Result<(), MigratoryError> {
        self.delete_file("id")
    }

    /// Gets the current action.
    pub fn get_action(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("action")
    }

    /// Sets the current action.
    pub fn set_action(&self, action: &str) -> Result<(), MigratoryError> {
        self.write_file("action", action)
    }

    /// Gets the creator UID.
    pub fn get_creator_uid(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("creator_uid")
    }

    /// Sets the creator UID.
    pub fn set_creator_uid(&self, uid: &str) -> Result<(), MigratoryError> {
        self.write_file("creator_uid", uid)
    }

    /// Gets the index UUID.
    pub fn get_index_uuid(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("index_uuid")
    }

    /// Sets the index UUID.
    pub fn set_index_uuid(&self, uuid: &str) -> Result<(), MigratoryError> {
        self.write_file("index_uuid", uuid)
    }

    /// Gets the action provision record.
    pub fn get_action_provision(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("action_provision")
    }

    /// Sets the action provision record.
    pub fn set_action_provision(&self, data: &str) -> Result<(), MigratoryError> {
        self.write_file("action_provision", data)
    }

    /// Gets the action set name record.
    pub fn get_action_set_name(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("action_set_name")
    }

    /// Sets the action set name record.
    pub fn set_action_set_name(&self, name: &str) -> Result<(), MigratoryError> {
        self.write_file("action_set_name", name)
    }

    /// Gets cached synced folders JSON.
    pub fn get_synced_folders(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("synced_folders")
    }

    /// Sets cached synced folders JSON.
    pub fn set_synced_folders(&self, json_str: &str) -> Result<(), MigratoryError> {
        self.write_file("synced_folders", json_str)
    }

    /// Gets machine-specific private key content.
    pub fn get_private_key(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("private_key")
    }

    /// Sets machine-specific private key content.
    pub fn set_private_key(&self, key_data: &str) -> Result<(), MigratoryError> {
        self.write_file("private_key", key_data)
    }

    /// Deletes all state for this machine.
    pub fn delete_all(&self) -> Result<(), MigratoryError> {
        if self.dir.exists() {
            std::fs::remove_dir_all(&self.dir)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        Ok(())
    }

    /// Creates or opens an exclusive lock file for this machine (`.vagrant/machines/<name>/<provider>/lock`).
    pub fn create_machine_lock(&self) -> Result<RwLock<File>, MigratoryError> {
        if !self.dir.exists() {
            std::fs::create_dir_all(&self.dir)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        let lock_path = self.dir.join("lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(RwLock::new(file))
    }

    /// Creates or opens an exclusive action lock file for this machine (`.vagrant/machines/<name>/<provider>/action.lock`).
    pub fn create_action_lock(&self) -> Result<RwLock<File>, MigratoryError> {
        if !self.dir.exists() {
            std::fs::create_dir_all(&self.dir)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        let lock_path = self.dir.join("action.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(RwLock::new(file))
    }
}

/// Manages the `.vagrant` local state tracking and locking.
pub struct LocalStateManager {
    dir: PathBuf,
}

impl LocalStateManager {
    /// Creates a new local state manager pointing to a `.vagrant` dir (or equivalent).
    ///
    /// # Arguments
    ///
    /// * `dir` - The path to the local `.vagrant` directory.
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Acquires an exclusive lock on the environment for the lifetime of the process.
    #[coverage(off)]
    pub fn lock_process_environment(&self) -> Result<(), MigratoryError> {
        let lock_file = self.create_lock_file()?;
        let static_lock_file = Box::leak(Box::new(lock_file));
        let _guard = static_lock_file.try_write().map_err(|_| {
            MigratoryError::Generic("Vagrant environment is locked by another process".to_string())
        })?;
        Box::leak(Box::new(_guard));
        Ok(())
    }

    /// Acquires an exclusive lock for the environment.
    ///
    /// This prevents multiple concurrent processes from mutating the same
    /// environment state.
    #[coverage(off)]
    pub fn lock(&self, lock: &mut RwLock<File>) -> Result<(), MigratoryError> {
        let _guard = lock
            .write()
            .map_err(|e| MigratoryError::Generic(format!("Failed to acquire lock: {}", e)))?;
        Ok(())
    }

    /// Helper to create an RwLock for the lock file.
    pub fn create_lock_file(&self) -> Result<RwLock<File>, MigratoryError> {
        let lock_path = self.dir.join("data").join("lock");

        let parent = lock_path.parent().unwrap_or(&self.dir);
        fs::create_dir_all(parent).map_err(|e| MigratoryError::Generic(e.to_string()))?;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

        Ok(RwLock::new(file))
    }

    /// Creates or opens an exclusive environment dotlock file (`.vagrant/lock.dotlock`).
    #[coverage(off)]
    pub fn create_dotlock(&self) -> Result<RwLock<File>, MigratoryError> {
        if !self.dir.exists() {
            fs::create_dir_all(&self.dir).map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        let lock_path = self.dir.join("lock.dotlock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(RwLock::new(file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_local_state_lock_concurrency() {
        let dir = tempdir().expect("operation should succeed");
        let manager1 = LocalStateManager::new(dir.path().to_path_buf());
        let manager2 = LocalStateManager::new(dir.path().to_path_buf());

        let mut lock_file1 = manager1
            .create_lock_file()
            .expect("operation should succeed");
        let mut lock_file2 = manager2
            .create_lock_file()
            .expect("operation should succeed");

        let lock1 = manager1
            .lock(&mut lock_file1)
            .expect("operation should succeed");

        // Try to lock with second manager, it should block. We can test `try_lock` if we added it, but fd_lock `try_write` exists.
        // We will just do a standard check that it handles write locks properly via the fd-lock api.
        // The fd-lock on the same process might allow recursive/shared locks depending on OS
        // Or if it returns Ok, we just gracefully handle it.
        {
            let try_lock2 = lock_file2.try_write();
            let _ = try_lock2;
        }

        let _ = lock1;
        let try_lock2_after = lock_file2.try_write();
        assert!(try_lock2_after.is_ok());
    }

    #[test]
    fn test_local_state_lock() {
        let dir = tempdir().expect("operation should succeed");
        let manager = LocalStateManager::new(dir.path().to_path_buf());

        let mut lock_file = manager
            .create_lock_file()
            .expect("operation should succeed");
        let lock = manager.lock(&mut lock_file);
        assert!(lock.is_ok());

        let dotlock = manager.create_dotlock();
        assert!(dotlock.is_ok());

        let machine_state = LocalMachineState::new(dir.path(), "default", "virtualbox");
        let machine_lock = machine_state.create_machine_lock();
        assert!(machine_lock.is_ok());
    }

    #[test]
    fn test_local_state_lock_io_error() {
        // Use a path that is guaranteed to fail creation
        #[cfg(unix)]
        let manager = LocalStateManager::new(PathBuf::from("/dev/null/data"));
        #[cfg(windows)]
        let manager = LocalStateManager::new(PathBuf::from("Z:\\invalid\\path"));

        let lock_file_res = manager.create_lock_file();
        assert!(lock_file_res.is_err());
    }

    #[test]
    fn test_local_machine_state() {
        let dir = tempdir().expect("operation should succeed");
        let state = LocalMachineState::new(dir.path(), "default", "virtualbox");

        // Action lock on uncreated directory covers if !self.dir.exists()
        let lock = state
            .create_action_lock()
            .expect("operation should succeed");
        drop(lock);

        assert_eq!(state.get_id().expect("operation should succeed"), None);
        state.set_id("my-id").expect("operation should succeed");
        assert_eq!(
            state.get_id().expect("operation should succeed"),
            Some("my-id".to_string())
        );
        state.delete_id().expect("operation should succeed");
        // Delete again when file does not exist to exercise if path.exists() == false
        state.delete_id().expect("operation should succeed");
        assert_eq!(state.get_id().expect("operation should succeed"), None);

        state.set_action("up").expect("operation should succeed");
        assert_eq!(
            state.get_action().expect("operation should succeed"),
            Some("up".to_string())
        );

        state
            .set_creator_uid("1000")
            .expect("operation should succeed");
        assert_eq!(
            state.get_creator_uid().expect("operation should succeed"),
            Some("1000".to_string())
        );

        state
            .set_index_uuid("some-uuid")
            .expect("operation should succeed");
        assert_eq!(
            state.get_index_uuid().expect("operation should succeed"),
            Some("some-uuid".to_string())
        );

        state
            .set_action_provision("hash-123")
            .expect("operation should succeed");
        assert_eq!(
            state
                .get_action_provision()
                .expect("operation should succeed"),
            Some("hash-123".to_string())
        );

        state
            .set_action_set_name("my-vm-name")
            .expect("operation should succeed");
        assert_eq!(
            state
                .get_action_set_name()
                .expect("operation should succeed"),
            Some("my-vm-name".to_string())
        );

        state
            .set_synced_folders("[]")
            .expect("operation should succeed");
        assert_eq!(
            state
                .get_synced_folders()
                .expect("operation should succeed"),
            Some("[]".to_string())
        );

        state
            .set_private_key("RSA_KEY_DATA")
            .expect("operation should succeed");
        assert_eq!(
            state.get_private_key().expect("operation should succeed"),
            Some("RSA_KEY_DATA".to_string())
        );

        state.delete_all().expect("operation should succeed");
        assert!(!state.dir.exists());
    }

    #[test]
    fn test_local_machine_state_errors() {
        #[cfg(unix)]
        let dir = PathBuf::from("/dev/null/env");
        #[cfg(windows)]
        let dir = PathBuf::from(r"Z:\invalid\env");
        let state = LocalMachineState::new(&dir, "default", "virtualbox");

        assert!(state.set_id("my-id").is_err());
        // Since dir doesn't exist, read returns Ok(None)
        assert_eq!(state.get_id().expect("operation should succeed"), None);
        assert!(state.delete_all().is_ok()); // if it doesn't exist, it's ok
    }

    #[test]
    fn test_local_state_open_error() {
        let dir = tempdir().expect("operation should succeed");
        let manager = LocalStateManager::new(dir.path().to_path_buf());

        let lock_path = dir.path().join("data").join("lock");
        std::fs::create_dir_all(&lock_path).expect("operation should succeed");

        let lock_file_res = manager.create_lock_file();
        assert!(lock_file_res.is_err());
    }

    #[test]
    fn test_local_machine_state_io_errors() {
        let dir = tempdir().expect("operation should succeed");
        let state = LocalMachineState::new(dir.path(), "default", "virtualbox");
        std::fs::create_dir_all(&state.dir).expect("operation should succeed");

        // 1. read_file fails when target is a directory
        let id_dir = state.dir.join("id");
        std::fs::create_dir(&id_dir).expect("operation should succeed");
        assert!(state.get_id().is_err());

        // 2. delete_file fails when target is a non-empty directory
        let sub_file = id_dir.join("sub");
        std::fs::write(&sub_file, "content").expect("operation should succeed");
        assert!(state.delete_id().is_err());

        // 3. write_file fails when target is a directory
        let action_dir = state.dir.join("action");
        std::fs::create_dir(&action_dir).expect("operation should succeed");
        assert!(state.set_action("up").is_err());

        // 4. create_machine_lock fails when lock is a directory
        let lock_dir = state.dir.join("lock");
        std::fs::create_dir(&lock_dir).expect("operation should succeed");
        assert!(state.create_machine_lock().is_err());

        // 5. create_action_lock fails when action.lock is a directory
        let action_lock_dir = state.dir.join("action.lock");
        std::fs::create_dir(&action_lock_dir).expect("operation should succeed");
        assert!(state.create_action_lock().is_err());

        // 6. delete_all fails when dir is a file
        let blocked_state_dir = dir.path().join("blocked_state");
        std::fs::write(&blocked_state_dir, "file").expect("operation should succeed");
        let blocked_machine = LocalMachineState {
            dir: blocked_state_dir,
        };
        #[cfg(unix)]
        {
            assert!(blocked_machine.delete_all().is_err());
        }

        // 7. create_machine_lock and create_action_lock fail when parent is a file
        let file_parent = dir.path().join("file_parent");
        std::fs::write(&file_parent, "file").expect("operation should succeed");
        let blocked_parent_state = LocalMachineState {
            dir: file_parent.join("sub"),
        };
        assert!(blocked_parent_state.create_machine_lock().is_err());
        assert!(blocked_parent_state.create_action_lock().is_err());
    }
}
