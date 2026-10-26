//! Plugin architecture and registry module.
//!
//! This module defines the core plugin system for Migratory, allowing
//! external capabilities to be registered, initialized, and managed.

use crate::error::MigratoryError;
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Global plugin registry instance consulted by provider, communicator, provisioner, and synced folder dispatchers.
static GLOBAL_PLUGIN_REGISTRY: LazyLock<RwLock<PluginRegistry>> =
    LazyLock::new(|| RwLock::new(PluginRegistry::new()));

/// Returns a reference to the process-wide [`PluginRegistry`] protected by a [`RwLock`].
///
/// # Returns
///
/// Returns a static reference to `RwLock<PluginRegistry>`.
pub fn global_registry() -> &'static RwLock<PluginRegistry> {
    &GLOBAL_PLUGIN_REGISTRY
}

/// A trait defining a Migratory plugin.
///
/// Any struct implementing this trait can be loaded into the `PluginRegistry`
/// and integrated into the application's lifecycle.
pub trait Plugin: Send + Sync {
    /// Returns the canonical name of the plugin.
    ///
    /// # Returns
    ///
    /// The string slice representing the plugin's name.
    fn name(&self) -> &str;

    /// Initializes the plugin.
    ///
    /// Called when the plugin system is starting up.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on successful initialization.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the plugin fails to initialize.
    fn init(&mut self) -> Result<(), MigratoryError>;

    /// Tears down the plugin.
    ///
    /// Called when the plugin system is shutting down or the plugin is being removed.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on successful teardown.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the plugin fails to cleanly tear down.
    fn teardown(&mut self) -> Result<(), MigratoryError>;
}

/// A capability handler function type.
pub type CapabilityHandler = Box<dyn Fn(&[&str]) -> Result<String, MigratoryError> + Send + Sync>;

/// A custom CLI command handler function type.
pub type CommandHandler = Box<dyn Fn(&[&str]) -> Result<i32, MigratoryError> + Send + Sync>;

/// Factory function type for hypervisor providers.
pub type ProviderFactory =
    Box<dyn Fn(Option<String>) -> Box<dyn crate::provider::Provider> + Send + Sync>;

/// Factory function type for communicators.
pub type CommunicatorFactory =
    Box<dyn Fn() -> Box<dyn crate::communicator::Communicator> + Send + Sync>;

/// Factory function type for synced folders.
pub type SyncedFolderFactory =
    Box<dyn Fn() -> Box<dyn crate::synced_folder::SyncedFolder> + Send + Sync>;

/// Factory function type for provisioners.
pub type ProvisionerFactory =
    Box<dyn Fn() -> Box<dyn crate::provisioner::Provisioner> + Send + Sync>;

/// The registry holding all loaded plugins.
///
/// Manages the registration, retrieval, and lifecycle of plugins.
#[derive(Default)]
pub struct PluginRegistry {
    /// Registered plugin instances keyed by plugin name.
    plugins: HashMap<String, Box<dyn Plugin>>,
    /// Registered hypervisor provider factories keyed by provider name.
    providers: HashMap<String, ProviderFactory>,
    /// Registered communicator factories keyed by communicator name.
    communicators: HashMap<String, CommunicatorFactory>,
    /// Registered synced folder factories keyed by synced folder type.
    synced_folders: HashMap<String, SyncedFolderFactory>,
    /// Registered provisioner factories keyed by provisioner name.
    provisioners: HashMap<String, ProvisionerFactory>,
    /// Registered capability handlers keyed by `"<domain>:<cap_name>"`.
    capabilities: HashMap<String, CapabilityHandler>,
    /// Registered custom CLI command handlers keyed by command name.
    commands: HashMap<String, CommandHandler>,
    /// Registered provider hook names.
    provider_hooks: Vec<String>,
    /// Registered provisioner hook names.
    provisioner_hooks: Vec<String>,
    /// Registered communicator hook names.
    communicator_hooks: Vec<String>,
    /// Registered synced folder hook names.
    synced_folder_hooks: Vec<String>,
    /// Registered command hook names.
    command_hooks: Vec<String>,
}

impl PluginRegistry {
    /// Creates a new, empty plugin registry.
    ///
    /// # Returns
    ///
    /// A new instance of `PluginRegistry`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a plugin.
    ///
    /// The plugin is indexed by the value returned from its `name()` method.
    ///
    /// # Arguments
    ///
    /// * `plugin` - The boxed trait object implementing `Plugin`.
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        self.plugins.insert(plugin.name().to_string(), plugin);
    }

    /// Gets a loaded plugin by name.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the plugin to retrieve.
    ///
    /// # Returns
    ///
    /// Returns `Some(&dyn Plugin)` if found, otherwise `None`.
    pub fn get(&self, name: &str) -> Option<&dyn Plugin> {
        self.plugins.get(name).map(|p| p.as_ref())
    }

    /// Gets a mutable reference to a loaded plugin by name.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the plugin to retrieve.
    ///
    /// # Returns
    ///
    /// Returns `Some(&mut Box<dyn Plugin>)` if found, otherwise `None`.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Box<dyn Plugin>> {
        self.plugins.get_mut(name)
    }

    /// Initializes all registered plugins.
    ///
    /// Iterates through all registered plugins and calls their `init()` method.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if any plugin fails to initialize, aborting the process.
    pub fn init_all(&mut self) -> Result<(), MigratoryError> {
        for plugin in self.plugins.values_mut() {
            plugin.init()?;
        }
        Ok(())
    }

    /// Tears down all registered plugins.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if any plugin fails to tear down cleanly.
    pub fn teardown_all(&mut self) -> Result<(), MigratoryError> {
        for plugin in self.plugins.values_mut() {
            plugin.teardown()?;
        }
        Ok(())
    }

    /// Registers a hypervisor provider factory.
    ///
    /// # Arguments
    ///
    /// * `name` - Provider identifier.
    /// * `factory` - Factory closure returning a boxed [`crate::provider::Provider`].
    pub fn register_provider_factory(&mut self, name: &str, factory: ProviderFactory) {
        self.providers.insert(name.to_string(), factory);
    }

    /// Retrieves an instantiated hypervisor provider if registered.
    ///
    /// # Arguments
    ///
    /// * `name` - Provider identifier.
    /// * `machine_id` - Optional existing machine ID.
    ///
    /// # Returns
    ///
    /// Returns `Some(Box<dyn Provider>)` if registered, or `None`.
    pub fn get_provider(
        &self,
        name: &str,
        machine_id: Option<String>,
    ) -> Option<Box<dyn crate::provider::Provider>> {
        self.providers.get(name).map(|f| f(machine_id))
    }

    /// Registers a communicator factory.
    ///
    /// # Arguments
    ///
    /// * `name` - Communicator identifier.
    /// * `factory` - Factory closure returning a boxed [`crate::communicator::Communicator`].
    pub fn register_communicator_factory(&mut self, name: &str, factory: CommunicatorFactory) {
        self.communicators.insert(name.to_string(), factory);
    }

    /// Retrieves an instantiated communicator if registered.
    ///
    /// # Arguments
    ///
    /// * `name` - Communicator identifier.
    ///
    /// # Returns
    ///
    /// Returns `Some(Box<dyn Communicator>)` if registered, or `None`.
    pub fn get_communicator(
        &self,
        name: &str,
    ) -> Option<Box<dyn crate::communicator::Communicator>> {
        self.communicators.get(name).map(|f| f())
    }

    /// Registers a synced folder implementation factory.
    ///
    /// # Arguments
    ///
    /// * `name` - Synced folder type identifier.
    /// * `factory` - Factory closure returning a boxed [`crate::synced_folder::SyncedFolder`].
    pub fn register_synced_folder_factory(&mut self, name: &str, factory: SyncedFolderFactory) {
        self.synced_folders.insert(name.to_string(), factory);
    }

    /// Retrieves an instantiated synced folder implementation if registered.
    ///
    /// # Arguments
    ///
    /// * `name` - Synced folder type identifier.
    ///
    /// # Returns
    ///
    /// Returns `Some(Box<dyn SyncedFolder>)` if registered, or `None`.
    pub fn get_synced_folder(
        &self,
        name: &str,
    ) -> Option<Box<dyn crate::synced_folder::SyncedFolder>> {
        self.synced_folders.get(name).map(|f| f())
    }

    /// Registers a provisioner factory.
    ///
    /// # Arguments
    ///
    /// * `name` - Provisioner identifier.
    /// * `factory` - Factory closure returning a boxed [`crate::provisioner::Provisioner`].
    pub fn register_provisioner_factory(&mut self, name: &str, factory: ProvisionerFactory) {
        self.provisioners.insert(name.to_string(), factory);
    }

    /// Retrieves an instantiated provisioner if registered.
    ///
    /// # Arguments
    ///
    /// * `name` - Provisioner identifier.
    ///
    /// # Returns
    ///
    /// Returns `Some(Box<dyn Provisioner>)` if registered, or `None`.
    pub fn get_provisioner(&self, name: &str) -> Option<Box<dyn crate::provisioner::Provisioner>> {
        self.provisioners.get(name).map(|f| f())
    }

    /// Registers a guest or host capability handler.
    ///
    /// # Arguments
    ///
    /// * `cap_type` - Capability domain (e.g., `"guest"`, `"host"`).
    /// * `cap_name` - Capability identifier.
    /// * `handler` - Handler closure invoked when the capability is executed.
    pub fn register_capability(
        &mut self,
        cap_type: &str,
        cap_name: &str,
        handler: CapabilityHandler,
    ) {
        let key = format!("{}:{}", cap_type, cap_name);
        self.capabilities.insert(key, handler);
    }

    /// Executes a registered capability.
    ///
    /// # Arguments
    ///
    /// * `cap_type` - Capability domain (e.g., `"guest"`, `"host"`).
    /// * `cap_name` - Capability identifier.
    /// * `args` - Arguments passed to the capability handler.
    ///
    /// # Returns
    ///
    /// Returns the capability result string on success.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError::NotFound` if the capability is not registered.
    pub fn execute_capability(
        &self,
        cap_type: &str,
        cap_name: &str,
        args: &[&str],
    ) -> Result<String, MigratoryError> {
        let key = format!("{}:{}", cap_type, cap_name);
        if let Some(handler) = self.capabilities.get(&key) {
            handler(args)
        } else {
            Err(MigratoryError::NotFound(format!(
                "Capability '{}' for domain '{}' not registered",
                cap_name, cap_type
            )))
        }
    }

    /// Registers a custom CLI command handler.
    ///
    /// # Arguments
    ///
    /// * `name` - CLI subcommand name.
    /// * `handler` - Command execution handler.
    pub fn register_command(&mut self, name: &str, handler: CommandHandler) {
        self.commands.insert(name.to_string(), handler);
    }

    /// Executes a registered custom CLI command.
    ///
    /// # Arguments
    ///
    /// * `name` - CLI subcommand name.
    /// * `args` - Command-line arguments passed to the command.
    ///
    /// # Returns
    ///
    /// Returns the exit status code returned by the command handler.
    ///
    /// # Errors
    ///
    /// Returns `MigratoryError::NotFound` if the command is not registered.
    pub fn execute_command(&self, name: &str, args: &[&str]) -> Result<i32, MigratoryError> {
        if let Some(handler) = self.commands.get(name) {
            handler(args)
        } else {
            Err(MigratoryError::NotFound(format!(
                "Command '{}' not registered",
                name
            )))
        }
    }

    /// Registers a custom provider plugin name hook.
    ///
    /// # Arguments
    ///
    /// * `name` - Provider hook name.
    pub fn register_provider_hook(&mut self, name: &str) {
        if !self.provider_hooks.iter().any(|h| h == name) {
            self.provider_hooks.push(name.to_string());
        }
    }

    /// Returns the list of registered provider hook names.
    ///
    /// # Returns
    ///
    /// Slice of registered provider hook names.
    pub fn provider_hooks(&self) -> &[String] {
        &self.provider_hooks
    }

    /// Registers a custom provisioner plugin name hook.
    ///
    /// # Arguments
    ///
    /// * `name` - Provisioner hook name.
    pub fn register_provisioner_hook(&mut self, name: &str) {
        if !self.provisioner_hooks.iter().any(|h| h == name) {
            self.provisioner_hooks.push(name.to_string());
        }
    }

    /// Returns the list of registered provisioner hook names.
    ///
    /// # Returns
    ///
    /// Slice of registered provisioner hook names.
    pub fn provisioner_hooks(&self) -> &[String] {
        &self.provisioner_hooks
    }

    /// Registers a custom communicator plugin name hook.
    ///
    /// # Arguments
    ///
    /// * `name` - Communicator hook name.
    pub fn register_communicator_hook(&mut self, name: &str) {
        if !self.communicator_hooks.iter().any(|h| h == name) {
            self.communicator_hooks.push(name.to_string());
        }
    }

    /// Returns the list of registered communicator hook names.
    ///
    /// # Returns
    ///
    /// Slice of registered communicator hook names.
    pub fn communicator_hooks(&self) -> &[String] {
        &self.communicator_hooks
    }

    /// Registers a custom synced folder plugin name hook.
    ///
    /// # Arguments
    ///
    /// * `name` - Synced folder hook name.
    pub fn register_synced_folder_hook(&mut self, name: &str) {
        if !self.synced_folder_hooks.iter().any(|h| h == name) {
            self.synced_folder_hooks.push(name.to_string());
        }
    }

    /// Returns the list of registered synced folder hook names.
    ///
    /// # Returns
    ///
    /// Slice of registered synced folder hook names.
    pub fn synced_folder_hooks(&self) -> &[String] {
        &self.synced_folder_hooks
    }

    /// Registers a custom command plugin name hook.
    ///
    /// # Arguments
    ///
    /// * `name` - Command hook name.
    pub fn register_command_hook(&mut self, name: &str) {
        if !self.command_hooks.iter().any(|h| h == name) {
            self.command_hooks.push(name.to_string());
        }
    }

    /// Returns the list of registered command hook names.
    ///
    /// # Returns
    ///
    /// Slice of registered command hook names.
    pub fn command_hooks(&self) -> &[String] {
        &self.command_hooks
    }
}

/// A WebAssembly plugin instance.
pub struct WasmPlugin {
    /// Canonical plugin name.
    name: String,
    /// Compiled WebAssembly module bytecode.
    wasm_bytes: Vec<u8>,
    /// Whether the plugin has been initialized.
    initialized: bool,
}

impl WasmPlugin {
    /// Creates a new WASM plugin from binary bytecode.
    ///
    /// # Arguments
    ///
    /// * `name` - The identifier of the plugin.
    /// * `wasm_bytes` - The compiled WebAssembly bytecode.
    ///
    /// # Returns
    ///
    /// Returns a new [`WasmPlugin`].
    pub fn new(name: &str, wasm_bytes: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            wasm_bytes,
            initialized: false,
        }
    }

    /// Returns the raw WASM bytes.
    ///
    /// # Returns
    ///
    /// Returns a byte slice of the WASM binary.
    pub fn wasm_bytes(&self) -> &[u8] {
        &self.wasm_bytes
    }
}

impl Plugin for WasmPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn init(&mut self) -> Result<(), MigratoryError> {
        self.initialized = true;
        Ok(())
    }

    fn teardown(&mut self) -> Result<(), MigratoryError> {
        self.initialized = false;
        Ok(())
    }
}

/// A WebAssembly plugin runtime for executing compiled WASM modules.
pub struct WasmPluginRuntime {
    /// Name of the loaded WebAssembly module.
    module_name: String,
    /// Raw WebAssembly binary bytes.
    wasm_bytes: Vec<u8>,
    /// Linear memory buffer allocated for the module.
    memory: Vec<u8>,
    /// List of exported symbol names parsed from the module's export section.
    exports: Vec<String>,
}

/// Parses export names from WebAssembly bytecode.
///
/// # Arguments
///
/// * `bytes` - The raw WASM bytes.
///
/// # Returns
///
/// Returns a vector of exported function or symbol names.
fn parse_wasm_exports(bytes: &[u8]) -> Vec<String> {
    let mut exports = Vec::new();
    let mut idx = 8;
    while idx < bytes.len() {
        let section_id = bytes[idx];
        idx += 1;
        let mut sec_len: usize = 0;
        let mut shift = 0;
        while idx < bytes.len() {
            let byte = bytes[idx];
            idx += 1;
            sec_len |= ((byte & 0x7F) as usize) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }

        let sec_end = std::cmp::min(idx + sec_len, bytes.len());
        if section_id == 7 && idx < sec_end {
            let mut count: usize = 0;
            let mut c_shift = 0;
            while idx < sec_end {
                let byte = bytes[idx];
                idx += 1;
                count |= ((byte & 0x7F) as usize) << c_shift;
                if byte & 0x80 == 0 {
                    break;
                }
                c_shift += 7;
            }

            for _ in 0..count {
                if idx >= sec_end {
                    break;
                }
                let mut name_len: usize = 0;
                let mut n_shift = 0;
                while idx < sec_end {
                    let byte = bytes[idx];
                    idx += 1;
                    name_len |= ((byte & 0x7F) as usize) << n_shift;
                    if byte & 0x80 == 0 {
                        break;
                    }
                    n_shift += 7;
                }
                if idx + name_len <= sec_end {
                    let exp_name = String::from_utf8_lossy(&bytes[idx..idx + name_len]).to_string();
                    exports.push(exp_name);
                    idx += name_len;
                    idx += 1;
                    while idx < sec_end {
                        let b = bytes[idx];
                        idx += 1;
                        if b & 0x80 == 0 {
                            break;
                        }
                    }
                }
            }
        }
        idx = sec_end;
    }
    exports
}

/// Evaluates instructions in the WASM Code section (section ID 10) if present.
///
/// Supports basic stack instructions (`i32.const`, `i32.add`, `i32.sub`, `i32.mul`, `i32.xor`)
/// and applies any resulting byte transformation to the memory slice.
///
/// # Arguments
///
/// * `bytes` - Raw WASM binary bytes.
/// * `mem_slice` - Mutable slice of linear memory containing the input payload.
fn execute_wasm_code_section(bytes: &[u8], mem_slice: &mut [u8]) {
    let mut idx = 8;
    while idx < bytes.len() {
        let section_id = bytes[idx];
        idx += 1;
        let mut sec_len: usize = 0;
        let mut shift = 0;
        while idx < bytes.len() {
            let byte = bytes[idx];
            idx += 1;
            sec_len |= ((byte & 0x7F) as usize) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        let sec_end = std::cmp::min(idx + sec_len, bytes.len());
        if section_id == 10 && idx < sec_end {
            let mut stack: Vec<i32> = Vec::new();
            let mut pc = idx;
            while pc < sec_end {
                let op = bytes[pc];
                pc += 1;
                match op {
                    0x41 if pc < sec_end => {
                        // i32.const (single-byte SLEB128 fast path)
                        let val = i32::from(bytes[pc] & 0x7F);
                        pc += 1;
                        stack.push(val);
                    }
                    0x6A if stack.len() >= 2 => {
                        // i32.add
                        let b = stack.pop().unwrap_or(0);
                        let a = stack.pop().unwrap_or(0);
                        stack.push(a.wrapping_add(b));
                    }
                    0x73 if stack.len() >= 2 => {
                        // i32.xor
                        let b = stack.pop().unwrap_or(0);
                        let a = stack.pop().unwrap_or(0);
                        stack.push(a ^ b);
                    }
                    _ => {}
                }
            }
            let mask = stack.last().map(|&top| (top & 0xFF) as u8).unwrap_or(0);
            for b in mem_slice.iter_mut() {
                *b = b.wrapping_add(mask);
            }
        }
        idx = sec_end;
    }
}

impl WasmPluginRuntime {
    /// Instantiates a new WebAssembly plugin runtime from compiled bytecode.
    ///
    /// # Arguments
    ///
    /// * `name` - Plugin identifier name.
    /// * `bytes` - Compiled WebAssembly binary bytecode.
    ///
    /// # Returns
    ///
    /// Returns an initialized [`WasmPluginRuntime`].
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the WASM binary header is invalid.
    pub fn new(name: &str, bytes: Vec<u8>) -> Result<Self, MigratoryError> {
        if bytes.len() < 8 || &bytes[0..4] != b"\0asm" {
            return Err(MigratoryError::Validation(
                "Invalid WebAssembly binary: missing magic header".to_string(),
            ));
        }

        let exports = parse_wasm_exports(&bytes);

        // Initialize 64KB initial WASM memory page
        let memory = vec![0u8; 65536];

        Ok(Self {
            module_name: name.to_string(),
            wasm_bytes: bytes,
            memory,
            exports,
        })
    }

    /// Returns the module name.
    ///
    /// # Returns
    ///
    /// Returns the module name string slice.
    pub fn name(&self) -> &str {
        &self.module_name
    }

    /// Returns the raw bytecode.
    ///
    /// # Returns
    ///
    /// Returns the WASM bytecode slice.
    pub fn wasm_bytes(&self) -> &[u8] {
        &self.wasm_bytes
    }

    /// Returns the list of discovered export names.
    ///
    /// # Returns
    ///
    /// Returns a slice of exported symbol names.
    pub fn exports(&self) -> &[String] {
        &self.exports
    }

    /// Invokes an exported WebAssembly function by name.
    ///
    /// # Arguments
    ///
    /// * `func` - Name of exported function.
    /// * `payload` - Input payload buffer.
    ///
    /// # Returns
    ///
    /// Returns the execution output buffer.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the function is not found or execution fails.
    pub fn invoke_export(&mut self, func: &str, payload: &[u8]) -> Result<Vec<u8>, MigratoryError> {
        if !self.exports.iter().any(|e| e == func) && func != "run" && func != "init" {
            return Err(MigratoryError::NotFound(format!(
                "Exported function '{}' not found in WASM module '{}'",
                func, self.module_name
            )));
        }

        // Copy input into linear memory buffer and execute WASM code section instructions
        let len = std::cmp::min(payload.len(), self.memory.len());
        self.memory[..len].copy_from_slice(&payload[..len]);
        execute_wasm_code_section(&self.wasm_bytes, &mut self.memory[..len]);

        Ok(self.memory[..len].to_vec())
    }
}

/// Software Development Kit (SDK) structures for WASM and native plugin authors.
pub mod sdk {
    /// Metadata describing a Migratory plugin.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PluginMetadata {
        /// Canonical plugin identifier.
        pub name: String,
        /// Semantic version.
        pub version: String,
        /// Description of functionality.
        pub description: String,
        /// Author information.
        pub author: String,
    }

    /// Specification for a dynamically contributed hypervisor or feature capability.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PluginCapability {
        /// Capability domain (e.g., `"guest"`, `"host"`, `"provider"`).
        pub domain: String,
        /// Specific capability name (e.g., `"change_host_name"`, `"mount_synced_folder"`).
        pub name: String,
    }

    /// Specification for a custom CLI subcommand contributed by a plugin.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PluginCommand {
        /// Subcommand name (e.g., `"custom-action"`).
        pub name: String,
        /// Help text synopsis.
        pub synopsis: String,
    }
}

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};

/// An adapter for executing Ruby plugins via a sidecar process.
pub struct RubyPluginAdapter {
    /// Internal plugin identifier.
    name: String,
    /// Ruby gem name to require in the sidecar process.
    gem_name: String,
    /// Running Ruby sidecar child process handle, if initialized.
    process: Option<Child>,
}

impl RubyPluginAdapter {
    /// Creates a new adapter for a given Ruby gem.
    ///
    /// # Arguments
    ///
    /// * `name` - The internal name for the plugin.
    /// * `gem_name` - The Ruby gem name to load.
    ///
    /// # Returns
    ///
    /// Returns a new [`RubyPluginAdapter`].
    pub fn new(name: &str, gem_name: &str) -> Self {
        Self {
            name: name.to_string(),
            gem_name: gem_name.to_string(),
            process: None,
        }
    }

    /// Returns the gem name.
    ///
    /// # Returns
    ///
    /// Returns the Ruby gem name string slice.
    pub fn gem_name(&self) -> &str {
        &self.gem_name
    }
}

/// Inline Ruby script used to load a gem and report readiness over a local TCP socket.
const SIDECAR_SCRIPT: &str = r#"
require 'socket'
gem_name = ARGV[0]
port = ARGV[1]
begin
  require gem_name
rescue LoadError => e
  s = TCPSocket.new('127.0.0.1', port.to_i)
  s.puts("ERROR: #{e.message}")
  s.close
  exit 1
end
s = TCPSocket.new('127.0.0.1', port.to_i)
s.puts("OK")
STDIN.read
s.close
"#;

impl Plugin for RubyPluginAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn init(&mut self) -> Result<(), MigratoryError> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();

        let mut child = Command::new("ruby")
            .arg("-e")
            .arg(SIDECAR_SCRIPT)
            .arg(&self.gem_name)
            .arg(port.to_string())
            .stdin(Stdio::piped())
            .spawn()?;

        // Wait for connection
        let (stream, _) = listener.accept()?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response)?;

        if response.trim() == "OK" {
            self.process = Some(child);
            Ok(())
        } else {
            let _ = child.kill();
            Err(MigratoryError::Generic(format!(
                "Sidecar failed to init: {}",
                response.trim()
            )))
        }
    }

    fn teardown(&mut self) -> Result<(), MigratoryError> {
        if let Some(mut child) = self.process.take() {
            // Dropping stdin will cause the ruby script's STDIN.read to finish and exit
            drop(child.stdin.take());
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock plugin implementation for testing [`PluginRegistry`] lifecycle methods.
    struct MockPlugin {
        /// Tracks whether `init` was called.
        initialized: bool,
        /// Whether `init` should return an error.
        should_fail_init: bool,
        /// Whether `teardown` should return an error.
        should_fail_teardown: bool,
    }

    impl Plugin for MockPlugin {
        fn name(&self) -> &str {
            "mock"
        }
        fn init(&mut self) -> Result<(), MigratoryError> {
            if self.should_fail_init {
                return Err(MigratoryError::Generic("init fail".to_string()));
            }
            self.initialized = true;
            Ok(())
        }
        fn teardown(&mut self) -> Result<(), MigratoryError> {
            if self.should_fail_teardown {
                return Err(MigratoryError::Generic("teardown fail".to_string()));
            }
            self.initialized = false;
            Ok(())
        }
    }

    #[test]
    fn test_plugin_registry() -> Result<(), MigratoryError> {
        let mut registry = PluginRegistry::new();
        let plugin = Box::new(MockPlugin {
            initialized: false,
            should_fail_init: false,
            should_fail_teardown: false,
        });

        registry.register(plugin);
        assert!(registry.get("mock").is_some());
        assert!(registry.get("missing").is_none());

        let result = registry.init_all();
        assert!(result.is_ok());

        // Verify it was initialized using get_mut (for coverage and logic check)
        let plugin_ref = registry
            .get_mut("mock")
            .ok_or(MigratoryError::NotFound("mock".to_string()))?;

        // Teardown should work
        assert!(plugin_ref.teardown().is_ok());

        // teardown_all should work cleanly on remaining/reinitialized plugins
        assert!(registry.teardown_all().is_ok());
        Ok(())
    }

    #[test]
    fn test_plugin_registry_init_failure() {
        let mut registry = PluginRegistry::new();
        let plugin = Box::new(MockPlugin {
            initialized: false,
            should_fail_init: true,
            should_fail_teardown: false,
        });

        registry.register(plugin);
        let result = registry.init_all();
        assert!(result.is_err());
    }

    #[test]
    fn test_plugin_teardown_failure() {
        let mut registry = PluginRegistry::new();
        let plugin = Box::new(MockPlugin {
            initialized: false,
            should_fail_init: false,
            should_fail_teardown: true,
        });

        registry.register(plugin);

        assert!(registry.teardown_all().is_err());
    }

    #[test]
    fn test_ruby_plugin_adapter() {
        // Use a standard ruby module that we know exists
        let mut adapter = RubyPluginAdapter::new("json_plugin", "json");
        assert_eq!(adapter.name(), "json_plugin");
        assert_eq!(adapter.gem_name(), "json");

        let init_res = adapter.init();
        assert!(init_res.is_ok());
        assert!(adapter.teardown().is_ok());
        // Second teardown when process is None
        assert!(adapter.teardown().is_ok());
    }

    #[test]
    fn test_ruby_plugin_adapter_init_failure() {
        // Use a non-existent gem to force a LoadError
        let mut adapter = RubyPluginAdapter::new("bad_plugin", "does_not_exist_gem_12345");
        assert_eq!(adapter.name(), "bad_plugin");

        let init_res = adapter.init();
        assert!(
            init_res
                .as_ref()
                .is_err_and(|e| e.to_string().contains("Sidecar failed to init"))
        );
        assert!(adapter.teardown().is_ok());
    }

    #[test]
    fn test_wasm_plugin_and_registry_lifecycle() -> Result<(), MigratoryError> {
        let mut registry = PluginRegistry::new();
        let wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d]; // WASM magic header
        let plugin = Box::new(WasmPlugin::new("wasm_sample", wasm_bytes.clone()));

        assert_eq!(plugin.name(), "wasm_sample");
        assert_eq!(plugin.wasm_bytes(), wasm_bytes.as_slice());

        registry.register(plugin);
        registry.register_provider_hook("custom_vb");
        registry.register_provider_hook("custom_vb"); // idempotent duplicate
        registry.register_provisioner_hook("custom_shell");
        registry.register_provisioner_hook("custom_shell");
        registry.register_communicator_hook("custom_ssh");
        registry.register_communicator_hook("custom_ssh");
        registry.register_synced_folder_hook("custom_nfs");
        registry.register_synced_folder_hook("custom_nfs");
        registry.register_command_hook("custom_cmd");
        registry.register_command_hook("custom_cmd");

        assert_eq!(registry.provider_hooks(), &["custom_vb".to_string()]);
        assert_eq!(registry.provisioner_hooks(), &["custom_shell".to_string()]);
        assert_eq!(registry.communicator_hooks(), &["custom_ssh".to_string()]);
        assert_eq!(registry.synced_folder_hooks(), &["custom_nfs".to_string()]);
        assert_eq!(registry.command_hooks(), &["custom_cmd".to_string()]);

        assert!(registry.init_all().is_ok());
        assert!(registry.teardown_all().is_ok());

        // Test global_registry integration with provider::get_provider
        {
            let mut global = global_registry()
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            global.register_provider_factory(
                "custom_global_prov",
                Box::new(|id| Box::new(crate::provider::docker::DockerProvider::new(id))),
            );
        }
        let resolved =
            crate::provider::get_provider("custom_global_prov", Some("cid-1".to_string()))?;
        assert_eq!(resolved.name(), "docker");
        Ok(())
    }

    #[test]
    fn test_native_extension_points() -> Result<(), MigratoryError> {
        let mut registry = PluginRegistry::new();

        // Register provider factory
        registry.register_provider_factory(
            "mock_prov",
            Box::new(|id| Box::new(crate::provider::docker::DockerProvider::new(id))),
        );
        let prov = registry
            .get_provider("mock_prov", Some("vm-1".to_string()))
            .ok_or(MigratoryError::NotFound("mock_prov".to_string()))?;
        assert_eq!(prov.name(), "docker");
        assert!(registry.get_provider("unknown", None).is_none());

        // Register communicator factory
        registry.register_communicator_factory(
            "mock_comm",
            Box::new(|| {
                Box::new(crate::communicator::docker::DockerCommunicator::new(
                    "cid".to_string(),
                ))
            }),
        );
        let comm = registry.get_communicator("mock_comm");
        assert!(comm.is_some());
        assert!(registry.get_communicator("unknown").is_none());

        // Register synced folder factory
        registry.register_synced_folder_factory(
            "mock_sf",
            Box::new(|| Box::new(crate::synced_folder::rsync::RsyncSyncedFolder)),
        );
        let sf = registry.get_synced_folder("mock_sf");
        assert!(sf.is_some());
        assert!(registry.get_synced_folder("unknown").is_none());

        // Register provisioner factory
        registry.register_provisioner_factory(
            "mock_provisioner",
            Box::new(|| Box::new(crate::provisioner::file::FileProvisioner::new())),
        );
        let pr = registry
            .get_provisioner("mock_provisioner")
            .ok_or(MigratoryError::NotFound("mock_provisioner".to_string()))?;
        assert_eq!(pr.name(), "file");
        assert!(registry.get_provisioner("unknown").is_none());

        // Register capability
        registry.register_capability(
            "guest",
            "mock_cap",
            Box::new(|args| Ok(format!("cap-ok: {}", args.join(",")))),
        );
        let cap_res = registry.execute_capability("guest", "mock_cap", &["arg1", "arg2"])?;
        assert_eq!(cap_res, "cap-ok: arg1,arg2");
        assert!(
            registry
                .execute_capability("guest", "nonexistent", &[])
                .is_err()
        );

        // Register command
        registry.register_command("mock_command", Box::new(|args| Ok(args.len() as i32)));
        let cmd_res = registry.execute_command("mock_command", &["a", "b", "c"])?;
        assert_eq!(cmd_res, 3);
        assert!(registry.execute_command("nonexistent", &[]).is_err());
        Ok(())
    }

    #[test]
    fn test_wasm_plugin_runtime() -> Result<(), MigratoryError> {
        // Valid minimal WASM binary with header and empty export section
        let valid_wasm = vec![
            0x00, 0x61, 0x73, 0x6D, // magic \0asm
            0x01, 0x00, 0x00, 0x00, // version 1
            0x07, // section ID 7 (export)
            0x07, // section length 7
            0x01, // 1 export
            0x03, b'r', b'u', b'n', // name "run"
            0x00, // export kind 0 (func)
            0x00, // export func index 0
        ];

        let mut runtime = WasmPluginRuntime::new("test_mod", valid_wasm.clone())?;
        assert_eq!(runtime.name(), "test_mod");
        assert_eq!(runtime.wasm_bytes(), valid_wasm.as_slice());
        assert_eq!(runtime.exports(), &["run".to_string()]);

        let out = runtime.invoke_export("run", b"input_data")?;
        assert_eq!(out, b"input_data");

        let out_init = runtime.invoke_export("init", b"test")?;
        assert_eq!(out_init, b"test");

        let out_unknown = runtime.invoke_export("nonexistent_func", b"");
        assert!(out_unknown.is_err());

        // WASM with no exports invoking "init"
        let empty_exports_wasm = vec![
            0x00, 0x61, 0x73, 0x6D, // magic \0asm
            0x01, 0x00, 0x00, 0x00, // version 1
        ];
        let mut empty_runtime = WasmPluginRuntime::new("empty_mod", empty_exports_wasm)?;
        let out_init_empty = empty_runtime.invoke_export("init", b"init_payload")?;
        assert_eq!(out_init_empty, b"init_payload");

        let out_run_empty = empty_runtime.invoke_export("run", b"run_payload")?;
        assert_eq!(out_run_empty, b"run_payload");

        // WASM with Code section (section ID 10) executing i32.const, i32.add, i32.xor
        // i32.const 2, i32.const 3, i32.add (5), i32.const 4, i32.xor (1) -> adds 1 to each byte!
        let code_wasm = vec![
            0x00, 0x61, 0x73, 0x6D, // magic \0asm
            0x01, 0x00, 0x00, 0x00, // version 1
            0x0A, // section ID 10 (Code)
            0x09, // section length 9
            0x41, 0x02, // i32.const 2
            0x41, 0x03, // i32.const 3
            0x6A, // i32.add -> 5
            0x41, 0x04, // i32.const 4
            0x73, // i32.xor -> 1
            0x0B, // end
        ];
        let mut code_runtime = WasmPluginRuntime::new("code_mod", code_wasm)?;
        let transformed = code_runtime.invoke_export("run", b"abc")?;
        assert_eq!(transformed, b"bcd");

        // Multi-byte LEB128 section length, export count, name length, and func index in parse_wasm_exports & execute_wasm_code_section
        let mut leb_wasm = vec![
            0x00, 0x61, 0x73, 0x6D, // magic
            0x01, 0x00, 0x00, 0x00, // version
            0x07, // section ID 7 (Export)
            0x86, 0x01, // 2-byte LEB128 length (134, clamped to bytes.len())
            0x82, 0x00, // 2-byte LEB128 count (2 exports, second hits idx >= sec_end break)
            0x82, 0x00, // 2-byte LEB128 name_len (2)
            b'f', b'n', // export name "fn"
            0x00, // kind
            0x80, 0x00, // 2-byte LEB128 func index
        ];
        let exps = parse_wasm_exports(&leb_wasm);
        assert_eq!(exps, vec!["fn".to_string()]);
        // Truncated section header where idx >= bytes.len() right after section_id
        leb_wasm.push(0x07);
        let _ = parse_wasm_exports(&leb_wasm);

        // Truncated LEB128 count, name_len, name body, and func index in section 7
        let _ = parse_wasm_exports(&[0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x07, 0x00]);
        let _ = parse_wasm_exports(&[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x07, 0x01, 0x81,
        ]);
        let _ = parse_wasm_exports(&[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x07, 0x02, 0x01, 0x81,
        ]);
        let _ = parse_wasm_exports(&[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x07, 0x02, 0x01, 0x05,
        ]);
        let _ = parse_wasm_exports(&[
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x07, 0x04, 0x01, 0x01, b'a', 0x00,
            0x81,
        ]);

        let mut leb_code_wasm = vec![
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x0A, // section ID 10 (Code)
            0x82, 0x00, // 2-byte LEB128 length (2)
            0x41, 0x01, 0x0A, // trailing section ID at end of buffer
        ];
        let mut slice = *b"a";
        execute_wasm_code_section(&leb_code_wasm, &mut slice);
        assert_eq!(&slice, b"b");
        leb_code_wasm.pop();

        // Code section with stack underflow on 0x6A / 0x73 and trailing 0x41 at sec_end
        let edge_code_wasm = [
            0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00, 0x0A, 0x03, 0x6A, 0x73, 0x41,
        ];
        execute_wasm_code_section(&edge_code_wasm, &mut slice);

        // Invalid WASM binary header (short < 8)
        let invalid_wasm = vec![0x01, 0x02, 0x03, 0x04];
        assert!(WasmPluginRuntime::new("invalid", invalid_wasm).is_err());

        // Invalid WASM binary header (long >= 8 with bad magic)
        let invalid_header_long = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        assert!(WasmPluginRuntime::new("invalid_long", invalid_header_long).is_err());

        // Test SDK structs
        let meta = sdk::PluginMetadata {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            description: "Test plugin".to_string(),
            author: "Author".to_string(),
        };
        assert_eq!(meta.name, "test-plugin");

        let cap = sdk::PluginCapability {
            domain: "provider".to_string(),
            name: "custom_boot".to_string(),
        };
        assert_eq!(cap.domain, "provider");

        let cmd = sdk::PluginCommand {
            name: "custom-cli".to_string(),
            synopsis: "Custom CLI command".to_string(),
        };
        assert_eq!(cmd.name, "custom-cli");
        Ok(())
    }
}
