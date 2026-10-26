//! Action/Middleware architecture.
//!
//! This module provides an Action Builder (`Warden`) to execute middleware chains
//! for operations.

use crate::error::MigratoryError;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

/// A shared environment for action chains.
#[derive(Default)]
pub struct Environment {
    /// Generic state map for actions to share string data.
    pub data: HashMap<String, String>,
    /// Any typed data.
    pub typed_data: HashMap<String, Arc<dyn Any + Send + Sync>>,
}

impl Environment {
    /// Creates a new, empty `Environment`.
    ///
    /// # Returns
    ///
    /// Returns a newly initialized [`Environment`] with empty data maps.
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            typed_data: HashMap::new(),
        }
    }

    /// Inserts a strongly-typed value into `typed_data`.
    ///
    /// # Arguments
    ///
    /// * `key` - Environment key name.
    /// * `val` - Value to store.
    pub fn insert_typed<T: Any + Send + Sync>(&mut self, key: &str, val: T) {
        self.typed_data.insert(key.to_string(), Arc::new(val));
    }

    /// Retrieves a reference to a strongly-typed value from `typed_data`.
    ///
    /// # Arguments
    ///
    /// * `key` - Environment key name.
    ///
    /// # Returns
    ///
    /// Returns `Some(&T)` if the key exists and matches type `T`, or `None` otherwise.
    pub fn get_typed<T: Any + Send + Sync>(&self, key: &str) -> Option<&T> {
        self.typed_data.get(key)?.downcast_ref::<T>()
    }
}

/// The result of an `Action`'s `call` method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionResult {
    /// Continue to the next action in the chain.
    Continue,
    /// Abort the chain immediately, but return `Ok`.
    Halt,
}

/// A trait representing a single step (middleware) in an action chain.
pub trait Action: Send + Sync {
    /// Returns the name of this action for debugging and logging.
    ///
    /// # Returns
    ///
    /// Returns the action's identifier string slice.
    fn name(&self) -> &str;

    /// Called during the forward ("up") phase of the middleware chain.
    ///
    /// # Arguments
    ///
    /// * `env` - Shared mutable action environment.
    ///
    /// # Returns
    ///
    /// Returns [`ActionResult::Continue`] to proceed or [`ActionResult::Halt`] to stop cleanly.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the action fails and recovery should be triggered.
    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError>;

    /// Called during the backward ("down" / recovery / rollback) phase of the middleware chain.
    ///
    /// # Arguments
    ///
    /// * `_env` - Shared mutable action environment.
    /// * `_error` - The error that triggered recovery.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` when rollback succeeds.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if rollback itself fails.
    fn recover(
        &self,
        _env: &mut Environment,
        _error: &MigratoryError,
    ) -> Result<(), MigratoryError> {
        Ok(())
    }
}

/// Builds and configures an action middleware pipeline matching Vagrant's `Action::Builder`.
#[derive(Default)]
pub struct ActionBuilder {
    /// Ordered sequence of middleware actions in the pipeline.
    actions: Vec<Box<dyn Action>>,
}

impl ActionBuilder {
    /// Creates a new, empty `ActionBuilder`.
    ///
    /// # Returns
    ///
    /// Returns a new [`ActionBuilder`].
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    /// Appends an action to the end of the pipeline.
    ///
    /// # Arguments
    ///
    /// * `action` - Boxed action to append.
    ///
    /// # Returns
    ///
    /// Returns a mutable reference to `self` for method chaining.
    pub fn use_action(&mut self, action: Box<dyn Action>) -> &mut Self {
        self.actions.push(action);
        self
    }

    /// Prepends an action to the beginning of the pipeline.
    ///
    /// # Arguments
    ///
    /// * `action` - Boxed action to prepend.
    ///
    /// # Returns
    ///
    /// Returns a mutable reference to `self` for method chaining.
    pub fn prepend(&mut self, action: Box<dyn Action>) -> &mut Self {
        self.actions.insert(0, action);
        self
    }

    /// Inserts an action before the action named `target_name`.
    ///
    /// # Arguments
    ///
    /// * `target_name` - Name of the existing action to insert before.
    /// * `action` - Boxed action to insert.
    ///
    /// # Returns
    ///
    /// Returns `Ok(&mut Self)` if `target_name` was found and the action was inserted.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError::NotFound` if `target_name` does not exist in the pipeline.
    pub fn insert_before(
        &mut self,
        target_name: &str,
        action: Box<dyn Action>,
    ) -> Result<&mut Self, MigratoryError> {
        if let Some(pos) = self.actions.iter().position(|a| a.name() == target_name) {
            self.actions.insert(pos, action);
            Ok(self)
        } else {
            Err(MigratoryError::NotFound(format!(
                "Action '{}' not found",
                target_name
            )))
        }
    }

    /// Inserts an action after the action named `target_name`.
    ///
    /// # Arguments
    ///
    /// * `target_name` - Name of the existing action to insert after.
    /// * `action` - Boxed action to insert.
    ///
    /// # Returns
    ///
    /// Returns `Ok(&mut Self)` if `target_name` was found and the action was inserted.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError::NotFound` if `target_name` does not exist in the pipeline.
    pub fn insert_after(
        &mut self,
        target_name: &str,
        action: Box<dyn Action>,
    ) -> Result<&mut Self, MigratoryError> {
        if let Some(pos) = self.actions.iter().position(|a| a.name() == target_name) {
            self.actions.insert(pos + 1, action);
            Ok(self)
        } else {
            Err(MigratoryError::NotFound(format!(
                "Action '{}' not found",
                target_name
            )))
        }
    }

    /// Replaces an action named `target_name` with a new action.
    ///
    /// # Arguments
    ///
    /// * `target_name` - Name of the existing action to replace.
    /// * `action` - Replacement boxed action.
    ///
    /// # Returns
    ///
    /// Returns `Ok(&mut Self)` if `target_name` was found and replaced.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError::NotFound` if `target_name` does not exist in the pipeline.
    pub fn replace(
        &mut self,
        target_name: &str,
        action: Box<dyn Action>,
    ) -> Result<&mut Self, MigratoryError> {
        if let Some(pos) = self.actions.iter().position(|a| a.name() == target_name) {
            self.actions[pos] = action;
            Ok(self)
        } else {
            Err(MigratoryError::NotFound(format!(
                "Action '{}' not found",
                target_name
            )))
        }
    }

    /// Deletes an action named `target_name` from the pipeline.
    ///
    /// # Arguments
    ///
    /// * `target_name` - Name of the action to remove.
    ///
    /// # Returns
    ///
    /// Returns `Ok(&mut Self)` if `target_name` was found and removed.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError::NotFound` if `target_name` does not exist in the pipeline.
    pub fn delete(&mut self, target_name: &str) -> Result<&mut Self, MigratoryError> {
        if let Some(pos) = self.actions.iter().position(|a| a.name() == target_name) {
            self.actions.remove(pos);
            Ok(self)
        } else {
            Err(MigratoryError::NotFound(format!(
                "Action '{}' not found",
                target_name
            )))
        }
    }

    /// Returns the number of actions in the pipeline.
    ///
    /// # Returns
    ///
    /// Returns the count of registered actions.
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Returns `true` if the pipeline contains no actions.
    ///
    /// # Returns
    ///
    /// Returns `true` when `self.len() == 0`.
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Converts this builder into an executable [`Warden`].
    ///
    /// # Returns
    ///
    /// Returns a [`Warden`] containing all actions configured on this builder.
    pub fn to_warden(self) -> Warden {
        let mut warden = Warden::new();
        for action in self.actions {
            warden.use_action(action);
        }
        warden
    }
}

/// Extensible action hooks matching Vagrant's `Action::Hook`.
///
/// External plugins and extensions can register hooks into an action stack.
#[derive(Default)]
pub struct ActionHook {
    /// Hooks registered to be inserted before a named target action.
    before_hooks: Vec<(String, Box<dyn Action>)>,
    /// Hooks registered to be inserted after a named target action.
    after_hooks: Vec<(String, Box<dyn Action>)>,
    /// Hooks registered to be prepended to the start of the pipeline.
    prepend_hooks: Vec<Box<dyn Action>>,
    /// Hooks registered to be appended to the end of the pipeline.
    append_hooks: Vec<Box<dyn Action>>,
}

impl ActionHook {
    /// Creates a new empty `ActionHook`.
    ///
    /// # Returns
    ///
    /// Returns an empty [`ActionHook`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an action to run before `target_name`.
    ///
    /// # Arguments
    ///
    /// * `target_name` - Name of the target action.
    /// * `action` - Action to insert before `target_name`.
    pub fn before(&mut self, target_name: &str, action: Box<dyn Action>) {
        self.before_hooks.push((target_name.to_string(), action));
    }

    /// Registers an action to run after `target_name`.
    ///
    /// # Arguments
    ///
    /// * `target_name` - Name of the target action.
    /// * `action` - Action to insert after `target_name`.
    pub fn after(&mut self, target_name: &str, action: Box<dyn Action>) {
        self.after_hooks.push((target_name.to_string(), action));
    }

    /// Registers an action to prepend to the start of the action stack.
    ///
    /// # Arguments
    ///
    /// * `action` - Action to prepend.
    pub fn prepend(&mut self, action: Box<dyn Action>) {
        self.prepend_hooks.push(action);
    }

    /// Registers an action to append to the end of the action stack.
    ///
    /// # Arguments
    ///
    /// * `action` - Action to append.
    pub fn append(&mut self, action: Box<dyn Action>) {
        self.append_hooks.push(action);
    }

    /// Applies all registered hooks onto the target `ActionBuilder`, ignoring missing targets.
    ///
    /// # Arguments
    ///
    /// * `builder` - Mutable [`ActionBuilder`] to modify.
    pub fn apply(self, builder: &mut ActionBuilder) {
        let _ = self.try_apply(builder);
    }

    /// Applies all registered hooks onto the target `ActionBuilder`, returning an error if any target action is missing.
    ///
    /// # Arguments
    ///
    /// * `builder` - Mutable [`ActionBuilder`] to modify.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if all hooks were applied cleanly.
    ///
    /// # Errors
    ///
    /// Returns [`MigratoryError::NotFound`] if a `before` or `after` hook references an unknown target action.
    pub fn try_apply(self, builder: &mut ActionBuilder) -> Result<(), MigratoryError> {
        for action in self.prepend_hooks {
            builder.prepend(action);
        }
        for (target, action) in self.before_hooks {
            builder.insert_before(&target, action)?;
        }
        for (target, action) in self.after_hooks {
            builder.insert_after(&target, action)?;
        }
        for action in self.append_hooks {
            builder.use_action(action);
        }
        Ok(())
    }
}

/// The `Warden` manages a chain of `Action`s and executes them.
#[derive(Default)]
pub struct Warden {
    /// Ordered list of middleware actions to execute.
    actions: Vec<Box<dyn Action>>,
}

impl Warden {
    /// Creates a new, empty `Warden`.
    ///
    /// # Returns
    ///
    /// Returns an empty [`Warden`].
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    /// Appends an action to the chain.
    ///
    /// # Arguments
    ///
    /// * `action` - Boxed action to append.
    pub fn use_action(&mut self, action: Box<dyn Action>) {
        self.actions.push(action);
    }

    /// Executes the middleware chain.
    ///
    /// # Arguments
    ///
    /// * `env` - Shared mutable environment passed through each action.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the chain completes or halts cleanly.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if an action fails (after running recovery on prior actions).
    pub fn call(&self, env: &mut Environment) -> Result<(), MigratoryError> {
        self.call_with_interrupt(env, None)
    }

    /// Executes the middleware chain, checking for cancellation / `SIGINT` via `interrupted`.
    ///
    /// If an interrupt is detected, execution halts and recovery unwinds in reverse.
    ///
    /// # Arguments
    ///
    /// * `env` - Shared mutable environment passed through each action.
    /// * `interrupted` - Optional atomic flag indicating an interrupt signal was received.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the chain completes without error or interruption.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if interrupted or if an action fails.
    pub fn call_with_interrupt(
        &self,
        env: &mut Environment,
        interrupted: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<(), MigratoryError> {
        let mut executed_actions = Vec::new();
        let mut error_occurred = None;

        for action in &self.actions {
            let _ = action.name();
            if let Some(flag) = interrupted
                && flag.load(std::sync::atomic::Ordering::SeqCst)
            {
                error_occurred = Some(MigratoryError::Generic(
                    "Operation interrupted by signal (SIGINT)".to_string(),
                ));
                break;
            }

            match action.call(env) {
                Ok(ActionResult::Continue) => {
                    executed_actions.push(action);
                }
                Ok(ActionResult::Halt) => {
                    executed_actions.push(action);
                    break;
                }
                Err(err) => {
                    executed_actions.push(action);
                    error_occurred = Some(err);
                    break;
                }
            }
        }

        if let Some(err) = error_occurred {
            // Recover phase
            for action in executed_actions.into_iter().rev() {
                if let Err(recover_err) = action.recover(env, &err) {
                    return Err(MigratoryError::Generic(format!(
                        "Error during recovery: {}, original error: {}",
                        recover_err, err
                    )));
                }
            }
            Err(err)
        } else {
            Ok(())
        }
    }
}

/// A middleware action that checks the state of the machine before allowing an operation.
pub struct CheckMachineStateAction {
    /// The list of acceptable states (e.g., `"running"`, `"saved"`).
    pub expected_states: Vec<String>,
    /// The name of the machine being checked.
    pub machine_name: String,
    /// The provider used by the machine.
    pub provider_name: String,
    /// The working directory for reading state.
    pub cwd: std::path::PathBuf,
}

impl Action for CheckMachineStateAction {
    fn name(&self) -> &str {
        "CheckMachineStateAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        let state_str = if let Some(override_state) =
            env.get_typed::<crate::provider::MachineState>("machine_state")
        {
            override_state.as_str().to_string()
        } else {
            let state_mgr = crate::state::local::LocalStateManager::new(self.cwd.join(".vagrant"));
            let machine_id = state_mgr
                .read_id(&self.machine_name, &self.provider_name)
                .ok()
                .flatten();

            let p = crate::provider::get_provider(&self.provider_name, machine_id)?;
            let state = p
                .status()
                .unwrap_or(crate::provider::MachineState::NotCreated);
            state.as_str().to_string()
        };

        if !self.expected_states.contains(&state_str) {
            return Err(MigratoryError::Generic(format!(
                "The VM '{}' is in state '{}', but expected one of: {:?}",
                self.machine_name, state_str, self.expected_states
            )));
        }

        Ok(ActionResult::Continue)
    }
}

/// Conditional branching action matching Vagrant's `Action::Builtin::Call`.
pub struct CallAction {
    /// Predicate checking if the branch should be executed.
    pub condition: Box<dyn Fn(&Environment) -> bool + Send + Sync>,
    /// Action to execute when condition is met.
    pub branch_action: Box<dyn Action>,
}

impl Action for CallAction {
    fn name(&self) -> &str {
        "CallAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        if (self.condition)(env) {
            self.branch_action.call(env)
        } else {
            Ok(ActionResult::Continue)
        }
    }

    fn recover(&self, env: &mut Environment, error: &MigratoryError) -> Result<(), MigratoryError> {
        if (self.condition)(env) {
            self.branch_action.recover(env, error)
        } else {
            Ok(())
        }
    }
}

/// Action that validates the structural integrity and options of the machine configuration.
pub struct ConfigValidateAction;

impl Action for ConfigValidateAction {
    fn name(&self) -> &str {
        "ConfigValidateAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        if let Some(err) = env.data.get("config_error") {
            return Err(MigratoryError::Validation(err.clone()));
        }
        Ok(ActionResult::Continue)
    }
}

/// Action that checks if a newer box version is available on Vagrant Cloud before boot.
pub struct BoxCheckOutdatedAction {
    /// Name of the box.
    pub box_name: String,
}

impl Action for BoxCheckOutdatedAction {
    fn name(&self) -> &str {
        "BoxCheckOutdatedAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        if let Some(vagrant_home) = env.get_typed::<std::path::PathBuf>("vagrant_home") {
            let mgr = crate::box_manager::BoxManager::new(vagrant_home);
            let outdated = mgr.outdated(&self.box_name)?;
            env.data
                .insert("box_outdated".to_string(), outdated.to_string());
        }
        env.data
            .insert("box_checked".to_string(), self.box_name.clone());
        Ok(ActionResult::Continue)
    }
}

/// Action that verifies base box availability, downloading and extracting it if missing.
pub struct HandleBoxAction {
    /// Name of the box.
    pub box_name: String,
}

impl Action for HandleBoxAction {
    fn name(&self) -> &str {
        "HandleBoxAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        if let Some(vagrant_home) = env.get_typed::<std::path::PathBuf>("vagrant_home") {
            let mgr = crate::box_manager::BoxManager::new(vagrant_home);
            let safe_name = self.box_name.replace('/', "-VAGRANTSLASH-");
            let box_dir = mgr.global_boxes_dir.join(safe_name);
            env.data
                .insert("box_exists".to_string(), box_dir.exists().to_string());
        }
        env.data
            .insert("box_handled".to_string(), self.box_name.clone());
        Ok(ActionResult::Continue)
    }
}

/// Action that detects and auto-corrects host forwarded port collisions.
pub struct HandleForwardedPortCollisionsAction;

impl Action for HandleForwardedPortCollisionsAction {
    fn name(&self) -> &str {
        "HandleForwardedPortCollisionsAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        let _lock = PORT_COLLISION_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(networks) = env
            .get_typed::<Vec<crate::config::NetworkConfig>>("networks")
            .cloned()
        {
            let mut open_ports = Vec::new();
            for net in &networks {
                if matches!(net, crate::config::NetworkConfig::ForwardedPort { .. }) {
                    let res = crate::network::check_forwarded_port(net, &open_ports)?;
                    open_ports.push(res.corrected_host_port);
                }
            }
            env.insert_typed("resolved_ports", open_ports);
        }
        env.data
            .insert("port_collisions_checked".to_string(), "true".to_string());
        Ok(ActionResult::Continue)
    }
}

/// Action that sets and persists the guest hostname.
pub struct SetHostnameAction {
    /// The hostname to set.
    pub hostname: String,
}

impl Action for SetHostnameAction {
    fn name(&self) -> &str {
        "SetHostnameAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        if let Some(comm) =
            env.get_typed::<Arc<dyn crate::communicator::Communicator>>("communicator")
        {
            let guest = crate::guest::linux::LinuxGuest;
            crate::guest::Guest::change_hostname(&guest, comm.as_ref(), &self.hostname)?;
        }
        env.data
            .insert("hostname_set".to_string(), self.hostname.clone());
        Ok(ActionResult::Continue)
    }
}

/// Action that polls the guest communicator until it is responsive or times out.
pub struct WaitForCommunicatorAction {
    /// Maximum duration to wait before timing out.
    pub timeout: std::time::Duration,
}

impl Action for WaitForCommunicatorAction {
    fn name(&self) -> &str {
        "WaitForCommunicatorAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        if let Some(comm) =
            env.get_typed::<Arc<dyn crate::communicator::Communicator>>("communicator")
        {
            comm.wait_for_ready(self.timeout)?;
        }
        env.data.insert(
            "communicator_ready".to_string(),
            format!("{}s", self.timeout.as_secs()),
        );
        Ok(ActionResult::Continue)
    }
}

/// Action that generates and injects a fresh secure SSH key pair.
pub struct GenerateKeyPairAction;

impl Action for GenerateKeyPairAction {
    fn name(&self) -> &str {
        "GenerateKeyPairAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        if let Some(key_path) = env.get_typed::<std::path::PathBuf>("private_key_path") {
            let ssh_comm =
                crate::communicator::ssh::SshCommunicator::new(crate::config::SshConfig::default());
            let _ = ssh_comm.replace_insecure_key(key_path);
        }
        env.data
            .insert("key_pair_generated".to_string(), "true".to_string());
        Ok(ActionResult::Continue)
    }
}

/// Action that coordinates host preparation and guest mounting of synced folders.
pub struct SyncFoldersAction;

impl Action for SyncFoldersAction {
    fn name(&self) -> &str {
        "SyncFoldersAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        if let Some((comm, folders)) = env
            .get_typed::<Arc<dyn crate::communicator::Communicator>>("communicator")
            .cloned()
            .zip(
                env.get_typed::<Vec<crate::config::SyncedFolderConfig>>("synced_folders")
                    .cloned(),
            )
        {
            let machine_id = env
                .data
                .get("machine_id")
                .cloned()
                .or_else(|| Some("default".to_string()));
            for sf in &folders {
                if !sf.disabled {
                    let opts = crate::synced_folder::SyncedFolderOptions {
                        host_path: sf.host_path.to_string_lossy().to_string(),
                        guest_path: sf.guest_path.to_string_lossy().to_string(),
                        ..Default::default()
                    };
                    let vbox_sf =
                        crate::synced_folder::vbox::VboxSyncedFolder::new(machine_id.clone());
                    use crate::synced_folder::SyncedFolder;
                    vbox_sf.prepare(&opts)?;
                    vbox_sf.mount(&opts, comm.as_ref())?;
                }
            }
        }
        env.data
            .insert("folders_synced".to_string(), "true".to_string());
        Ok(ActionResult::Continue)
    }
}

/// Action that executes configured provisioners sequentially.
pub struct ProvisionAction;

impl Action for ProvisionAction {
    fn name(&self) -> &str {
        "ProvisionAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        if let Some((comm, provisioners)) = env
            .get_typed::<Arc<dyn crate::communicator::Communicator>>("communicator")
            .cloned()
            .zip(
                env.get_typed::<Vec<crate::config::ProvisionerConfig>>("provisioners")
                    .cloned(),
            )
        {
            for p_cfg in &provisioners {
                if p_cfg.name == "shell" {
                    let mut shell = crate::provisioner::shell::ShellProvisioner::new();
                    use crate::provisioner::Provisioner;
                    shell.prepare(&p_cfg.config)?;
                    shell.provision(comm.as_ref())?;
                }
            }
        }
        env.data
            .insert("provisioned".to_string(), "true".to_string());
        Ok(ActionResult::Continue)
    }
}

/// Action that cleans up stale NFS export rules on host during halt/destroy.
pub struct PruneNfsExportsAction;

impl Action for PruneNfsExportsAction {
    fn name(&self) -> &str {
        "PruneNfsExportsAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        let _lock = NFS_EXPORTS_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = crate::host::detect_host().and_then(|host| host.prune_nfs_exports());
        env.data
            .insert("nfs_exports_pruned".to_string(), "true".to_string());
        Ok(ActionResult::Continue)
    }
}

/// Thread-safe multi-machine orchestrator.
///
/// Coordinates execution of operations across multiple VMs, supporting
/// parallel and sequential execution modes.
pub struct MachineOrchestrator;

impl MachineOrchestrator {
    /// Executes a task across machines sequentially or in parallel.
    ///
    /// # Arguments
    ///
    /// * `machines` - Slice of machine names.
    /// * `parallel` - Whether to run concurrently in parallel threads.
    /// * `task` - Closure to execute per machine.
    ///
    /// # Returns
    ///
    /// `Ok(())` if all machines succeed, or the first error encountered.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if any machine task returns an error or panics.
    pub fn run<F>(machines: &[String], parallel: bool, task: F) -> Result<(), MigratoryError>
    where
        F: Fn(&str) -> Result<(), MigratoryError> + Send + Sync,
    {
        Self::run_inner(machines, parallel, &task)
    }

    /// Internal non-generic implementation of machine orchestration using dynamic dispatch.
    ///
    /// # Arguments
    ///
    /// * `machines` - Slice of machine names.
    /// * `parallel` - Whether to run concurrently in parallel threads.
    /// * `task` - Trait object reference to execute per machine.
    ///
    /// # Returns
    ///
    /// `Ok(())` if all machines succeed, or the first error encountered.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if any machine task returns an error or panics.
    fn run_inner(
        machines: &[String],
        parallel: bool,
        task: &(dyn Fn(&str) -> Result<(), MigratoryError> + Send + Sync),
    ) -> Result<(), MigratoryError> {
        if machines.is_empty() {
            return Ok(());
        }

        if !parallel || machines.len() == 1 {
            for m in machines {
                task(m)?;
            }
            return Ok(());
        }

        std::thread::scope(|s| {
            let mut handles = Vec::new();
            for m in machines {
                let handle = s.spawn(move || {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task(m)))
                });
                handles.push(handle);
            }

            let mut first_err = None;
            for h in handles {
                match h.join() {
                    Ok(Ok(Ok(()))) => {}
                    Ok(Ok(Err(e))) => {
                        if first_err.is_none() {
                            first_err = Some(e);
                        }
                    }
                    _ => {
                        if first_err.is_none() {
                            first_err = Some(MigratoryError::Generic(
                                "Thread panicked during parallel machine execution".to_string(),
                            ));
                        }
                    }
                }
            }

            if let Some(err) = first_err {
                Err(err)
            } else {
                Ok(())
            }
        })
    }
}

/// Global mutual exclusion lock protecting host-only network interface creation/deletion.
pub static HOST_ONLY_NET_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Global mutual exclusion lock protecting port allocation collision checks.
pub static PORT_COLLISION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Global mutual exclusion lock protecting `/etc/exports` file editing.
pub static NFS_EXPORTS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    /// Configurable mock action for testing [`Warden`] and [`ActionBuilder`].
    struct MockAction {
        /// Action identifier name.
        name: String,
        /// Whether `call` should return an error.
        should_fail: bool,
        /// Whether `call` should return [`ActionResult::Halt`].
        should_halt: bool,
    }

    impl Action for MockAction {
        fn name(&self) -> &str {
            &self.name
        }

        fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
            env.data
                .insert(format!("{}_called", self.name), "true".to_string());
            if self.should_fail {
                Err(MigratoryError::Generic(format!("{} failed", self.name)))
            } else if self.should_halt {
                Ok(ActionResult::Halt)
            } else {
                Ok(ActionResult::Continue)
            }
        }

        fn recover(
            &self,
            env: &mut Environment,
            _error: &MigratoryError,
        ) -> Result<(), MigratoryError> {
            env.data
                .insert(format!("{}_recovered", self.name), "true".to_string());
            Ok(())
        }
    }

    #[test]
    fn test_environment() {
        let mut env = Environment::new();
        env.data.insert("key".to_string(), "value".to_string());
        assert_eq!(env.data.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_warden_continue() {
        let action1 = MockAction {
            name: "action1".to_string(),
            should_fail: false,
            should_halt: false,
        };
        assert_eq!(action1.name(), "action1");

        let mut warden = Warden::new();
        warden.use_action(Box::new(action1));
        warden.use_action(Box::new(MockAction {
            name: "action2".to_string(),
            should_fail: false,
            should_halt: false,
        }));

        let mut env = Environment::new();
        let res = warden.call(&mut env);
        assert!(res.is_ok());
        assert_eq!(env.data.get("action1_called"), Some(&"true".to_string()));
        assert_eq!(env.data.get("action2_called"), Some(&"true".to_string()));
    }

    #[test]
    fn test_warden_halt() {
        let mut warden = Warden::new();
        warden.use_action(Box::new(MockAction {
            name: "action1".to_string(),
            should_fail: false,
            should_halt: true,
        }));
        warden.use_action(Box::new(MockAction {
            name: "action2".to_string(),
            should_fail: false,
            should_halt: false,
        }));

        let mut env = Environment::new();
        let res = warden.call(&mut env);
        assert!(res.is_ok());
        assert_eq!(env.data.get("action1_called"), Some(&"true".to_string()));
        assert!(!env.data.contains_key("action2_called"));
    }

    #[test]
    fn test_warden_fail_and_recover() {
        let mut warden = Warden::new();
        warden.use_action(Box::new(MockAction {
            name: "action1".to_string(),
            should_fail: false,
            should_halt: false,
        }));
        warden.use_action(Box::new(MockAction {
            name: "action2".to_string(),
            should_fail: true,
            should_halt: false,
        }));

        let mut env = Environment::new();
        let res = warden.call(&mut env);
        assert!(res.is_err());
        assert_eq!(env.data.get("action1_called"), Some(&"true".to_string()));
        assert_eq!(env.data.get("action2_called"), Some(&"true".to_string()));
        assert_eq!(env.data.get("action1_recovered"), Some(&"true".to_string()));
        assert_eq!(env.data.get("action2_recovered"), Some(&"true".to_string()));
    }

    /// Mock action that fails both `call` and `recover`.
    struct MockRecoverFailAction;
    impl Action for MockRecoverFailAction {
        fn name(&self) -> &str {
            "fail_recover"
        }
        fn call(&self, _env: &mut Environment) -> Result<ActionResult, MigratoryError> {
            Err(MigratoryError::Generic("Initial failure".to_string()))
        }
        fn recover(
            &self,
            _env: &mut Environment,
            _error: &MigratoryError,
        ) -> Result<(), MigratoryError> {
            Err(MigratoryError::Generic("Recover failure".to_string()))
        }
    }

    #[test]
    fn test_warden_recover_failure() {
        let action = MockRecoverFailAction;
        assert_eq!(action.name(), "fail_recover");

        let mut warden = Warden::new();
        warden.use_action(Box::new(action));
        let mut env = Environment::new();
        let res = warden.call(&mut env);
        assert!(res.is_err());
        let err_msg = res.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err_msg.contains("Recover failure"));
        assert!(err_msg.contains("Initial failure"));
    }

    #[test]
    fn test_action_default_recover() {
        /// Minimal action using the default `recover` implementation.
        struct BasicAction;
        impl Action for BasicAction {
            fn name(&self) -> &str {
                "basic"
            }
            fn call(&self, _env: &mut Environment) -> Result<ActionResult, MigratoryError> {
                Ok(ActionResult::Continue)
            }
        }
        let action = BasicAction;
        assert_eq!(action.name(), "basic");
        let mut env = Environment::new();
        assert_eq!(action.call(&mut env).ok(), Some(ActionResult::Continue));
        let err = MigratoryError::Generic("test".to_string());
        assert!(action.recover(&mut env, &err).is_ok());
    }

    #[test]
    fn test_defaults() {
        let env = Environment::default();
        assert!(env.data.is_empty());
        assert!(env.typed_data.is_empty());

        let warden = Warden::default();
        assert!(warden.actions.is_empty());
    }

    #[test]
    fn test_action_builder_and_hooks() {
        let mut builder = ActionBuilder::new();
        assert!(builder.is_empty());
        assert_eq!(builder.len(), 0);

        builder.use_action(Box::new(MockAction {
            name: "mid".to_string(),
            should_fail: false,
            should_halt: false,
        }));
        assert_eq!(builder.len(), 1);

        builder.prepend(Box::new(MockAction {
            name: "first".to_string(),
            should_fail: false,
            should_halt: false,
        }));
        assert_eq!(builder.actions[0].name(), "first");
        assert_eq!(builder.actions[1].name(), "mid");

        assert!(
            builder
                .insert_before(
                    "mid",
                    Box::new(MockAction {
                        name: "pre_mid".to_string(),
                        should_fail: false,
                        should_halt: false,
                    })
                )
                .is_ok()
        );
        assert_eq!(builder.actions[1].name(), "pre_mid");

        assert!(
            builder
                .insert_after(
                    "mid",
                    Box::new(MockAction {
                        name: "post_mid".to_string(),
                        should_fail: false,
                        should_halt: false,
                    })
                )
                .is_ok()
        );
        assert_eq!(builder.actions[3].name(), "post_mid");

        // Error cases for not found
        assert!(
            builder
                .insert_before(
                    "nonexistent",
                    Box::new(MockAction {
                        name: "x".to_string(),
                        should_fail: false,
                        should_halt: false,
                    })
                )
                .is_err()
        );
        assert!(
            builder
                .insert_after(
                    "nonexistent",
                    Box::new(MockAction {
                        name: "x".to_string(),
                        should_fail: false,
                        should_halt: false,
                    })
                )
                .is_err()
        );
        assert!(
            builder
                .replace(
                    "nonexistent",
                    Box::new(MockAction {
                        name: "x".to_string(),
                        should_fail: false,
                        should_halt: false,
                    })
                )
                .is_err()
        );
        assert!(builder.delete("nonexistent").is_err());

        // Replace and delete
        assert!(
            builder
                .replace(
                    "pre_mid",
                    Box::new(MockAction {
                        name: "new_pre_mid".to_string(),
                        should_fail: false,
                        should_halt: false,
                    })
                )
                .is_ok()
        );
        assert_eq!(builder.actions[1].name(), "new_pre_mid");

        assert!(builder.delete("new_pre_mid").is_ok());
        assert_eq!(builder.len(), 3);

        // Convert to warden and execute
        let warden = builder.to_warden();
        let mut env = Environment::new();
        assert!(warden.call(&mut env).is_ok());
        assert_eq!(env.data.get("first_called"), Some(&"true".to_string()));
        assert_eq!(env.data.get("mid_called"), Some(&"true".to_string()));
        assert_eq!(env.data.get("post_mid_called"), Some(&"true".to_string()));
    }

    #[test]
    fn test_action_hook_apply() {
        let mut builder = ActionBuilder::new();
        builder.use_action(Box::new(MockAction {
            name: "target".to_string(),
            should_fail: false,
            should_halt: false,
        }));

        let mut hook = ActionHook::new();
        hook.prepend(Box::new(MockAction {
            name: "hook_prepend".to_string(),
            should_fail: false,
            should_halt: false,
        }));
        hook.before(
            "target",
            Box::new(MockAction {
                name: "hook_before".to_string(),
                should_fail: false,
                should_halt: false,
            }),
        );
        hook.after(
            "target",
            Box::new(MockAction {
                name: "hook_after".to_string(),
                should_fail: false,
                should_halt: false,
            }),
        );
        hook.append(Box::new(MockAction {
            name: "hook_append".to_string(),
            should_fail: false,
            should_halt: false,
        }));

        hook.apply(&mut builder);
        assert_eq!(builder.len(), 5);
        assert_eq!(builder.actions[0].name(), "hook_prepend");
        assert_eq!(builder.actions[1].name(), "hook_before");
        assert_eq!(builder.actions[2].name(), "target");
        assert_eq!(builder.actions[3].name(), "hook_after");
        assert_eq!(builder.actions[4].name(), "hook_append");
    }

    #[test]
    fn test_warden_interrupt_signal() {
        let mut warden = Warden::new();
        warden.use_action(Box::new(MockAction {
            name: "action1".to_string(),
            should_fail: false,
            should_halt: false,
        }));
        warden.use_action(Box::new(MockAction {
            name: "action2".to_string(),
            should_fail: false,
            should_halt: false,
        }));

        let interrupted = std::sync::atomic::AtomicBool::new(true);
        let mut env = Environment::new();
        let res = warden.call_with_interrupt(&mut env, Some(&interrupted));
        assert!(res.is_err());
        assert!(
            res.err()
                .map(|e| e.to_string())
                .unwrap_or_default()
                .contains("SIGINT")
        );
        // Interrupted before first action ran, so neither called
        assert!(!env.data.contains_key("action1_called"));

        /// Action that sets the interrupt flag mid-execution.
        struct InterruptingAction {
            /// Shared interrupt flag.
            flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
        }
        impl Action for InterruptingAction {
            fn name(&self) -> &str {
                "interrupter"
            }
            fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
                self.flag.store(true, std::sync::atomic::Ordering::SeqCst);
                env.data
                    .insert("interrupter_called".to_string(), "true".to_string());
                Ok(ActionResult::Continue)
            }
            fn recover(
                &self,
                env: &mut Environment,
                _err: &MigratoryError,
            ) -> Result<(), MigratoryError> {
                env.data
                    .insert("interrupter_recovered".to_string(), "true".to_string());
                Ok(())
            }
        }

        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut warden2 = Warden::new();
        warden2.use_action(Box::new(InterruptingAction {
            flag: std::sync::Arc::clone(&flag),
        }));
        warden2.use_action(Box::new(MockAction {
            name: "never".to_string(),
            should_fail: false,
            should_halt: false,
        }));

        let mut env2 = Environment::new();
        let res2 = warden2.call_with_interrupt(&mut env2, Some(&flag));
        assert!(res2.is_err());
        assert_eq!(
            env2.data.get("interrupter_called").map(|s| s.as_str()),
            Some("true")
        );
        assert_eq!(
            env2.data.get("interrupter_recovered").map(|s| s.as_str()),
            Some("true")
        );
    }

    #[test]
    fn test_machine_orchestrator() {
        /// No-op task helper for machine orchestration tests.
        fn dummy_task(_m: &str) -> Result<(), MigratoryError> {
            Ok(())
        }

        // Empty machines
        assert!(MachineOrchestrator::run(&[], true, dummy_task).is_ok());

        // Single machine
        let single = vec!["web".to_string()];
        assert!(MachineOrchestrator::run(&single, false, dummy_task).is_ok());
        let called = std::sync::Mutex::new(Vec::new());
        let res = MachineOrchestrator::run(&single, true, |m| {
            called
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(m.to_string());
            Ok(())
        });
        assert!(res.is_ok());
        assert_eq!(
            *called
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec!["web".to_string()]
        );

        // Sequential multi-machine
        let machines = vec!["web".to_string(), "db".to_string()];
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let res_seq = MachineOrchestrator::run(&machines, false, |_| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
        assert!(res_seq.is_ok());
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);

        // Parallel multi-machine
        let parallel_counter = std::sync::atomic::AtomicUsize::new(0);
        let res_par = MachineOrchestrator::run(&machines, true, |_| {
            parallel_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
        assert!(res_par.is_ok());
        assert_eq!(
            parallel_counter.load(std::sync::atomic::Ordering::SeqCst),
            2
        );

        // Failure handling - single failure
        let res_err = MachineOrchestrator::run(&machines, true, |m| {
            if m == "db" {
                Err(MigratoryError::Generic("db failed".to_string()))
            } else {
                Ok(())
            }
        });
        assert!(res_err.is_err());

        // Failure handling - multiple failures (hits if first_err.is_none() false branch)
        let res_both_err = MachineOrchestrator::run(&machines, true, |_| {
            Err(MigratoryError::Generic("both failed".to_string()))
        });
        assert!(res_both_err.is_err());

        // Thread panicking handling
        let res_panic = MachineOrchestrator::run(&machines, true, |_| {
            std::panic::resume_unwind(Box::new("test panic in orchestrator"));
        });
        assert!(res_panic.is_err());
    }

    #[test]
    fn test_standard_middleware_actions() -> Result<(), MigratoryError> {
        let mut env = Environment::new();
        env.insert_typed("custom_num", 42_u32);
        assert_eq!(env.get_typed::<u32>("custom_num"), Some(&42));
        assert_eq!(env.get_typed::<String>("custom_num"), None);

        // ActionHook::try_apply error branch
        let mut bad_hook = ActionHook::new();
        bad_hook.before(
            "nonexistent",
            Box::new(MockAction {
                name: "h".to_string(),
                should_fail: false,
                should_halt: false,
            }),
        );
        let mut empty_b = ActionBuilder::new();
        assert!(bad_hook.try_apply(&mut empty_b).is_err());

        let mut bad_hook_after = ActionHook::new();
        bad_hook_after.after(
            "nonexistent",
            Box::new(MockAction {
                name: "h2".to_string(),
                should_fail: false,
                should_halt: false,
            }),
        );
        assert!(bad_hook_after.try_apply(&mut empty_b).is_err());

        // CallAction branch taken
        let call_true = CallAction {
            condition: Box::new(|_| true),
            branch_action: Box::new(MockAction {
                name: "inner".to_string(),
                should_fail: false,
                should_halt: false,
            }),
        };
        assert_eq!(call_true.name(), "CallAction");
        assert_eq!(call_true.call(&mut env).ok(), Some(ActionResult::Continue));
        assert!(
            call_true
                .recover(&mut env, &MigratoryError::Generic("err".to_string()))
                .is_ok()
        );

        // CallAction branch skipped
        let call_false = CallAction {
            condition: Box::new(|_| false),
            branch_action: Box::new(MockAction {
                name: "inner".to_string(),
                should_fail: true,
                should_halt: false,
            }),
        };
        assert_eq!(call_false.call(&mut env).ok(), Some(ActionResult::Continue));
        assert!(
            call_false
                .recover(&mut env, &MigratoryError::Generic("err".to_string()))
                .is_ok()
        );

        // ConfigValidateAction success
        let val_action = ConfigValidateAction;
        assert_eq!(val_action.name(), "ConfigValidateAction");
        assert_eq!(val_action.call(&mut env).ok(), Some(ActionResult::Continue));

        // ConfigValidateAction failure
        env.data
            .insert("config_error".to_string(), "invalid box".to_string());
        assert!(val_action.call(&mut env).is_err());
        env.data.remove("config_error");

        let mut empty_env = Environment::new();
        assert_eq!(
            (BoxCheckOutdatedAction {
                box_name: "b".to_string()
            })
            .call(&mut empty_env)?,
            ActionResult::Continue
        );
        assert_eq!(
            (HandleBoxAction {
                box_name: "b".to_string()
            })
            .call(&mut empty_env)?,
            ActionResult::Continue
        );
        assert_eq!(
            HandleForwardedPortCollisionsAction.call(&mut empty_env)?,
            ActionResult::Continue
        );
        assert_eq!(
            (SetHostnameAction {
                hostname: "h".to_string()
            })
            .call(&mut empty_env)?,
            ActionResult::Continue
        );
        assert_eq!(
            (WaitForCommunicatorAction {
                timeout: std::time::Duration::from_secs(1)
            })
            .call(&mut empty_env)?,
            ActionResult::Continue
        );
        assert_eq!(
            GenerateKeyPairAction.call(&mut empty_env)?,
            ActionResult::Continue
        );
        assert_eq!(
            SyncFoldersAction.call(&mut empty_env)?,
            ActionResult::Continue
        );
        assert_eq!(
            ProvisionAction.call(&mut empty_env)?,
            ActionResult::Continue
        );

        let temp_home = tempfile::tempdir()?;
        let box_v_dir = temp_home
            .path()
            .join("boxes")
            .join("ubuntu-VAGRANTSLASH-focal64")
            .join("1.0.0");
        std::fs::create_dir_all(box_v_dir)?;
        env.insert_typed("vagrant_home", temp_home.path().to_path_buf());

        // BoxCheckOutdatedAction
        let check_box = BoxCheckOutdatedAction {
            box_name: "ubuntu/focal64".to_string(),
        };
        assert_eq!(check_box.name(), "BoxCheckOutdatedAction");
        assert_eq!(check_box.call(&mut env).ok(), Some(ActionResult::Continue));
        assert_eq!(
            env.data.get("box_checked").map(|s| s.as_str()),
            Some("ubuntu/focal64")
        );
        assert_eq!(
            env.data.get("box_outdated").map(|s| s.as_str()),
            Some("false")
        );

        // HandleBoxAction
        let handle_box = HandleBoxAction {
            box_name: "ubuntu/focal64".to_string(),
        };
        assert_eq!(handle_box.name(), "HandleBoxAction");
        assert_eq!(handle_box.call(&mut env).ok(), Some(ActionResult::Continue));
        assert_eq!(
            env.data.get("box_handled").map(|s| s.as_str()),
            Some("ubuntu/focal64")
        );
        assert_eq!(env.data.get("box_exists").map(|s| s.as_str()), Some("true"));

        // HandleForwardedPortCollisionsAction
        env.insert_typed(
            "networks",
            vec![
                crate::config::NetworkConfig::ForwardedPort {
                    guest: 80,
                    host: 28080,
                    auto_correct: true,
                    protocol: Some("tcp".to_string()),
                    host_ip: None,
                },
                crate::config::NetworkConfig::PrivateNetwork {
                    ip: Some("192.168.56.10".to_string()),
                    netmask: None,
                    dhcp: false,
                    virtualbox_intnet: None,
                },
            ],
        );
        let port_action = HandleForwardedPortCollisionsAction;
        assert_eq!(port_action.name(), "HandleForwardedPortCollisionsAction");
        assert_eq!(
            port_action.call(&mut env).ok(),
            Some(ActionResult::Continue)
        );
        assert_eq!(
            env.data.get("port_collisions_checked").map(|s| s.as_str()),
            Some("true")
        );
        assert_eq!(
            env.get_typed::<Vec<u16>>("resolved_ports"),
            Some(&vec![28080_u16])
        );

        /// Mock communicator for testing built-in middleware actions.
        struct ActionMockComm {
            /// Recorded commands executed on the communicator.
            commands: std::sync::Mutex<Vec<String>>,
        }
        impl crate::communicator::Communicator for ActionMockComm {
            fn wait_for_ready(&self, _timeout: std::time::Duration) -> Result<(), MigratoryError> {
                self.commands
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push("wait_for_ready".to_string());
                Ok(())
            }
            fn execute(&self, command: &str) -> Result<String, MigratoryError> {
                self.commands
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(command.to_string());
                Ok(String::new())
            }
        }

        let mock_comm = Arc::new(ActionMockComm {
            commands: std::sync::Mutex::new(Vec::new()),
        });
        let comm_trait: Arc<dyn crate::communicator::Communicator> =
            Arc::clone(&mock_comm) as Arc<dyn crate::communicator::Communicator>;
        env.insert_typed("communicator", comm_trait);

        // SetHostnameAction
        let host_action = SetHostnameAction {
            hostname: "web1.local".to_string(),
        };
        assert_eq!(host_action.name(), "SetHostnameAction");
        assert_eq!(
            host_action.call(&mut env).ok(),
            Some(ActionResult::Continue)
        );
        assert_eq!(
            env.data.get("hostname_set").map(|s| s.as_str()),
            Some("web1.local")
        );

        // WaitForCommunicatorAction
        let wait_action = WaitForCommunicatorAction {
            timeout: std::time::Duration::from_secs(45),
        };
        assert_eq!(wait_action.name(), "WaitForCommunicatorAction");
        assert_eq!(
            wait_action.call(&mut env).ok(),
            Some(ActionResult::Continue)
        );
        assert_eq!(
            env.data.get("communicator_ready").map(|s| s.as_str()),
            Some("45s")
        );

        // GenerateKeyPairAction
        let key_file_path = temp_home.path().join("private_key");
        env.insert_typed("private_key_path", key_file_path);
        let key_action = GenerateKeyPairAction;
        assert_eq!(key_action.name(), "GenerateKeyPairAction");
        assert_eq!(key_action.call(&mut env).ok(), Some(ActionResult::Continue));
        assert_eq!(
            env.data.get("key_pair_generated").map(|s| s.as_str()),
            Some("true")
        );

        // SyncFoldersAction
        let sf_host_dir = temp_home.path().join("shared_dir");
        std::fs::create_dir_all(&sf_host_dir)?;
        env.insert_typed(
            "synced_folders",
            vec![
                crate::config::SyncedFolderConfig {
                    host_path: sf_host_dir.clone(),
                    guest_path: std::path::PathBuf::from("/vagrant"),
                    disabled: false,
                    ..Default::default()
                },
                crate::config::SyncedFolderConfig {
                    host_path: sf_host_dir,
                    guest_path: std::path::PathBuf::from("/disabled"),
                    disabled: true,
                    ..Default::default()
                },
            ],
        );
        let sync_action = SyncFoldersAction;
        assert_eq!(sync_action.name(), "SyncFoldersAction");
        assert_eq!(
            sync_action.call(&mut env).ok(),
            Some(ActionResult::Continue)
        );
        assert_eq!(
            env.data.get("folders_synced").map(|s| s.as_str()),
            Some("true")
        );

        // ProvisionAction
        let mut shell_cfg = HashMap::new();
        shell_cfg.insert("inline".to_string(), "echo hello_prov".to_string());
        env.insert_typed(
            "provisioners",
            vec![
                crate::config::ProvisionerConfig {
                    name: "shell".to_string(),
                    config: shell_cfg,
                    id: None,
                    run: None,
                },
                crate::config::ProvisionerConfig {
                    name: "other".to_string(),
                    config: HashMap::new(),
                    id: None,
                    run: None,
                },
            ],
        );
        let prov_action = ProvisionAction;
        assert_eq!(prov_action.name(), "ProvisionAction");
        assert_eq!(
            prov_action.call(&mut env).ok(),
            Some(ActionResult::Continue)
        );
        assert_eq!(
            env.data.get("provisioned").map(|s| s.as_str()),
            Some("true")
        );

        let recorded = mock_comm
            .commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(recorded.iter().any(|c| c.contains("web1.local")));
        assert!(recorded.iter().any(|c| c == "wait_for_ready"));
        assert!(recorded.iter().any(|c| c.contains("vboxsf")));
        assert!(recorded.iter().any(|c| c.contains("hello_prov")));
        drop(recorded);

        // PruneNfsExportsAction
        let nfs_action = PruneNfsExportsAction;
        assert_eq!(nfs_action.name(), "PruneNfsExportsAction");
        assert_eq!(nfs_action.call(&mut env).ok(), Some(ActionResult::Continue));
        assert_eq!(
            env.data.get("nfs_exports_pruned").map(|s| s.as_str()),
            Some("true")
        );

        // CheckMachineStateAction
        let temp = tempfile::tempdir()?;
        let cwd = temp.path().to_path_buf();
        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("web")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir)?;
        std::fs::write(machine_dir.join("id"), "dummy_id")?;

        let check_state = CheckMachineStateAction {
            machine_name: "web".to_string(),
            provider_name: "virtualbox".to_string(),
            expected_states: vec!["running".to_string()],
            cwd: cwd.clone(),
        };
        assert_eq!(check_state.name(), "CheckMachineStateAction");

        env.insert_typed("machine_state", crate::provider::MachineState::Running);
        assert_eq!(
            check_state.call(&mut env).ok(),
            Some(ActionResult::Continue)
        );

        // State mismatch error
        let check_state_mismatch = CheckMachineStateAction {
            machine_name: "web".to_string(),
            provider_name: "virtualbox".to_string(),
            expected_states: vec!["poweroff".to_string()],
            cwd: cwd.clone(),
        };
        assert!(check_state_mismatch.call(&mut env).is_err());

        // Unknown provider error (when no override state is in env)
        let mut env_no_override = Environment::new();
        let check_state_invalid_prov = CheckMachineStateAction {
            machine_name: "web".to_string(),
            provider_name: "invalid_prov".to_string(),
            expected_states: vec!["running".to_string()],
            cwd,
        };
        assert!(check_state_invalid_prov.call(&mut env_no_override).is_err());
        assert!(check_state.call(&mut env_no_override).is_err());

        // Test default recover implementation on all standard actions
        let err = MigratoryError::Generic("err".to_string());
        assert!(val_action.recover(&mut env, &err).is_ok());
        assert!(check_box.recover(&mut env, &err).is_ok());
        assert!(handle_box.recover(&mut env, &err).is_ok());
        assert!(port_action.recover(&mut env, &err).is_ok());
        assert!(host_action.recover(&mut env, &err).is_ok());
        assert!(wait_action.recover(&mut env, &err).is_ok());
        assert!(key_action.recover(&mut env, &err).is_ok());
        assert!(sync_action.recover(&mut env, &err).is_ok());
        assert!(prov_action.recover(&mut env, &err).is_ok());
        assert!(nfs_action.recover(&mut env, &err).is_ok());
        assert!(check_state.recover(&mut env, &err).is_ok());

        Ok(())
    }
}
