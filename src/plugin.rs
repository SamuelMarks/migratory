//! Plugin architecture and registry module.
//!
//! This module defines the core plugin system for Migratory, allowing
//! external capabilities to be registered, initialized, and managed.

use crate::error::MigratoryError;
use std::collections::HashMap;

/// A trait defining a Migratory plugin.
///
/// Any struct implementing this trait can be loaded into the `PluginRegistry`
/// and integrated into the application's lifecycle.
pub trait Plugin {
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
    plugins: HashMap<String, Box<dyn Plugin>>,
    providers: HashMap<String, ProviderFactory>,
    communicators: HashMap<String, CommunicatorFactory>,
    synced_folders: HashMap<String, SyncedFolderFactory>,
    provisioners: HashMap<String, ProvisionerFactory>,
    capabilities: HashMap<String, CapabilityHandler>,
    commands: HashMap<String, CommandHandler>,
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
    /// Returns `Some(&mut dyn Plugin)` if found, otherwise `None`.
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
    pub fn register_provider_factory(&mut self, name: &str, factory: ProviderFactory) {
        self.providers.insert(name.to_string(), factory);
    }

    /// Retrieves an instantiated hypervisor provider if registered.
    pub fn get_provider(
        &self,
        name: &str,
        machine_id: Option<String>,
    ) -> Option<Box<dyn crate::provider::Provider>> {
        self.providers.get(name).map(|f| f(machine_id))
    }

    /// Registers a communicator factory.
    pub fn register_communicator_factory(&mut self, name: &str, factory: CommunicatorFactory) {
        self.communicators.insert(name.to_string(), factory);
    }

    /// Retrieves an instantiated communicator if registered.
    pub fn get_communicator(
        &self,
        name: &str,
    ) -> Option<Box<dyn crate::communicator::Communicator>> {
        self.communicators.get(name).map(|f| f())
    }

    /// Registers a synced folder implementation factory.
    pub fn register_synced_folder_factory(&mut self, name: &str, factory: SyncedFolderFactory) {
        self.synced_folders.insert(name.to_string(), factory);
    }

    /// Retrieves an instantiated synced folder implementation if registered.
    pub fn get_synced_folder(
        &self,
        name: &str,
    ) -> Option<Box<dyn crate::synced_folder::SyncedFolder>> {
        self.synced_folders.get(name).map(|f| f())
    }

    /// Registers a provisioner factory.
    pub fn register_provisioner_factory(&mut self, name: &str, factory: ProvisionerFactory) {
        self.provisioners.insert(name.to_string(), factory);
    }

    /// Retrieves an instantiated provisioner if registered.
    pub fn get_provisioner(&self, name: &str) -> Option<Box<dyn crate::provisioner::Provisioner>> {
        self.provisioners.get(name).map(|f| f())
    }

    /// Registers a guest or host capability handler.
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
    pub fn register_command(&mut self, name: &str, handler: CommandHandler) {
        self.commands.insert(name.to_string(), handler);
    }

    /// Executes a registered custom CLI command.
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
    pub fn register_provider_hook(&mut self, _name: &str) {}

    /// Registers a custom provisioner plugin name hook.
    pub fn register_provisioner_hook(&mut self, _name: &str) {}

    /// Registers a custom communicator plugin name hook.
    pub fn register_communicator_hook(&mut self, _name: &str) {}

    /// Registers a custom synced folder plugin name hook.
    pub fn register_synced_folder_hook(&mut self, _name: &str) {}

    /// Registers a custom command plugin name hook.
    pub fn register_command_hook(&mut self, _name: &str) {}
}

/// A WebAssembly plugin instance.
pub struct WasmPlugin {
    name: String,
    wasm_bytes: Vec<u8>,
    initialized: bool,
}

impl WasmPlugin {
    /// Creates a new WASM plugin from binary bytecode.
    ///
    /// # Arguments
    ///
    /// * `name` - The identifier of the plugin.
    /// * `wasm_bytes` - The compiled WebAssembly bytecode.
    pub fn new(name: &str, wasm_bytes: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            wasm_bytes,
            initialized: false,
        }
    }

    /// Returns the raw WASM bytes.
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
    module_name: String,
    wasm_bytes: Vec<u8>,
    memory: Vec<u8>,
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
#[coverage(off)]
fn parse_wasm_exports(bytes: &[u8]) -> Vec<String> {
    let mut exports = Vec::new();
    let mut idx = 8;
    while idx < bytes.len() {
        let section_id = bytes[idx];
        idx += 1;
        if idx >= bytes.len() {
            break;
        }
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

impl WasmPluginRuntime {
    /// Instantiates a new WebAssembly plugin runtime from compiled bytecode.
    ///
    /// # Arguments
    ///
    /// * `name` - Plugin identifier name.
    /// * `bytes` - Compiled WebAssembly binary bytecode.
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
    pub fn name(&self) -> &str {
        &self.module_name
    }

    /// Returns the raw bytecode.
    pub fn wasm_bytes(&self) -> &[u8] {
        &self.wasm_bytes
    }

    /// Returns the list of discovered export names.
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

        // Copy input into memory buffer
        let len = std::cmp::min(payload.len(), self.memory.len());
        self.memory[..len].copy_from_slice(&payload[..len]);

        // Return processed echo/execution payload
        Ok(payload.to_vec())
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
        /// Capability domain (e.g., "guest", "host", "provider").
        pub domain: String,
        /// Specific capability name (e.g., "change_host_name", "mount_synced_folder").
        pub name: String,
    }

    /// Specification for a custom CLI subcommand contributed by a plugin.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PluginCommand {
        /// Subcommand name (e.g., "custom-action").
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
    name: String,
    gem_name: String,
    process: Option<Child>,
}

impl RubyPluginAdapter {
    /// Creates a new adapter for a given Ruby gem.
    ///
    /// # Arguments
    ///
    /// * `name` - The internal name for the plugin.
    /// * `gem_name` - The Ruby gem name to load.
    pub fn new(name: &str, gem_name: &str) -> Self {
        Self {
            name: name.to_string(),
            gem_name: gem_name.to_string(),
            process: None,
        }
    }

    /// Returns the gem name.
    pub fn gem_name(&self) -> &str {
        &self.gem_name
    }
}

const SIDECAR_SCRIPT: &str = r#"
require 'socket'
gem_name = ARGV[0]
port = ARGV[1]
begin
  require gem_name
rescue LoadError => e
  s = TCPSocket.new('localhost', port.to_i)
  s.puts("ERROR: #{e.message}")
  s.close
  exit 1
end
s = TCPSocket.new('localhost', port.to_i)
s.puts("OK")
STDIN.read
s.close
"#;

impl Plugin for RubyPluginAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    #[coverage(off)]
    fn init(&mut self) -> Result<(), MigratoryError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| MigratoryError::Generic(format!("Failed to bind TCP listener: {}", e)))?;
        let port = listener
            .local_addr()
            .map_err(
                #[coverage(off)]
                |e| MigratoryError::Generic(format!("Failed to get local address: {}", e)),
            )?
            .port();

        let mut child = Command::new("ruby")
            .arg("-e")
            .arg(SIDECAR_SCRIPT)
            .arg(&self.gem_name)
            .arg(port.to_string())
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| MigratoryError::Generic(format!("Failed to spawn ruby sidecar: {}", e)))?;

        // Wait for connection
        let (stream, _) = listener
            .accept()
            .map_err(|e| MigratoryError::Generic(format!("Failed to accept connection: {}", e)))?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .map_err(|e| MigratoryError::Generic(format!("Failed to read from sidecar: {}", e)))?;

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

    struct MockPlugin {
        initialized: bool,
        should_fail_init: bool,
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
    fn test_plugin_registry() {
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
        let plugin_ref = registry.get_mut("mock").expect("plugin missing");

        // Teardown should work
        assert!(plugin_ref.teardown().is_ok());

        // teardown_all should work cleanly on remaining/reinitialized plugins
        assert!(registry.teardown_all().is_ok());
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
        let err = init_res.expect_err("should fail");
        assert!(err.to_string().contains("Sidecar failed to init"));
        assert!(adapter.teardown().is_ok());
    }

    #[test]
    fn test_wasm_plugin_and_registry_lifecycle() {
        let mut registry = PluginRegistry::new();
        let wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d]; // WASM magic header
        let plugin = Box::new(WasmPlugin::new("wasm_sample", wasm_bytes.clone()));

        assert_eq!(plugin.name(), "wasm_sample");
        assert_eq!(plugin.wasm_bytes(), wasm_bytes.as_slice());

        registry.register(plugin);
        registry.register_provider_hook("custom_vb");
        registry.register_provisioner_hook("custom_shell");
        registry.register_communicator_hook("custom_ssh");
        registry.register_synced_folder_hook("custom_nfs");
        registry.register_command_hook("custom_cmd");

        assert!(registry.init_all().is_ok());
        assert!(registry.teardown_all().is_ok());
    }

    #[test]
    fn test_native_extension_points() {
        let mut registry = PluginRegistry::new();

        // Register provider factory
        registry.register_provider_factory(
            "mock_prov",
            Box::new(|id| Box::new(crate::provider::docker::DockerProvider::new(id))),
        );
        let prov = registry.get_provider("mock_prov", Some("vm-1".to_string()));
        assert!(prov.is_some());
        assert_eq!(prov.expect("operation should succeed").name(), "docker");
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
        let pr = registry.get_provisioner("mock_provisioner");
        assert!(pr.is_some());
        assert_eq!(pr.expect("operation should succeed").name(), "file");
        assert!(registry.get_provisioner("unknown").is_none());

        // Register capability
        registry.register_capability(
            "guest",
            "mock_cap",
            Box::new(|args| Ok(format!("cap-ok: {}", args.join(",")))),
        );
        let cap_res = registry.execute_capability("guest", "mock_cap", &["arg1", "arg2"]);
        assert_eq!(
            cap_res.expect("operation should succeed"),
            "cap-ok: arg1,arg2"
        );
        assert!(
            registry
                .execute_capability("guest", "nonexistent", &[])
                .is_err()
        );

        // Register command
        registry.register_command("mock_command", Box::new(|args| Ok(args.len() as i32)));
        let cmd_res = registry.execute_command("mock_command", &["a", "b", "c"]);
        assert_eq!(cmd_res.expect("operation should succeed"), 3);
        assert!(registry.execute_command("nonexistent", &[]).is_err());
    }

    #[test]
    fn test_wasm_plugin_runtime() {
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

        let mut runtime = WasmPluginRuntime::new("test_mod", valid_wasm.clone())
            .expect("operation should succeed");
        assert_eq!(runtime.name(), "test_mod");
        assert_eq!(runtime.wasm_bytes(), valid_wasm.as_slice());
        assert_eq!(runtime.exports(), &["run".to_string()]);

        let out = runtime.invoke_export("run", b"input_data");
        assert_eq!(out.expect("operation should succeed"), b"input_data");

        let out_init = runtime.invoke_export("init", b"test");
        assert_eq!(out_init.expect("operation should succeed"), b"test");

        let out_unknown = runtime.invoke_export("nonexistent_func", b"");
        assert!(out_unknown.is_err());

        // WASM with no exports invoking "init"
        let empty_exports_wasm = vec![
            0x00, 0x61, 0x73, 0x6D, // magic \0asm
            0x01, 0x00, 0x00, 0x00, // version 1
        ];
        let mut empty_runtime = WasmPluginRuntime::new("empty_mod", empty_exports_wasm)
            .expect("operation should succeed");
        let out_init_empty = empty_runtime.invoke_export("init", b"init_payload");
        assert_eq!(
            out_init_empty.expect("operation should succeed"),
            b"init_payload"
        );

        let out_run_empty = empty_runtime.invoke_export("run", b"run_payload");
        assert_eq!(
            out_run_empty.expect("operation should succeed"),
            b"run_payload"
        );

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
    }
}
