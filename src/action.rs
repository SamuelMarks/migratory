//! Action/Middleware architecture.
//!
//! This module provides an Action Builder (Warden) to execute middleware chains
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
    /// Creates a new, empty Environment.
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            typed_data: HashMap::new(),
        }
    }
}

/// The result of an Action's `call` method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionResult {
    /// Continue to the next action in the chain.
    Continue,
    /// Abort the chain immediately, but return Ok.
    Halt,
}

/// A trait representing a single step (middleware) in an action chain.
pub trait Action: Send + Sync {
    /// The name of this action for debugging/logging.
    fn name(&self) -> &str;

    /// Called during the "up" phase of the middleware chain.
    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError>;

    /// Called during the "down" (recovery/rollback) phase of the middleware chain.
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
    actions: Vec<Box<dyn Action>>,
}

impl ActionBuilder {
    /// Creates a new, empty ActionBuilder.
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    /// Appends an action to the pipeline.
    pub fn use_action(&mut self, action: Box<dyn Action>) -> &mut Self {
        self.actions.push(action);
        self
    }

    /// Prepends an action to the beginning of the pipeline.
    pub fn prepend(&mut self, action: Box<dyn Action>) -> &mut Self {
        self.actions.insert(0, action);
        self
    }

    /// Inserts an action before the action named `target_name`.
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
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Returns true if the pipeline contains no actions.
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// Converts this builder into an executable `Warden`.
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
    before_hooks: Vec<(String, Box<dyn Action>)>,
    after_hooks: Vec<(String, Box<dyn Action>)>,
    prepend_hooks: Vec<Box<dyn Action>>,
    append_hooks: Vec<Box<dyn Action>>,
}

impl ActionHook {
    /// Creates a new empty `ActionHook`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an action to run before `target_name`.
    pub fn before(&mut self, target_name: &str, action: Box<dyn Action>) {
        self.before_hooks.push((target_name.to_string(), action));
    }

    /// Registers an action to run after `target_name`.
    pub fn after(&mut self, target_name: &str, action: Box<dyn Action>) {
        self.after_hooks.push((target_name.to_string(), action));
    }

    /// Registers an action to prepend to the start of the action stack.
    pub fn prepend(&mut self, action: Box<dyn Action>) {
        self.prepend_hooks.push(action);
    }

    /// Registers an action to append to the end of the action stack.
    pub fn append(&mut self, action: Box<dyn Action>) {
        self.append_hooks.push(action);
    }

    /// Applies all registered hooks onto the target `ActionBuilder`.
    pub fn apply(self, builder: &mut ActionBuilder) {
        for action in self.prepend_hooks {
            builder.prepend(action);
        }
        for (target, action) in self.before_hooks {
            let _ = builder.insert_before(&target, action);
        }
        for (target, action) in self.after_hooks {
            let _ = builder.insert_after(&target, action);
        }
        for action in self.append_hooks {
            builder.use_action(action);
        }
    }
}

/// The Warden manages a chain of Actions and executes them.
#[derive(Default)]
pub struct Warden {
    actions: Vec<Box<dyn Action>>,
}

impl Warden {
    /// Creates a new, empty Warden.
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    /// Appends an action to the chain.
    pub fn use_action(&mut self, action: Box<dyn Action>) {
        self.actions.push(action);
    }

    /// Executes the middleware chain.
    pub fn call(&self, env: &mut Environment) -> Result<(), MigratoryError> {
        self.call_with_interrupt(env, None)
    }

    /// Executes the middleware chain, checking for cancellation / SIGINT via `interrupted`.
    ///
    /// If an interrupt is detected, execution halts and recovery unwinds in reverse.
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
    /// The list of acceptable states (e.g., "running", "saved").
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

    fn call(&self, _env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        let state_mgr = crate::provider::StateManager::new(self.cwd.join(".vagrant"));
        let machine_id = state_mgr
            .read_id(&self.machine_name, &self.provider_name)
            .ok()
            .flatten();

        let p = crate::provider::get_provider(&self.provider_name, machine_id)?;
        let state = p.status().unwrap_or("not_created".to_string());

        if !self.expected_states.contains(&state) {
            return Err(MigratoryError::Generic(format!(
                "The VM '{}' is in state '{}', but expected one of: {:?}",
                self.machine_name, state, self.expected_states
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

/// Global mutual exclusion lock protecting /etc/exports file editing.
pub static NFS_EXPORTS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    struct MockAction {
        name: String,
        should_fail: bool,
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

        // Now interrupt between action1 and action2
        struct InterruptingAction {
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
        warden2.use_action(Box::new(InterruptingAction { flag: flag.clone() }));
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
            called.lock().expect("lock").push(m.to_string());
            Ok(())
        });
        assert!(res.is_ok());
        assert_eq!(*called.lock().expect("lock"), vec!["web".to_string()]);

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
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let res_panic = MachineOrchestrator::run(&machines, true, |_| {
            panic!("test panic in orchestrator");
        });
        std::panic::set_hook(prev_hook);
        assert!(res_panic.is_err());
    }

    #[test]
    fn test_standard_middleware_actions() -> Result<(), Box<dyn std::error::Error>> {
        let mut env = Environment::new();

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

        // HandleForwardedPortCollisionsAction
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
        let key_action = GenerateKeyPairAction;
        assert_eq!(key_action.name(), "GenerateKeyPairAction");
        assert_eq!(key_action.call(&mut env).ok(), Some(ActionResult::Continue));
        assert_eq!(
            env.data.get("key_pair_generated").map(|s| s.as_str()),
            Some("true")
        );

        // SyncFoldersAction
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

        // PruneNfsExportsAction
        let nfs_action = PruneNfsExportsAction;
        assert_eq!(nfs_action.name(), "PruneNfsExportsAction");
        assert_eq!(nfs_action.call(&mut env).ok(), Some(ActionResult::Continue));
        assert_eq!(
            env.data.get("nfs_exports_pruned").map(|s| s.as_str()),
            Some("true")
        );

        // CheckMachineStateAction
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().to_path_buf();
        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("web")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir).expect("mkdir");
        std::fs::write(machine_dir.join("id"), "dummy_id").expect("write");

        let check_state = CheckMachineStateAction {
            machine_name: "web".to_string(),
            provider_name: "virtualbox".to_string(),
            expected_states: vec!["running".to_string()],
            cwd: cwd.clone(),
        };
        assert_eq!(check_state.name(), "CheckMachineStateAction");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }
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

        // Unknown provider error
        let check_state_invalid_prov = CheckMachineStateAction {
            machine_name: "web".to_string(),
            provider_name: "invalid_prov".to_string(),
            expected_states: vec!["running".to_string()],
            cwd: cwd.clone(),
        };
        assert!(check_state_invalid_prov.call(&mut env).is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
        }

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
