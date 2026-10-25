//! Local State Management Module.
//!
//! This module manages the `.vagrant` local state directory, per-machine runtime metadata,
//! and file locking.

use crate::error::MigratoryError;
use fd_lock::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

/// Persisted forwarded port mapping stored in `.vagrant/machines/<name>/<provider>/forwarded_ports`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedForwardedPort {
    /// Guest port number.
    pub guest: u16,
    /// Actual allocated or auto-corrected host port number.
    pub host: u16,
    /// Network protocol (`"tcp"` or `"udp"`).
    pub protocol: String,
    /// Optional host IP address bound by the forwarded port.
    pub host_ip: Option<String>,
}

/// Manages state for a specific machine and provider inside `.vagrant/machines/<name>/<provider>`.
pub struct LocalMachineState {
    /// Directory path storing state files for this machine and provider (`.vagrant/machines/<name>/<provider>`).
    dir: PathBuf,
}

impl LocalMachineState {
    /// Creates a new state manager for a machine and provider.
    ///
    /// # Arguments
    ///
    /// * `env_dir` - Path to the `.vagrant` environment directory.
    /// * `machine_name` - Name of the machine.
    /// * `provider` - Name of the provider.
    ///
    /// # Returns
    ///
    /// Returns a [`LocalMachineState`] targeting `.vagrant/machines/<machine_name>/<provider>`.
    pub fn new(env_dir: &Path, machine_name: &str, provider: &str) -> Self {
        let dir = env_dir.join("machines").join(machine_name).join(provider);
        Self { dir }
    }

    /// Reads and trims a string from a named state file if it exists.
    ///
    /// # Arguments
    ///
    /// * `name` - File name relative to the machine state directory.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(String))` if the file exists, or `Ok(None)` if it does not.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading the existing file fails.
    fn read_file(&self, name: &str) -> Result<Option<String>, MigratoryError> {
        let path = self.dir.join(name);
        if !path.exists() {
            return Ok(None);
        }
        std::fs::read_to_string(&path)
            .map(|s| Some(s.trim().to_string()))
            .map_err(|e| MigratoryError::Generic(e.to_string()))
    }

    /// Writes `content` to a named state file, creating parent directories if needed.
    ///
    /// # Arguments
    ///
    /// * `name` - File name relative to the machine state directory.
    /// * `content` - String content to write.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if directory creation or file writing fails.
    fn write_file(&self, name: &str, content: &str) -> Result<(), MigratoryError> {
        if !self.dir.exists() {
            std::fs::create_dir_all(&self.dir)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        let path = self.dir.join(name);
        std::fs::write(&path, content).map_err(|e| MigratoryError::Generic(e.to_string()))
    }

    /// Deletes a named state file if it exists.
    ///
    /// # Arguments
    ///
    /// * `name` - File name relative to the machine state directory.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if removing the file fails.
    fn delete_file(&self, name: &str) -> Result<(), MigratoryError> {
        let path = self.dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        Ok(())
    }

    /// Gets the persisted hypervisor machine ID.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(id))` if present, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading the `id` file fails.
    pub fn get_id(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("id")
    }

    /// Sets the persisted hypervisor machine ID.
    ///
    /// # Arguments
    ///
    /// * `id` - Hypervisor machine identifier to persist.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing the `id` file fails.
    pub fn set_id(&self, id: &str) -> Result<(), MigratoryError> {
        self.write_file("id", id)
    }

    /// Deletes the persisted hypervisor machine ID.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if removing the `id` file fails.
    pub fn delete_id(&self) -> Result<(), MigratoryError> {
        self.delete_file("id")
    }

    /// Gets the current action name recorded for this machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(action))` if present, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading the `action` file fails.
    pub fn get_action(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("action")
    }

    /// Sets the current action name recorded for this machine.
    ///
    /// # Arguments
    ///
    /// * `action` - Action name to record.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing the `action` file fails.
    pub fn set_action(&self, action: &str) -> Result<(), MigratoryError> {
        self.write_file("action", action)
    }

    /// Gets the creator UID recorded for this machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(uid))` if present, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading the `creator_uid` file fails.
    pub fn get_creator_uid(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("creator_uid")
    }

    /// Sets the creator UID recorded for this machine.
    ///
    /// # Arguments
    ///
    /// * `uid` - Creator user ID string.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing the `creator_uid` file fails.
    pub fn set_creator_uid(&self, uid: &str) -> Result<(), MigratoryError> {
        self.write_file("creator_uid", uid)
    }

    /// Gets the global index UUID recorded for this machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(uuid))` if present, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading the `index_uuid` file fails.
    pub fn get_index_uuid(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("index_uuid")
    }

    /// Sets the global index UUID recorded for this machine.
    ///
    /// # Arguments
    ///
    /// * `uuid` - Global index UUID string.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing the `index_uuid` file fails.
    pub fn set_index_uuid(&self, uuid: &str) -> Result<(), MigratoryError> {
        self.write_file("index_uuid", uuid)
    }

    /// Gets the action provision record.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(data))` if present, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading the `action_provision` file fails.
    pub fn get_action_provision(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("action_provision")
    }

    /// Sets the action provision record.
    ///
    /// # Arguments
    ///
    /// * `data` - Provision marker content.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing the `action_provision` file fails.
    pub fn set_action_provision(&self, data: &str) -> Result<(), MigratoryError> {
        self.write_file("action_provision", data)
    }

    /// Checks whether the `action_provision` marker file exists for this machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if the machine has been marked provisioned, `Ok(false)` otherwise.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if checking the marker fails.
    pub fn has_provisioned(&self) -> Result<bool, MigratoryError> {
        Ok(self.dir.join("action_provision").exists())
    }

    /// Deletes the `action_provision` marker file for this machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if removing the file fails.
    pub fn clear_provisioned(&self) -> Result<(), MigratoryError> {
        self.delete_file("action_provision")
    }

    /// Gets the action set name record.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(name))` if present, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading the `action_set_name` file fails.
    pub fn get_action_set_name(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("action_set_name")
    }

    /// Sets the action set name record.
    ///
    /// # Arguments
    ///
    /// * `name` - Set name string.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing the `action_set_name` file fails.
    pub fn set_action_set_name(&self, name: &str) -> Result<(), MigratoryError> {
        self.write_file("action_set_name", name)
    }

    /// Gets cached synced folders JSON.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(json))` if present, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading the `synced_folders` file fails.
    pub fn get_synced_folders(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("synced_folders")
    }

    /// Sets cached synced folders JSON.
    ///
    /// # Arguments
    ///
    /// * `json_str` - Serialized synced folders JSON.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing the `synced_folders` file fails.
    pub fn set_synced_folders(&self, json_str: &str) -> Result<(), MigratoryError> {
        self.write_file("synced_folders", json_str)
    }

    /// Gets machine-specific private key content.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(key))` if present, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading the `private_key` file fails.
    pub fn get_private_key(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("private_key")
    }

    /// Sets machine-specific private key content.
    ///
    /// # Arguments
    ///
    /// * `key_data` - Private key PEM content.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing the `private_key` file fails.
    pub fn set_private_key(&self, key_data: &str) -> Result<(), MigratoryError> {
        self.write_file("private_key", key_data)
    }

    /// Gets the persisted runtime guest IP address for this machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(ip))` if a runtime IP was persisted, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading the `guest_ip` file fails.
    pub fn get_guest_ip(&self) -> Result<Option<String>, MigratoryError> {
        self.read_file("guest_ip")
    }

    /// Sets the persisted runtime guest IP address for this machine.
    ///
    /// # Arguments
    ///
    /// * `ip` - Guest IP address string.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing the `guest_ip` file fails.
    pub fn set_guest_ip(&self, ip: &str) -> Result<(), MigratoryError> {
        self.write_file("guest_ip", ip)
    }

    /// Gets the persisted dynamically allocated or auto-corrected SSH host port.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(port))` if an SSH port was persisted, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading or parsing the `ssh_port` file fails.
    pub fn get_ssh_port(&self) -> Result<Option<u16>, MigratoryError> {
        match self.read_file("ssh_port")? {
            Some(s) if !s.is_empty() => Ok(Some(s.parse::<u16>()?)),
            _ => Ok(None),
        }
    }

    /// Sets the persisted dynamically allocated or auto-corrected SSH host port.
    ///
    /// # Arguments
    ///
    /// * `port` - Host TCP port mapped to guest SSH.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing the `ssh_port` file fails.
    pub fn set_ssh_port(&self, port: u16) -> Result<(), MigratoryError> {
        self.write_file("ssh_port", &port.to_string())
    }

    /// Gets the persisted list of runtime forwarded ports (including auto-corrected ports).
    ///
    /// # Returns
    ///
    /// Returns `Ok(Vec<PersistedForwardedPort>)` (empty if no `forwarded_ports` file exists).
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading or deserializing `forwarded_ports` fails.
    pub fn get_forwarded_ports(&self) -> Result<Vec<PersistedForwardedPort>, MigratoryError> {
        match self.read_file("forwarded_ports")? {
            Some(json) if !json.is_empty() => {
                // Support both Vec<PersistedForwardedPort> and HashMap<u16, u16> formats
                if let Ok(list) = serde_json::from_str::<Vec<PersistedForwardedPort>>(&json) {
                    Ok(list)
                } else {
                    let map: HashMap<u16, u16> = serde_json::from_str(&json)?;
                    let mut list: Vec<PersistedForwardedPort> = map
                        .into_iter()
                        .map(|(guest, host)| PersistedForwardedPort {
                            guest,
                            host,
                            protocol: "tcp".to_string(),
                            host_ip: None,
                        })
                        .collect();
                    list.sort_by_key(|p| p.guest);
                    Ok(list)
                }
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Persists the list of runtime forwarded ports for this machine.
    ///
    /// # Arguments
    ///
    /// * `ports` - Slice of [`PersistedForwardedPort`] mappings to store.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if serialization or writing fails.
    pub fn set_forwarded_ports(
        &self,
        ports: &[PersistedForwardedPort],
    ) -> Result<(), MigratoryError> {
        let json = serde_json::to_string_pretty(ports)?;
        self.write_file("forwarded_ports", &json)
    }

    /// Looks up the persisted host port for a given `guest_port`.
    ///
    /// Checks `forwarded_ports` first, and if `guest_port == 22`, also checks `ssh_port`.
    ///
    /// # Arguments
    ///
    /// * `guest_port` - The guest port number to look up.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(host_port))` if a persisted mapping exists, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading state files fails.
    pub fn get_forwarded_host_port(&self, guest_port: u16) -> Result<Option<u16>, MigratoryError> {
        let ports = self.get_forwarded_ports()?;
        if let Some(found) = ports.iter().find(|p| p.guest == guest_port) {
            return Ok(Some(found.host));
        }
        if guest_port == 22 {
            return self.get_ssh_port();
        }
        Ok(None)
    }

    /// Deletes all state for this machine and provider.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if removing the state directory fails.
    pub fn delete_all(&self) -> Result<(), MigratoryError> {
        if self.dir.exists() {
            std::fs::remove_dir_all(&self.dir)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        Ok(())
    }

    /// Creates or opens an exclusive lock file for this machine (`.vagrant/machines/<name>/<provider>/lock`).
    ///
    /// # Returns
    ///
    /// Returns an [`RwLock<File>`] handle on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if creating or opening the lock file fails.
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
    ///
    /// # Returns
    ///
    /// Returns an [`RwLock<File>`] handle on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if creating or opening the action lock file fails.
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

/// Manages the `.vagrant` local state directory, per-machine state lookup, and file locking.
pub struct LocalStateManager {
    /// Root `.vagrant` directory path for the project environment.
    dir: PathBuf,
}

impl LocalStateManager {
    /// Creates a new local state manager pointing to a `.vagrant` directory (or equivalent).
    ///
    /// # Arguments
    ///
    /// * `dir` - The path to the local `.vagrant` directory.
    ///
    /// # Returns
    ///
    /// Returns a new [`LocalStateManager`].
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Returns a [`LocalMachineState`] handle for the specified `machine_name` and `provider_name`.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - Logical machine name.
    /// * `provider_name` - Provider name.
    ///
    /// # Returns
    ///
    /// Returns a [`LocalMachineState`] scoped to `.vagrant/machines/<machine_name>/<provider_name>`.
    pub fn machine_state(&self, machine_name: &str, provider_name: &str) -> LocalMachineState {
        LocalMachineState::new(&self.dir, machine_name, provider_name)
    }

    /// Reads the stored ID for a specific machine and provider.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    /// * `provider_name` - The provider name.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(String))` containing the ID if it exists, or `Ok(None)` if it doesn't.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if an I/O error occurs reading the file.
    pub fn read_id(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<Option<String>, MigratoryError> {
        self.machine_state(machine_name, provider_name).get_id()
    }

    /// Writes the ID for a specific machine and provider.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    /// * `provider_name` - The provider name.
    /// * `id` - The machine ID string to write.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if an I/O error occurs creating directories or writing the file.
    pub fn write_id(
        &self,
        machine_name: &str,
        provider_name: &str,
        id: &str,
    ) -> Result<(), MigratoryError> {
        self.machine_state(machine_name, provider_name).set_id(id)
    }

    /// Clears all state for a specific machine and provider by removing its directory.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    /// * `provider_name` - The provider name.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if an I/O error occurs while removing the directory.
    pub fn clear_machine_state(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<(), MigratoryError> {
        self.machine_state(machine_name, provider_name).delete_all()
    }

    /// Checks if a machine has been provisioned under the given provider.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    /// * `provider_name` - The provider name.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if provisioned, `Ok(false)` otherwise.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if checking the provision marker fails.
    pub fn has_provisioned(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<bool, MigratoryError> {
        self.machine_state(machine_name, provider_name)
            .has_provisioned()
    }

    /// Marks a machine as provisioned under the given provider.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    /// * `provider_name` - The provider name.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing the provision marker fails.
    pub fn mark_provisioned(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<(), MigratoryError> {
        self.machine_state(machine_name, provider_name)
            .set_action_provision("")
    }

    /// Clears the provisioned marker for a machine under the given provider.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    /// * `provider_name` - The provider name.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if deleting the provision marker fails.
    pub fn clear_provisioned(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<(), MigratoryError> {
        self.machine_state(machine_name, provider_name)
            .clear_provisioned()
    }

    /// Reads persisted runtime forwarded ports for a machine and provider.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    /// * `provider_name` - The provider name.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Vec<PersistedForwardedPort>)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading or parsing fails.
    pub fn read_forwarded_ports(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<Vec<PersistedForwardedPort>, MigratoryError> {
        self.machine_state(machine_name, provider_name)
            .get_forwarded_ports()
    }

    /// Writes persisted runtime forwarded ports for a machine and provider.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    /// * `provider_name` - The provider name.
    /// * `ports` - Slice of [`PersistedForwardedPort`] entries to persist.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing fails.
    pub fn write_forwarded_ports(
        &self,
        machine_name: &str,
        provider_name: &str,
        ports: &[PersistedForwardedPort],
    ) -> Result<(), MigratoryError> {
        self.machine_state(machine_name, provider_name)
            .set_forwarded_ports(ports)
    }

    /// Reads the persisted runtime guest IP address for a machine and provider.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    /// * `provider_name` - The provider name.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Some(ip))` if present, or `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if reading fails.
    pub fn read_guest_ip(
        &self,
        machine_name: &str,
        provider_name: &str,
    ) -> Result<Option<String>, MigratoryError> {
        self.machine_state(machine_name, provider_name)
            .get_guest_ip()
    }

    /// Writes the persisted runtime guest IP address for a machine and provider.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - The logical name of the machine.
    /// * `provider_name` - The provider name.
    /// * `ip` - Guest IP address string.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if writing fails.
    pub fn write_guest_ip(
        &self,
        machine_name: &str,
        provider_name: &str,
        ip: &str,
    ) -> Result<(), MigratoryError> {
        self.machine_state(machine_name, provider_name)
            .set_guest_ip(ip)
    }

    /// Acquires an exclusive lock on the environment for the lifetime of the process.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns [`MigratoryError::EnvironmentLocked`] if the environment is already locked.
    pub fn lock_process_environment(&self) -> Result<(), MigratoryError> {
        let lock_file = self.create_lock_file()?;
        let static_lock_file = Box::leak(Box::new(lock_file));
        let _guard =
            static_lock_file
                .try_write()
                .map_err(|_| MigratoryError::EnvironmentLocked {
                    path: self.dir.clone(),
                })?;
        Box::leak(Box::new(_guard));
        Ok(())
    }

    /// Acquires an exclusive write lock on `lock`.
    ///
    /// # Arguments
    ///
    /// * `lock` - Mutable reference to an [`RwLock<File>`].
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if acquiring the lock fails.
    pub fn lock(&self, lock: &mut RwLock<File>) -> Result<(), MigratoryError> {
        let _guard = lock.write().map_err(MigratoryError::Io)?;
        Ok(())
    }

    /// Creates or opens the environment lock file (`.vagrant/data/lock`).
    ///
    /// # Returns
    ///
    /// Returns an [`RwLock<File>`] handle on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if directory creation or opening the lock file fails.
    pub fn create_lock_file(&self) -> Result<RwLock<File>, MigratoryError> {
        let lock_path = self.dir.join("data").join("lock");

        let parent = lock_path.parent().unwrap_or(&self.dir);
        fs::create_dir_all(parent)?;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;

        Ok(RwLock::new(file))
    }

    /// Creates or opens an exclusive environment dotlock file (`.vagrant/lock.dotlock`).
    ///
    /// # Returns
    ///
    /// Returns an [`RwLock<File>`] handle on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if creating or opening the dotlock file fails.
    pub fn create_dotlock(&self) -> Result<RwLock<File>, MigratoryError> {
        if !self.dir.exists() {
            fs::create_dir_all(&self.dir)?;
        }
        let lock_path = self.dir.join("lock.dotlock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        Ok(RwLock::new(file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_local_state_lock_concurrency() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let manager1 = LocalStateManager::new(dir.path().to_path_buf());
        let manager2 = LocalStateManager::new(dir.path().to_path_buf());

        let mut lock_file1 = manager1.create_lock_file()?;
        let mut lock_file2 = manager2.create_lock_file()?;

        manager1.lock(&mut lock_file1)?;

        {
            let try_lock2 = lock_file2.try_write();
            let _ = try_lock2;
        }

        let try_lock2_after = lock_file2.try_write();
        assert!(try_lock2_after.is_ok());
        Ok(())
    }

    #[test]
    fn test_local_state_lock() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let manager = LocalStateManager::new(dir.path().to_path_buf());

        let mut lock_file = manager.create_lock_file()?;
        let lock = manager.lock(&mut lock_file);
        assert!(lock.is_ok());

        let dotlock = manager.create_dotlock();
        assert!(dotlock.is_ok());

        let machine_state = LocalMachineState::new(dir.path(), "default", "virtualbox");
        let machine_lock = machine_state.create_machine_lock();
        assert!(machine_lock.is_ok());
        Ok(())
    }

    #[test]
    fn test_local_state_lock_io_error() {
        #[cfg(unix)]
        let manager = LocalStateManager::new(PathBuf::from("/dev/null/data"));
        #[cfg(windows)]
        let manager = LocalStateManager::new(PathBuf::from(r"Z:\invalid\path"));

        let lock_file_res = manager.create_lock_file();
        assert!(lock_file_res.is_err());
    }

    #[test]
    fn test_local_machine_state() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let state = LocalMachineState::new(dir.path(), "default", "virtualbox");

        let lock = state.create_action_lock()?;
        drop(lock);

        assert_eq!(state.get_id()?, None);
        state.set_id("my-id")?;
        assert_eq!(state.get_id()?, Some("my-id".to_string()));
        state.delete_id()?;
        state.delete_id()?;
        assert_eq!(state.get_id()?, None);

        state.set_action("up")?;
        assert_eq!(state.get_action()?, Some("up".to_string()));

        state.set_creator_uid("1000")?;
        assert_eq!(state.get_creator_uid()?, Some("1000".to_string()));

        state.set_index_uuid("some-uuid")?;
        assert_eq!(state.get_index_uuid()?, Some("some-uuid".to_string()));

        state.set_action_provision("hash-123")?;
        assert_eq!(state.get_action_provision()?, Some("hash-123".to_string()));
        assert!(state.has_provisioned()?);
        state.clear_provisioned()?;
        assert!(!state.has_provisioned()?);

        state.set_action_set_name("my-vm-name")?;
        assert_eq!(state.get_action_set_name()?, Some("my-vm-name".to_string()));

        state.set_synced_folders("[]")?;
        assert_eq!(state.get_synced_folders()?, Some("[]".to_string()));

        state.set_private_key("RSA_KEY_DATA")?;
        assert_eq!(state.get_private_key()?, Some("RSA_KEY_DATA".to_string()));

        assert_eq!(state.get_guest_ip()?, None);
        state.set_guest_ip("192.168.56.101")?;
        assert_eq!(state.get_guest_ip()?, Some("192.168.56.101".to_string()));

        assert_eq!(state.get_ssh_port()?, None);
        state.set_ssh_port(2202)?;
        assert_eq!(state.get_ssh_port()?, Some(2202));
        assert_eq!(state.get_forwarded_host_port(22)?, Some(2202));
        assert_eq!(state.get_forwarded_host_port(80)?, None);

        let ports = vec![
            PersistedForwardedPort {
                guest: 22,
                host: 2225,
                protocol: "tcp".to_string(),
                host_ip: Some("127.0.0.1".to_string()),
            },
            PersistedForwardedPort {
                guest: 80,
                host: 8088,
                protocol: "tcp".to_string(),
                host_ip: None,
            },
        ];
        state.set_forwarded_ports(&ports)?;
        assert_eq!(state.get_forwarded_ports()?, ports);
        assert_eq!(state.get_forwarded_host_port(22)?, Some(2225));
        assert_eq!(state.get_forwarded_host_port(80)?, Some(8088));

        // Also test HashMap<u16, u16> fallback deserialization in get_forwarded_ports
        state.write_file("forwarded_ports", r#"{"3389": 13389, "5985": 55985}"#)?;
        let map_ports = state.get_forwarded_ports()?;
        assert_eq!(map_ports.len(), 2);
        assert_eq!(state.get_forwarded_host_port(3389)?, Some(13389));
        assert_eq!(state.get_forwarded_host_port(5985)?, Some(55985));

        state.delete_all()?;
        assert!(!state.dir.exists());
        Ok(())
    }

    #[test]
    fn test_local_state_manager_delegation() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let mgr = LocalStateManager::new(dir.path().to_path_buf());

        assert_eq!(mgr.read_id("web", "virtualbox")?, None);
        mgr.write_id("web", "virtualbox", "vm-123")?;
        assert_eq!(
            mgr.read_id("web", "virtualbox")?,
            Some("vm-123".to_string())
        );

        assert!(!mgr.has_provisioned("web", "virtualbox")?);
        mgr.mark_provisioned("web", "virtualbox")?;
        assert!(mgr.has_provisioned("web", "virtualbox")?);
        mgr.clear_provisioned("web", "virtualbox")?;
        assert!(!mgr.has_provisioned("web", "virtualbox")?);

        assert_eq!(mgr.read_guest_ip("web", "virtualbox")?, None);
        mgr.write_guest_ip("web", "virtualbox", "10.0.0.50")?;
        assert_eq!(
            mgr.read_guest_ip("web", "virtualbox")?,
            Some("10.0.0.50".to_string())
        );

        assert!(mgr.read_forwarded_ports("web", "virtualbox")?.is_empty());
        let ports = vec![PersistedForwardedPort {
            guest: 80,
            host: 8080,
            protocol: "tcp".to_string(),
            host_ip: None,
        }];
        mgr.write_forwarded_ports("web", "virtualbox", &ports)?;
        assert_eq!(mgr.read_forwarded_ports("web", "virtualbox")?, ports);

        mgr.clear_machine_state("web", "virtualbox")?;
        assert_eq!(mgr.read_id("web", "virtualbox")?, None);
        Ok(())
    }

    #[test]
    fn test_local_machine_state_errors() -> Result<(), MigratoryError> {
        #[cfg(unix)]
        let dir = PathBuf::from("/dev/null/env");
        #[cfg(windows)]
        let dir = PathBuf::from(r"Z:\invalid\env");
        let state = LocalMachineState::new(&dir, "default", "virtualbox");

        assert!(state.set_id("my-id").is_err());
        assert_eq!(state.get_id()?, None);
        assert!(state.delete_all().is_ok());
        Ok(())
    }

    #[test]
    fn test_local_state_open_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let manager = LocalStateManager::new(dir.path().to_path_buf());

        let lock_path = dir.path().join("data").join("lock");
        std::fs::create_dir_all(&lock_path)?;

        let lock_file_res = manager.create_lock_file();
        assert!(lock_file_res.is_err());
        Ok(())
    }

    #[test]
    fn test_local_machine_state_io_errors() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let state = LocalMachineState::new(dir.path(), "default", "virtualbox");
        std::fs::create_dir_all(&state.dir)?;

        // 1. read_file fails when target is a directory
        let id_dir = state.dir.join("id");
        std::fs::create_dir(&id_dir)?;
        assert!(state.get_id().is_err());

        // 2. delete_file fails when target is a non-empty directory
        let sub_file = id_dir.join("sub");
        std::fs::write(&sub_file, "content")?;
        assert!(state.delete_id().is_err());

        // 3. write_file fails when target is a directory
        let action_dir = state.dir.join("action");
        std::fs::create_dir(&action_dir)?;
        assert!(state.set_action("up").is_err());

        // 4. create_machine_lock fails when lock is a directory
        let lock_dir = state.dir.join("lock");
        std::fs::create_dir(&lock_dir)?;
        assert!(state.create_machine_lock().is_err());

        // 5. create_action_lock fails when action.lock is a directory
        let action_lock_dir = state.dir.join("action.lock");
        std::fs::create_dir(&action_lock_dir)?;
        assert!(state.create_action_lock().is_err());

        // 6. delete_all fails when dir is a file
        let blocked_state_dir = dir.path().join("blocked_state");
        std::fs::write(&blocked_state_dir, "file")?;
        let blocked_machine = LocalMachineState {
            dir: blocked_state_dir,
        };
        #[cfg(unix)]
        {
            assert!(blocked_machine.delete_all().is_err());
        }

        // 7. create_machine_lock and create_action_lock fail when parent is a file
        let file_parent = dir.path().join("file_parent");
        std::fs::write(&file_parent, "file")?;
        let blocked_parent_state = LocalMachineState {
            dir: file_parent.join("sub"),
        };
        assert!(blocked_parent_state.create_machine_lock().is_err());
        assert!(blocked_parent_state.create_action_lock().is_err());

        // 8. lock_process_environment and create_dotlock on non-existent dir
        let fresh_dir = dir.path().join("fresh_dot_vagrant");
        let fresh_mgr = LocalStateManager::new(fresh_dir.clone());
        assert!(fresh_mgr.create_dotlock().is_ok());
        assert!(fresh_mgr.lock_process_environment().is_ok());
        // Second lock attempt in another thread fails with EnvironmentLocked
        let second_res = std::thread::spawn(move || {
            let mgr2 = LocalStateManager::new(fresh_dir);
            mgr2.lock_process_environment()
        })
        .join();
        assert!(matches!(second_res, Ok(Err(_))));
        Ok(())
    }
}
