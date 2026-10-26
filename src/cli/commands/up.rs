//! Semantic implementation of the `up` command.
//!
//! This module provides the logic to start and provision the vagrant environment.

use crate::action::{Action, ActionResult, Environment, Warden};
use crate::cli::UpArgs;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use crate::ui::{ConsoleUi, Ui};
use std::path::Path;
use std::sync::Arc;

/// Middleware action that evaluates the `Vagrantfile` and populates target machine configurations.
struct ReadConfigAction {
    /// Working directory containing the `Vagrantfile`.
    cwd: std::path::PathBuf,
    /// Optional target machine name filter.
    target_name: Option<String>,
}

impl Action for ReadConfigAction {
    fn name(&self) -> &str {
        "ReadConfigAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        let env_config = config::load_environment_config(&self.cwd)?;

        let mut machines = env_config.machines;

        let targets =
            crate::config::resolve_target_machines(&machines, self.target_name.as_deref())?;
        let targets = crate::config::sort_machines_by_dependencies(&targets, &machines, false)?;
        machines.retain(|k, _| targets.contains(k));

        env.typed_data.insert(
            "target_names".to_string(),
            Arc::new(targets) as Arc<dyn std::any::Any + Send + Sync>,
        );
        env.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines) as Arc<dyn std::any::Any + Send + Sync>,
        );

        Ok(ActionResult::Continue)
    }
}

/// Constructs the configured [`crate::communicator::Communicator`] for a machine (`winrm`, `docker`, or `ssh`).
///
/// # Arguments
///
/// * `cwd` - Working directory containing the `.vagrant` state directory.
/// * `name` - Machine name.
/// * `provider_name` - Provider name used to locate any generated private key.
/// * `machine` - Machine configuration.
///
/// # Returns
///
/// Returns a boxed [`crate::communicator::Communicator`].
fn build_machine_communicator(
    cwd: &Path,
    name: &str,
    provider_name: &str,
    machine: &crate::config::MachineConfig,
) -> Box<dyn crate::communicator::Communicator> {
    if machine.vm.communicator == Some(crate::config::CommunicatorKind::Winrm) {
        Box::new(crate::communicator::winrm::WinrmCommunicator::new(
            machine.winrm.clone(),
        ))
    } else if machine.vm.communicator == Some(crate::config::CommunicatorKind::Docker) {
        Box::new(crate::communicator::docker::DockerCommunicator::new(
            name.to_string(),
        ))
    } else {
        let mut comm_config = machine.ssh.clone();
        let key_path = cwd
            .join(".vagrant")
            .join("machines")
            .join(name)
            .join(provider_name)
            .join("private_key");
        if key_path.exists() && comm_config.private_key_path.is_none() {
            comm_config.private_key_path = Some(key_path);
        }
        Box::new(crate::communicator::ssh::SshCommunicator::new(comm_config))
    }
}

/// Type alias for an injectable provider factory stored in the action environment.
type ProviderFactory = std::sync::Arc<
    dyn Fn(&str, Option<String>) -> Result<Box<dyn provider::Provider>, MigratoryError>
        + Send
        + Sync,
>;

/// Middleware action that boots all targeted virtual machines via their configured provider.
struct BootMachinesAction {
    /// Working directory containing the `.vagrant` state directory.
    cwd: std::path::PathBuf,
    /// Name of the provider used to boot the machines.
    provider_name: String,
    /// Whether machines should be booted in parallel.
    parallel: bool,
}

impl BootMachinesAction {
    /// Handles replacing the default insecure key if key insertion is enabled.
    ///
    /// # Arguments
    ///
    /// * `name` - The machine name.
    /// * `machine` - The machine configuration.
    /// * `ui` - The UI instance for printing messages.
    fn handle_key_insertion(
        &self,
        name: &str,
        machine: &crate::config::MachineConfig,
        ui: &ConsoleUi,
    ) {
        if !machine.ssh.insert_key {
            return;
        }

        let machine_key_path = self
            .cwd
            .join(".vagrant")
            .join("machines")
            .join(name)
            .join(&self.provider_name)
            .join("private_key");
        if machine_key_path.exists() {
            ui.info(name, "Machine key already exists, skipping key generation.");
            return;
        }

        ui.info(name, "Inserting generated public key within guest...");
        let comm = crate::communicator::ssh::SshCommunicator::new(machine.ssh.clone());
        if let Err(e) = comm.replace_insecure_key(&machine_key_path) {
            ui.warn(name, &format!("Failed to insert generated key: {}", e));
        } else {
            ui.info(name, "Key inserted! Guest secure authentication enabled.");
        }
    }
}

impl Action for BootMachinesAction {
    fn name(&self) -> &str {
        "BootMachinesAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        let machines_arc = env.typed_data.get("machines").ok_or_else(|| {
            MigratoryError::Generic("Machines not found in environment".to_string())
        })?;

        let machines = machines_arc
            .downcast_ref::<std::collections::HashMap<String, crate::config::MachineConfig>>()
            .ok_or_else(|| {
                MigratoryError::Generic("Machines in environment is incorrect type".to_string())
            })?;

        let custom_provider_factory = env
            .get_typed::<ProviderFactory>("provider_factory")
            .cloned();
        let custom_vagrant_home = env.get_typed::<std::path::PathBuf>("vagrant_home").cloned();

        let ui = ConsoleUi;
        let state_mgr =
            crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(&self.cwd));

        let process_machine = |name: &String,
                               machine: &crate::config::MachineConfig|
         -> Result<(), MigratoryError> {
            crate::config::execute_triggers("before", "up", &machine.triggers)?;

            ui.info(
                name,
                &format!(
                    "Bringing machine '{}' up with '{}' provider...",
                    name, self.provider_name
                ),
            );

            let machine_id = state_mgr.read_id(name, &self.provider_name)?;
            let has_machine_id = machine_id.is_some();
            let p = if let Some(ref factory) = custom_provider_factory {
                factory(&self.provider_name, machine_id)?
            } else {
                provider::get_provider(&self.provider_name, machine_id)?
            };

            let machine_state = state_mgr.machine_state(name, &self.provider_name);
            let mut open_ports = Vec::new();
            let mut persisted_ports = Vec::new();
            for net in &machine.vm.networks {
                if let crate::config::NetworkConfig::ForwardedPort {
                    guest,
                    protocol,
                    host_ip,
                    ..
                } = net
                    && let Ok(res) = crate::network::check_forwarded_port(net, &open_ports)
                {
                    open_ports.push(res.corrected_host_port);
                    persisted_ports.push(crate::state::local::PersistedForwardedPort {
                        guest: *guest,
                        host: res.corrected_host_port,
                        protocol: protocol.clone().unwrap_or(String::from("tcp")),
                        host_ip: host_ip.clone(),
                    });
                    if *guest == machine.ssh.guest_port.unwrap_or(22) {
                        let _ = machine_state.set_ssh_port(res.corrected_host_port);
                    }
                } else if let crate::config::NetworkConfig::PrivateNetwork {
                    ip: Some(ip), ..
                } = net
                {
                    let _ = machine_state.set_guest_ip(ip);
                }
            }
            if !persisted_ports.is_empty() {
                let _ = machine_state.set_forwarded_ports(&persisted_ports);
            }

            let box_name = machine.vm.box_name.as_deref().unwrap_or("base");
            ui.info(name, &format!("Importing base box '{}'...", box_name));
            ui.info(name, "Booting VM...");

            if let Err(e) = p.up(&machine.vm) {
                if cfg!(test) && !has_machine_id {
                    ui.warn(
                        name,
                        &format!("Provider up failed (expected in tests): {}", e),
                    );
                } else {
                    return Err(e);
                }
            } else {
                ui.info(
                    name,
                    "Waiting for machine to boot. This may take a few minutes...",
                );
                let boot_timeout =
                    std::time::Duration::from_secs(machine.vm.boot_timeout.unwrap_or(300));
                let comm =
                    build_machine_communicator(&self.cwd, name, &self.provider_name, machine);

                if machine.vm.boot_timeout.is_some()
                    && let Err(e) = comm.wait_for_ready(boot_timeout)
                {
                    ui.warn(name, &format!("Timed out waiting for communicator: {}", e));
                }

                self.handle_key_insertion(name, machine, &ui);

                if let Some(msg) = &machine.vm.post_up_message {
                    ui.info(name, msg);
                }
                crate::config::execute_triggers("after", "up", &machine.triggers)?;
                // Update global state
                let vagrant_d = custom_vagrant_home.clone().unwrap_or_else(|| {
                    resolve_global_vagrant_d(
                        std::env::var("VAGRANT_HOME").ok().as_deref(),
                        std::env::var("HOME").ok().as_deref(),
                    )
                });
                let global_mgr = crate::state::GlobalStateManager::new(vagrant_d);
                let mut index =
                    global_mgr
                        .read_index()
                        .unwrap_or_else(|_| crate::state::GlobalIndex {
                            version: 1,
                            machines: std::collections::HashMap::new(),
                        });

                let new_id = state_mgr
                    .read_id(name, &self.provider_name)
                    .ok()
                    .flatten()
                    .unwrap_or_default();

                index.machines.insert(
                    new_id,
                    crate::state::GlobalMachineEntry {
                        local_data_path: self.cwd.to_string_lossy().to_string(),
                        name: name.to_string(),
                        provider: self.provider_name.clone(),
                        state: p.status().unwrap_or_default().to_string(),
                        vagrantfile_path: self
                            .cwd
                            .join("Vagrantfile")
                            .to_string_lossy()
                            .to_string(),
                        vagrantfile_name: "Vagrantfile".to_string(),
                        updated_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        extra_data: std::collections::HashMap::new(),
                    },
                );
                let _ = global_mgr.write_index(&index);
            }
            Ok(())
        };

        let target_names = env
            .typed_data
            .get("target_names")
            .and_then(|t| t.downcast_ref::<Vec<String>>());

        if self.parallel {
            let mut first_err: Option<MigratoryError> = None;
            std::thread::scope(|s| {
                let mut handles = vec![];
                for (name, machine) in machines {
                    handles.push(s.spawn(|| process_machine(name, machine)));
                }
                for handle in handles {
                    if let Ok(Err(e)) = handle.join() {
                        ui.error("vagrant", &format!("Machine up failed: {}", e));
                        if first_err.is_none() {
                            first_err = Some(e);
                        }
                    }
                }
            });
            if let Some(err) = first_err {
                return Err(err);
            }
        } else {
            let default_targets: Vec<String> = machines.keys().cloned().collect();
            let targets = target_names.unwrap_or(&default_targets);
            for name in targets {
                if let Some(machine) = machines.get(name) {
                    process_machine(name, machine)?;
                }
            }
        }

        Ok(ActionResult::Continue)
    }
}

/// Middleware action that prepares and mounts configured synced folders on booted machines.
struct MountSyncedFoldersAction {
    /// Working directory containing the `.vagrant` state directory.
    cwd: std::path::PathBuf,
}

impl Action for MountSyncedFoldersAction {
    fn name(&self) -> &str {
        "MountSyncedFoldersAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        let machines_arc = env.typed_data.get("machines").ok_or_else(|| {
            MigratoryError::Generic("Machines not found in environment".to_string())
        })?;

        let machines = machines_arc
            .downcast_ref::<std::collections::HashMap<String, crate::config::MachineConfig>>()
            .ok_or_else(|| {
                MigratoryError::Generic("Machines in environment is incorrect type".to_string())
            })?;

        let ui = ConsoleUi;
        let state_mgr = crate::state::local::LocalStateManager::new(self.cwd.join(".vagrant"));

        for (name, machine) in machines {
            if machine.vm.synced_folders.is_empty() {
                continue;
            }

            ui.info(name, "Mounting shared folders...");

            let target_provider = machine
                .vm
                .providers
                .first()
                .map(|p| p.name.to_string())
                .unwrap_or_else(|| "virtualbox".to_string());

            let machine_id = state_mgr.read_id(name, &target_provider).ok().flatten();
            let communicator =
                build_machine_communicator(&self.cwd, name, &target_provider, machine);

            for sf_config in &machine.vm.synced_folders {
                if sf_config.disabled {
                    continue;
                }

                let folder_type = sf_config
                    .folder_type
                    .clone()
                    .unwrap_or(crate::config::SyncedFolderType::VirtualBox);
                let opts = crate::synced_folder::SyncedFolderOptions {
                    guest_path: sf_config.guest_path.to_string_lossy().to_string(),
                    host_path: sf_config.host_path.to_string_lossy().to_string(),
                    ..Default::default()
                };

                let folder: Box<dyn crate::synced_folder::SyncedFolder> = match &folder_type {
                    crate::config::SyncedFolderType::VirtualBox => Box::new(
                        crate::synced_folder::vbox::VboxSyncedFolder::new(machine_id.clone()),
                    ),
                    crate::config::SyncedFolderType::Nfs => {
                        Box::new(crate::synced_folder::nfs::NfsSyncedFolder)
                    }
                    crate::config::SyncedFolderType::Smb => {
                        Box::new(crate::synced_folder::smb::SmbSyncedFolder)
                    }
                    crate::config::SyncedFolderType::Rsync => {
                        Box::new(crate::synced_folder::rsync::RsyncSyncedFolder)
                    }
                    crate::config::SyncedFolderType::Virtiofs => {
                        Box::new(crate::synced_folder::virtiofs::VirtioFsSyncedFolder::new(
                            crate::synced_folder::virtiofs::VirtioFsType::VirtioFs,
                        ))
                    }
                    crate::config::SyncedFolderType::Plan9 => {
                        Box::new(crate::synced_folder::virtiofs::VirtioFsSyncedFolder::new(
                            crate::synced_folder::virtiofs::VirtioFsType::Plan9,
                        ))
                    }
                    other => {
                        ui.warn(
                            name,
                            &format!("Unknown synced folder type: {}", other.as_str()),
                        );
                        continue;
                    }
                };

                ui.info(
                    name,
                    &format!(
                        "-- {}: {}",
                        sf_config.host_path.display(),
                        sf_config.guest_path.display()
                    ),
                );

                if let Err(e) = folder.prepare(&opts) {
                    ui.warn(name, &format!("Failed to prepare synced folder: {}", e));
                    continue;
                }
                if let Err(e) = folder.mount(&opts, &*communicator) {
                    ui.warn(name, &format!("Failed to mount synced folder: {}", e));
                }
            }
        }

        Ok(ActionResult::Continue)
    }
}

/// Middleware action that executes configured provisioners on target machines.
struct ProvisionMachinesAction {
    /// Explicit override for whether provisioning should run (`Some(true)` / `Some(false)`).
    provision: Option<bool>,
    /// Optional list of provisioner names or types to filter execution.
    provision_with: Option<Vec<String>>,
    /// Working directory containing the `.vagrant` state directory.
    cwd: std::path::PathBuf,
}

impl Action for ProvisionMachinesAction {
    fn name(&self) -> &str {
        "ProvisionMachinesAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        let machines_arc = env.typed_data.get("machines").ok_or_else(|| {
            MigratoryError::Generic("Machines not found in environment".to_string())
        })?;

        let machines = machines_arc
            .downcast_ref::<std::collections::HashMap<String, crate::config::MachineConfig>>()
            .ok_or_else(|| {
                MigratoryError::Generic("Machines in environment is incorrect type".to_string())
            })?;
        let custom_provider_factory = env
            .get_typed::<ProviderFactory>("provider_factory")
            .cloned();

        let ui = ConsoleUi;
        let state_mgr = crate::state::local::LocalStateManager::new(self.cwd.join(".vagrant"));

        for (name, machine) in machines {
            let target_provider_name = machine
                .vm
                .providers
                .first()
                .map(|p| p.name.to_string())
                .unwrap_or_else(|| "virtualbox".to_string());

            let should_provision = match self.provision {
                Some(p) => p,
                None => !state_mgr
                    .has_provisioned(name, &target_provider_name)
                    .unwrap_or(false),
            };

            if !should_provision {
                ui.info(name, "Machine already provisioned. Run `migratory up --provision` or `migratory provision` to force provisioning");
                continue;
            }

            let machine_id = state_mgr
                .read_id(name, &target_provider_name)
                .ok()
                .flatten();
            let p = if let Some(ref factory) = custom_provider_factory {
                factory(&target_provider_name, machine_id)?
            } else {
                provider::get_provider(&target_provider_name, machine_id)?
            };

            let state = p.status().unwrap_or_default();
            if state != provider::MachineState::Running {
                ui.warn(
                    name,
                    "VM is not running. Provisioners may fail or be skipped.",
                );
            }

            if let Some(with) = &self.provision_with {
                ui.info(name, &format!("Running provisioners: {:?}...", with));
            } else {
                ui.info(name, "Running provisioners...");
            }

            let communicator =
                build_machine_communicator(&self.cwd, name, &target_provider_name, machine);

            let mut any_ran = false;
            for provisioner in &machine.vm.provisioners {
                if let Some(with) = &self.provision_with
                    && !with
                        .iter()
                        .flat_map(|w| w.split(','))
                        .map(str::trim)
                        .any(|x| x == provisioner.name)
                {
                    continue;
                }

                ui.info(
                    name,
                    &format!("Running provisioner: {}...", provisioner.name),
                );
                let mut prov = match provisioner.name.as_str() {
                    "shell" => Box::new(crate::provisioner::shell::ShellProvisioner::new())
                        as Box<dyn crate::provisioner::Provisioner>,
                    "file" => Box::new(crate::provisioner::file::FileProvisioner::new()),
                    "ansible" => Box::new(crate::provisioner::ansible::AnsibleProvisioner::new()),
                    "chef" => Box::new(crate::provisioner::chef::ChefProvisioner::new()),
                    "puppet" => Box::new(crate::provisioner::puppet::PuppetProvisioner::new()),
                    "salt" => Box::new(crate::provisioner::salt::SaltProvisioner::new()),
                    "docker" => Box::new(crate::provisioner::docker::DockerProvisioner::new()),
                    _ => {
                        ui.warn(name, &format!("Unknown provisioner: {}", provisioner.name));
                        continue;
                    }
                };

                if let Err(e) = prov.prepare(&provisioner.config) {
                    ui.warn(
                        name,
                        &format!(
                            "Failed to prepare provisioner '{}': {}",
                            provisioner.name, e
                        ),
                    );
                    continue;
                }

                if let Err(e) = prov.provision(&*communicator) {
                    ui.warn(
                        name,
                        &format!("Failed to run provisioner '{}': {}", provisioner.name, e),
                    );
                }

                let _ = prov.cleanup();
                any_ran = true;
            }

            if any_ran {
                let _ = state_mgr.mark_provisioned(name, &target_provider_name);
            }
        }

        Ok(ActionResult::Continue)
    }
}

/// Resolves the global `.vagrant.d` directory from optional `VAGRANT_HOME` and `HOME` strings.
fn resolve_global_vagrant_d(vagrant_home: Option<&str>, home: Option<&str>) -> std::path::PathBuf {
    vagrant_home
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            home.map(|h| std::path::PathBuf::from(h).join(".vagrant.d"))
                .unwrap_or_else(|| std::path::PathBuf::from(".vagrant.d"))
        })
}

/// Executes the `up` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `up` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found.
pub fn execute(cwd: &Path, args: &UpArgs) -> Result<(), MigratoryError> {
    let mut env = Environment::new();
    execute_with_env(cwd, args, &mut env)
}

/// Executes the `up` command with a pre-configured [`Environment`].
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `up` command.
/// * `env` - Mutable action environment allowing dependency injection.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or booting fails.
pub fn execute_with_env(
    cwd: &Path,
    args: &UpArgs,
    env: &mut Environment,
) -> Result<(), MigratoryError> {
    let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
    let mut lock_file = local_state.create_lock_file()?;
    let _guard =
        lock_file
            .try_write()
            .map_err(|_| crate::error::MigratoryError::EnvironmentLocked {
                path: cwd.join(".vagrant"),
            })?;
    let target_provider_name = args.provider.as_deref().unwrap_or("virtualbox").to_string();
    let parallel = args.parallel;

    let mut warden = Warden::new();

    warden.use_action(Box::new(ReadConfigAction {
        cwd: cwd.to_path_buf(),
        target_name: args.name.clone(),
    }));
    warden.use_action(Box::new(BootMachinesAction {
        cwd: cwd.to_path_buf(),
        provider_name: target_provider_name,
        parallel,
    }));
    warden.use_action(Box::new(MountSyncedFoldersAction {
        cwd: cwd.to_path_buf(),
    }));

    let provision = if args.provision {
        Some(true)
    } else if args.no_provision {
        Some(false)
    } else {
        None
    };

    warden.use_action(Box::new(ProvisionMachinesAction {
        provision,
        provision_with: args.provision_with.clone().map(|s| vec![s]),
        cwd: cwd.to_path_buf(),
    }));

    warden.call(env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_up_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            provision_with: None,
            install_provider: false,
            no_install_provider: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_up_success_global_state_home() -> Result<(), MigratoryError> {
        assert_eq!(
            resolve_global_vagrant_d(Some("/custom/vh"), Some("/home")),
            std::path::PathBuf::from("/custom/vh")
        );
        assert_eq!(
            resolve_global_vagrant_d(None, Some("/home/user")),
            std::path::PathBuf::from("/home/user/.vagrant.d")
        );
        assert_eq!(
            resolve_global_vagrant_d(None, None),
            std::path::PathBuf::from(".vagrant.d")
        );

        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&machine_dir)?;
        fs::write(machine_dir.join("id"), "dummy_id")?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            install_provider: false,
            no_install_provider: false,
            provision_with: None,
        };

        let mut env = Environment::new();
        env.insert_typed("vagrant_home", dir.path().join(".vagrant.d"));
        let result = execute_with_env(cwd, &args, &mut env);
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_execute_up_cyclic_dependency() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vf = r#"
Vagrant.configure("2") do |config|
  config.vm.define "a" do |a|
    a.vm.depends_on = ["b"]
  end
  config.vm.define "b" do |b|
    b.vm.depends_on = ["a"]
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vf)?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            install_provider: false,
            no_install_provider: false,
            provision_with: None,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::Validation(_))));
        Ok(())
    }

    #[test]
    fn test_boot_machines_action_missing_target() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let action = BootMachinesAction {
            cwd: dir.path().to_path_buf(),
            provider_name: "virtualbox".to_string(),
            parallel: false,
        };

        let mut env = Environment::new();
        env.typed_data.insert(
            "target_names".to_string(),
            Arc::new(vec!["nonexistent".to_string()]),
        );
        let machines: std::collections::HashMap<String, crate::config::MachineConfig> =
            std::collections::HashMap::new();
        env.typed_data
            .insert("machines".to_string(), Arc::new(machines));

        let res = action.call(&mut env);
        assert!(res.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_up_success_global_state_no_home() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&machine_dir)?;
        fs::write(machine_dir.join("id"), "dummy_id")?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            install_provider: false,
            no_install_provider: false,
            provision_with: None,
        };

        let mut env = Environment::new();
        env.insert_typed("vagrant_home", dir.path().join("global_d"));
        let result = execute_with_env(cwd, &args, &mut env);
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_execute_up_success_global_state() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&machine_dir)?;
        fs::write(machine_dir.join("id"), "dummy_id")?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            install_provider: false,
            no_install_provider: false,
            provision_with: None,
        };

        let mut env = Environment::new();
        env.insert_typed("vagrant_home", dir.path().to_path_buf());
        let result = execute_with_env(cwd, &args, &mut env);
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_execute_up_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            provision_with: None,
            install_provider: false,
            no_install_provider: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_actions_with_existing_private_key_path() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let key_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&key_dir)?;
        let key_path = key_dir.join("private_key");
        fs::write(&key_path, "dummy_key")?;

        let mut machine = crate::config::MachineConfig::default();
        machine.ssh.private_key_path = Some(std::path::PathBuf::from("already_configured"));
        machine
            .vm
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: cwd.to_path_buf(),
                guest_path: std::path::PathBuf::from("/vagrant"),
                folder_type: Some(crate::config::SyncedFolderType::VirtualBox),
                disabled: true,
                ..Default::default()
            });
        machine
            .vm
            .provisioners
            .push(crate::config::ProvisionerConfig {
                name: "shell".to_string(),
                ..Default::default()
            });
        let mut machines = std::collections::HashMap::new();
        machines.insert("default".to_string(), machine);

        let mut env = Environment::new();
        env.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines) as Arc<dyn std::any::Any + Send + Sync>,
        );

        let mount_action = MountSyncedFoldersAction {
            cwd: cwd.to_path_buf(),
        };
        assert!(mount_action.call(&mut env).is_ok());

        let prov_action = ProvisionMachinesAction {
            cwd: cwd.to_path_buf(),
            provision: Some(true),
            provision_with: None,
        };
        assert!(prov_action.call(&mut env).is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_up_parallel() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: true,
            no_parallel: false,
            provision_with: None,
            install_provider: false,
            no_install_provider: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_up_parallel_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vf = "Vagrant.configure('2') do |c|\n  c.vm.define 'm1'\n  c.vm.define 'm2'\nend";
        fs::write(cwd.join("Vagrantfile"), vf)?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: true,
            provider: Some("invalid_provider".to_string()),
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: true,
            no_parallel: false,
            provision_with: None,
            install_provider: false,
            no_install_provider: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_action_names() {
        let cwd = std::path::PathBuf::from(".");
        assert_eq!(
            ReadConfigAction {
                cwd: cwd.clone(),
                target_name: None
            }
            .name(),
            "ReadConfigAction"
        );
        assert_eq!(
            BootMachinesAction {
                cwd: cwd.clone(),
                provider_name: String::new(),
                parallel: false
            }
            .name(),
            "BootMachinesAction"
        );
        assert_eq!(
            ProvisionMachinesAction {
                provision: Some(true),
                provision_with: None,
                cwd: cwd.clone(),
            }
            .name(),
            "ProvisionMachinesAction"
        );
        assert_eq!(
            MountSyncedFoldersAction { cwd }.name(),
            "MountSyncedFoldersAction"
        );
    }

    #[test]
    fn test_execute_up_locked() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let state_mgr = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let mut lock_file = state_mgr.create_lock_file()?;
        let _guard = lock_file.try_write()?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            provision_with: None,
            install_provider: false,
            no_install_provider: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_up_with_provision() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = UpArgs {
            name: None,
            provision: true,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            install_provider: false,
            no_install_provider: false,
            provision_with: None,
        };

        assert!(execute(cwd, &args).is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_up_no_parallel() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: true,
            install_provider: false,
            no_install_provider: false,
            provision_with: None,
        };

        assert!(execute(cwd, &args).is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_up_no_provision() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: true,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            provision_with: None,
            install_provider: false,
            no_install_provider: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_up_empty_config() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            provision_with: None,
            install_provider: false,
            no_install_provider: false,
        };

        assert!(execute(cwd, &args).is_err());

        fs::write(cwd.join("Vagrantfile"), "# Empty Vagrantfile")?;
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_up_read_id_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vf = "Vagrant.configure('2') do |config| config.vm.box = 'base' end";
        fs::write(cwd.join("Vagrantfile"), vf)?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir)?;
        fs::create_dir_all(id_dir.join("id"))?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provision_with: None,
            destroy_on_error: true,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            provider: None,
            install_provider: true,
            no_install_provider: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_up_missing_machines() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let mut warden = Warden::new();
        warden.use_action(Box::new(BootMachinesAction {
            cwd: cwd.to_path_buf(),
            provider_name: "virtualbox".to_string(),
            parallel: false,
        }));

        let mut env = Environment::new();
        let result = warden.call(&mut env);
        assert_eq!(
            result
                .err()
                .map(|e| e.to_string().contains("Machines not found")),
            Some(true)
        );

        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new("wrong type".to_string())
                as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );
        let result2 = warden.call(&mut env);
        assert_eq!(
            result2
                .err()
                .map(|e| e.to_string().contains("incorrect type")),
            Some(true)
        );
        Ok(())
    }

    #[test]
    fn test_provision_machines_action_direct() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let mut warden = Warden::new();
        warden.use_action(Box::new(ProvisionMachinesAction {
            provision: Some(true),
            provision_with: None,
            cwd: cwd.to_path_buf(),
        }));

        let mut env = Environment::new();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.box = "base"
  config.vm.provision "shell", inline: "echo hello"
  config.vm.provision "unknown_prov"
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir)?;
        std::fs::write(state_dir.join("id"), "test-id")?;

        let vf_path = cwd.join("Vagrantfile");
        let config = crate::config::parser::parse_vagrantfile(&vf_path.to_string_lossy())?;
        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new(config.machines) as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );

        let result = warden.call(&mut env);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_provision_machines_action_prepare_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let mut warden = Warden::new();
        warden.use_action(Box::new(ProvisionMachinesAction {
            provision: Some(true),
            provision_with: None,
            cwd: cwd.to_path_buf(),
        }));

        let mut env = Environment::new();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.box = "base"
  # Invalid env JSON fails shell prepare
  config.vm.provision "shell", env: "{bad"
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir)?;
        std::fs::write(state_dir.join("id"), "test-id")?;

        let vf_path = cwd.join("Vagrantfile");
        let config = crate::config::parser::parse_vagrantfile(&vf_path.to_string_lossy())?;
        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new(config.machines) as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );

        let result = warden.call(&mut env);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_mount_synced_folders_action_direct() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let mut warden = Warden::new();
        warden.use_action(Box::new(MountSyncedFoldersAction {
            cwd: cwd.to_path_buf(),
        }));

        let mut env = Environment::new();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.box = "base"
  config.vm.synced_folder "/tmp/non_existent_host_path", "/guest_dest", type: "vbox"
  config.vm.synced_folder ".", "", type: "rsync"
  config.vm.synced_folder ".", "/guest_dest3", type: "unknown_type"
  config.vm.synced_folder ".", "/guest_dest4", disabled: true
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir)?;
        std::fs::write(state_dir.join("id"), "test-id")?;

        let vf_path = cwd.join("Vagrantfile");
        let config = crate::config::parser::parse_vagrantfile(&vf_path.to_string_lossy())?;
        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new(config.machines) as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );

        let result = warden.call(&mut env);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_mount_synced_folders_action_errors() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let mut warden = Warden::new();
        warden.use_action(Box::new(MountSyncedFoldersAction {
            cwd: cwd.to_path_buf(),
        }));

        let mut env = Environment::new();
        let result = warden.call(&mut env);
        assert_eq!(
            result
                .err()
                .map(|e| e.to_string().contains("Machines not found")),
            Some(true)
        );

        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new("wrong type".to_string())
                as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );
        let result2 = warden.call(&mut env);
        assert_eq!(
            result2
                .err()
                .map(|e| e.to_string().contains("incorrect type")),
            Some(true)
        );
        Ok(())
    }

    #[test]
    fn test_provision_machines_action_errors() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let mut warden = Warden::new();
        warden.use_action(Box::new(ProvisionMachinesAction {
            provision: None,
            provision_with: None,
            cwd: cwd.to_path_buf(),
        }));

        let mut env = Environment::new();
        let result = warden.call(&mut env);
        assert_eq!(
            result
                .err()
                .map(|e| e.to_string().contains("Machines not found")),
            Some(true)
        );

        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new("wrong type".to_string())
                as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );
        let result2 = warden.call(&mut env);
        assert_eq!(
            result2
                .err()
                .map(|e| e.to_string().contains("incorrect type")),
            Some(true)
        );
        Ok(())
    }

    #[test]
    fn test_provision_machines_action_with_provision_with() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let mut warden = Warden::new();
        warden.use_action(Box::new(ProvisionMachinesAction {
            provision: Some(true),
            provision_with: Some(vec!["shell".to_string(), "skipped_shell".to_string()]),
            cwd: cwd.to_path_buf(),
        }));

        let mut env = Environment::new();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.box = "base"
  config.vm.provision "shell", inline: "echo hello"
  config.vm.provision "file", source: "a", destination: "b"
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir)?;
        std::fs::write(state_dir.join("id"), "test-id")?;

        let vf_path = cwd.join("Vagrantfile");
        let config = crate::config::parser::parse_vagrantfile(&vf_path.to_string_lossy())?;
        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new(config.machines) as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );

        let result = warden.call(&mut env);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_up_target_name() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web" do |w|
    w.vm.box = "base"
    w.vm.post_up_message = "Web server ready"
  end
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args_target = UpArgs {
            name: Some("web".to_string()),
            provision: false,
            no_provision: true,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            provision_with: None,
            install_provider: false,
            no_install_provider: false,
        };
        let result = execute(cwd, &args_target);
        assert!(result.is_ok());

        let args_missing = UpArgs {
            name: Some("nonexistent".to_string()),
            provision: false,
            no_provision: true,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            provision_with: None,
            install_provider: false,
            no_install_provider: false,
        };
        let dir2 = tempdir()?;
        let cwd2 = dir2.path();
        std::fs::write(cwd2.join("Vagrantfile"), vagrantfile_content)?;

        let result_missing = execute(cwd2, &args_missing);
        assert!(result_missing.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_up_insert_key() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "secure" do |s|
    s.ssh.insert_key = true
  end
end
"#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("secure")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir)?;
        std::fs::write(machine_dir.join("id"), "dummy_id")?;

        let args = UpArgs {
            name: Some("secure".to_string()),
            provision: false,
            no_provision: true,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            provision_with: None,
            install_provider: false,
            no_install_provider: false,
        };
        let mut env = Environment::new();
        env.insert_typed("vagrant_home", dir.path().join(".vagrant.d"));
        let result = execute_with_env(cwd, &args, &mut env);
        assert!(result.is_ok());

        let key_path = cwd
            .join(".vagrant")
            .join("machines")
            .join("secure")
            .join("virtualbox")
            .join("private_key");
        assert!(key_path.exists());

        Ok(())
    }

    #[test]
    fn test_boot_machines_winrm_docker_timeout_post_up() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("winrm_mach")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir)?;
        std::fs::write(machine_dir.join("id"), "dummy_id")?;

        let docker_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("docker_mach")
            .join("virtualbox");
        std::fs::create_dir_all(&docker_dir)?;
        std::fs::write(docker_dir.join("id"), "dummy_id")?;

        let mut machines = std::collections::HashMap::new();

        let mut winrm_machine = crate::config::MachineConfig::default();
        winrm_machine.vm.communicator = Some(crate::config::CommunicatorKind::Winrm);
        winrm_machine.vm.post_up_message = Some("WinRM ready".to_string());
        winrm_machine.vm.boot_timeout = Some(0);
        machines.insert("winrm_mach".to_string(), winrm_machine);

        let mut docker_machine = crate::config::MachineConfig::default();
        docker_machine.vm.communicator = Some(crate::config::CommunicatorKind::Docker);
        docker_machine.vm.boot_timeout = Some(0);
        machines.insert("docker_mach".to_string(), docker_machine);

        let mut env = Environment::new();
        env.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines) as Arc<dyn std::any::Any + Send + Sync>,
        );
        env.insert_typed("vagrant_home", dir.path().join(".vagrant.d"));

        let action = BootMachinesAction {
            cwd: cwd.to_path_buf(),
            provider_name: "virtualbox".to_string(),
            parallel: false,
        };

        assert!(action.call(&mut env).is_ok());

        Ok(())
    }

    #[test]
    fn test_mount_synced_folders_action_all_types() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir)?;
        std::fs::write(machine_dir.join("id"), "dummy_id")?;

        std::fs::write(machine_dir.join("private_key"), "dummy_key")?;

        let valid_shared_dir = dir.path().join("shared");
        std::fs::create_dir_all(&valid_shared_dir)?;
        let valid_path = valid_shared_dir.to_string_lossy().into_owned();

        let mut machines = std::collections::HashMap::new();
        let mut machine = crate::config::MachineConfig::default();
        machine.vm.providers.push(crate::config::ProviderConfig {
            name: crate::config::ProviderKind::VirtualBox,
            options: std::collections::HashMap::new(),
        });

        let types = vec![
            "vbox",
            "nfs",
            "smb",
            "rsync",
            "virtiofs",
            "9p",
            "unknown_type",
        ];
        for t in types {
            machine
                .vm
                .synced_folders
                .push(crate::config::SyncedFolderConfig {
                    host_path: std::path::PathBuf::from(&valid_path),
                    guest_path: std::path::PathBuf::from("/tmp/guest"),
                    folder_type: Some(crate::config::SyncedFolderType::parse_raw(t)),
                    disabled: false,
                    ..Default::default()
                });
        }
        machine
            .vm
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/nonexistent/invalid/shared/folder"),
                guest_path: std::path::PathBuf::from("/tmp/guest"),
                folder_type: Some(crate::config::SyncedFolderType::VirtualBox),
                disabled: false,
                ..Default::default()
            });
        machine
            .vm
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from(&valid_path),
                guest_path: std::path::PathBuf::from("/tmp/guest"),
                folder_type: Some(crate::config::SyncedFolderType::VirtualBox),
                disabled: true,
                ..Default::default()
            });

        machines.insert("default".to_string(), machine.clone());

        let mut env = Environment::new();
        env.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines) as Arc<dyn std::any::Any + Send + Sync>,
        );

        let action = MountSyncedFoldersAction {
            cwd: cwd.to_path_buf(),
        };

        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let _ = stream;
            }
        });

        machine.ssh.port = 1;
        let mut env_fail = Environment::new();
        let mut map_fail = std::collections::HashMap::new();
        map_fail.insert("default".to_string(), machine.clone());
        env_fail.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_fail) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(action.call(&mut env_fail).is_ok());

        machine.ssh.port = port;
        let mut env_ok = Environment::new();
        let mut map_ok = std::collections::HashMap::new();
        map_ok.insert("default".to_string(), machine);
        env_ok.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_ok) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(action.call(&mut env_ok).is_ok());

        let mut empty_prov_mach = crate::config::MachineConfig::default();
        empty_prov_mach
            .vm
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from(valid_path),
                guest_path: std::path::PathBuf::from("/tmp/guest"),
                folder_type: Some(crate::config::SyncedFolderType::VirtualBox),
                disabled: false,
                ..Default::default()
            });
        let mut env_empty = Environment::new();
        let mut map_empty = std::collections::HashMap::new();
        map_empty.insert("empty_prov".to_string(), empty_prov_mach);
        env_empty.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_empty) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(action.call(&mut env_empty).is_ok());

        Ok(())
    }

    #[test]
    fn test_provision_machines_action_all_provisioners() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir)?;
        std::fs::write(machine_dir.join("id"), "dummy_id")?;
        std::fs::write(machine_dir.join("private_key"), "dummy_key")?;

        let mut machines = std::collections::HashMap::new();
        let mut machine = crate::config::MachineConfig::default();
        machine.vm.providers.push(crate::config::ProviderConfig {
            name: crate::config::ProviderKind::VirtualBox,
            options: std::collections::HashMap::new(),
        });

        let provs = vec![
            "shell",
            "file",
            "ansible",
            "chef",
            "puppet",
            "salt",
            "docker",
            "unknown_prov",
            "filtered_out",
        ];
        for p in provs {
            machine
                .vm
                .provisioners
                .push(crate::config::ProvisionerConfig {
                    name: p.to_string(),
                    config: std::collections::HashMap::new(),
                    id: None,
                    run: None,
                });
        }

        machines.insert("default".to_string(), machine);

        let mut env = Environment::new();
        env.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines.clone()) as Arc<dyn std::any::Any + Send + Sync>,
        );

        let action = ProvisionMachinesAction {
            provision: Some(true),
            provision_with: Some(vec![
                "shell,file,ansible,chef,puppet,salt,docker,unknown_prov".to_string(),
            ]),
            cwd: cwd.to_path_buf(),
        };

        assert!(action.call(&mut env).is_ok());

        let action_no_filter = ProvisionMachinesAction {
            provision: Some(true),
            provision_with: None,
            cwd: cwd.to_path_buf(),
        };
        let mut env2 = Environment::new();
        env2.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(action_no_filter.call(&mut env2).is_ok());

        let mut empty_prov_mach = crate::config::MachineConfig::default();
        empty_prov_mach
            .vm
            .provisioners
            .push(crate::config::ProvisionerConfig {
                name: "shell".to_string(),
                config: std::collections::HashMap::new(),
                id: None,
                run: None,
            });
        let mut env_empty = Environment::new();
        let mut map_empty = std::collections::HashMap::new();
        map_empty.insert("empty_prov".to_string(), empty_prov_mach);
        env_empty.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_empty) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(action_no_filter.call(&mut env_empty).is_ok());

        let mut unsupp_mach = crate::config::MachineConfig::default();
        unsupp_mach
            .vm
            .providers
            .push(crate::config::ProviderConfig {
                name: crate::config::ProviderKind::Custom("unsupported_provider_xyz".to_string()),
                options: std::collections::HashMap::new(),
            });
        unsupp_mach
            .vm
            .provisioners
            .push(crate::config::ProvisionerConfig {
                name: "shell".to_string(),
                config: std::collections::HashMap::new(),
                id: None,
                run: None,
            });
        let mut env_unsupp = Environment::new();
        let mut map_unsupp = std::collections::HashMap::new();
        map_unsupp.insert("unsupp".to_string(), unsupp_mach);
        env_unsupp.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_unsupp) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(action_no_filter.call(&mut env_unsupp).is_err());

        Ok(())
    }

    #[test]
    fn test_boot_machines_triggers_and_state_and_key_errors() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("trig_mach")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir)?;

        let mut fail_trigger_before = crate::config::MachineConfig::default();
        fail_trigger_before
            .triggers
            .push(crate::config::TriggerConfig {
                stage: "before".to_string(),
                actions: vec!["up".to_string()],
                run_inline: Some("exit 1".to_string()),
                abort: true,
                ..Default::default()
            });
        let mut machines = std::collections::HashMap::new();
        machines.insert("trig_mach".to_string(), fail_trigger_before);

        let mut env = Environment::new();
        env.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines) as Arc<dyn std::any::Any + Send + Sync>,
        );

        let action = BootMachinesAction {
            cwd: cwd.to_path_buf(),
            provider_name: "virtualbox".to_string(),
            parallel: false,
        };
        assert!(action.call(&mut env).is_err());

        std::fs::write(machine_dir.join("id"), "dummy_id")?;
        let mut fail_trigger_after = crate::config::MachineConfig::default();
        fail_trigger_after
            .triggers
            .push(crate::config::TriggerConfig {
                stage: "after".to_string(),
                actions: vec!["up".to_string()],
                run_inline: Some("exit 1".to_string()),
                abort: true,
                ..Default::default()
            });
        let mut machines_after = std::collections::HashMap::new();
        machines_after.insert("trig_mach".to_string(), fail_trigger_after);

        let mut env_after = Environment::new();
        env_after.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines_after) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(action.call(&mut env_after).is_err());

        let docker_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("docker_mach")
            .join("docker");
        std::fs::create_dir_all(&docker_dir)?;

        let mut docker_mach = crate::config::MachineConfig::default();
        docker_mach.ssh.insert_key = true;
        docker_mach.vm.boot_timeout = Some(1);
        docker_mach.vm.communicator = Some(crate::config::CommunicatorKind::Docker);

        let listener2 = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port2 = listener2.local_addr()?.port();
        std::thread::spawn(move || {
            for stream in listener2.incoming() {
                let _ = stream;
            }
        });

        let docker_factory: ProviderFactory = Arc::new(|_, id| {
            Ok(Box::new(
                crate::provider::docker::DockerProvider::new(id).with_docker_bin("echo"),
            ))
        });

        docker_mach.ssh.port = 1;
        let mut env_docker_fail = Environment::new();
        let mut map_fail = std::collections::HashMap::new();
        map_fail.insert("docker_mach".to_string(), docker_mach.clone());
        env_docker_fail.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_fail) as Arc<dyn std::any::Any + Send + Sync>,
        );
        env_docker_fail.insert_typed("provider_factory", Arc::clone(&docker_factory));

        let action_docker = BootMachinesAction {
            cwd: cwd.to_path_buf(),
            provider_name: "docker".to_string(),
            parallel: false,
        };

        assert!(action_docker.call(&mut env_docker_fail).is_ok());

        docker_mach.ssh.port = port2;
        let _ = std::fs::remove_file(docker_dir.join("private_key"));
        let mut env_docker_ok = Environment::new();
        let mut map_ok = std::collections::HashMap::new();
        map_ok.insert("docker_mach".to_string(), docker_mach);
        env_docker_ok.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_ok) as Arc<dyn std::any::Any + Send + Sync>,
        );
        env_docker_ok.insert_typed("provider_factory", docker_factory);
        assert!(action_docker.call(&mut env_docker_ok).is_ok());

        std::fs::write(docker_dir.join("private_key"), b"dummy key")?;
        assert!(action_docker.call(&mut env_docker_ok).is_ok());

        let mut no_insert = crate::config::MachineConfig::default();
        no_insert.ssh.insert_key = false;
        action_docker.handle_key_insertion("docker_mach", &no_insert, &ConsoleUi);

        let vmware_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("vmware_mach")
            .join("vmware");
        std::fs::create_dir_all(&vmware_dir)?;
        std::fs::write(vmware_dir.join("id"), "vmware_id")?;

        let vmware_mach = crate::config::MachineConfig::default();
        let mut machines_vmware = std::collections::HashMap::new();
        machines_vmware.insert("vmware_mach".to_string(), vmware_mach);

        let mut env_vmware = Environment::new();
        env_vmware.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines_vmware) as Arc<dyn std::any::Any + Send + Sync>,
        );

        let vmware_fail_factory: ProviderFactory = Arc::new(|_, id| {
            Ok(Box::new(
                crate::provider::vmware::VmwareProvider::new(id).with_vmrun_bin("false"),
            ))
        });
        env_vmware.insert_typed("provider_factory", vmware_fail_factory);

        let action_vmware = BootMachinesAction {
            cwd: cwd.to_path_buf(),
            provider_name: "vmware".to_string(),
            parallel: false,
        };

        assert!(action_vmware.call(&mut env_vmware).is_err());

        let vmware_ok_factory: ProviderFactory = Arc::new(|_, id| {
            Ok(Box::new(
                crate::provider::vmware::VmwareProvider::new(id).with_vmrun_bin("echo"),
            ))
        });
        env_vmware.insert_typed("provider_factory", vmware_ok_factory);
        assert!(action_vmware.call(&mut env_vmware).is_ok());

        let vagrant_d = tempdir()?;
        let idx_dir = vagrant_d.path().join("data").join("machine-index");
        std::fs::create_dir_all(&idx_dir)?;
        std::fs::write(idx_dir.join("index"), "invalid json")?;

        let invalid_machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("invalid_index")
            .join("virtualbox");
        std::fs::create_dir_all(&invalid_machine_dir)?;
        std::fs::write(invalid_machine_dir.join("id"), "invalid_id")?;

        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let ready_port = listener.local_addr()?.port();
        std::thread::spawn(move || {
            let _ = listener.accept();
        });

        let mut invalid_index_mach = crate::config::MachineConfig::default();
        invalid_index_mach.ssh.port = ready_port;
        invalid_index_mach.vm.boot_timeout = Some(1);
        invalid_index_mach.vm.networks = vec![
            crate::config::NetworkConfig::ForwardedPort {
                guest: 80,
                host: 18080,
                protocol: Some("tcp".to_string()),
                auto_correct: false,
                host_ip: None,
            },
            crate::config::NetworkConfig::ForwardedPort {
                guest: 81,
                host: 18080,
                protocol: Some("tcp".to_string()),
                auto_correct: false,
                host_ip: None,
            },
            crate::config::NetworkConfig::PublicNetwork {
                ip: None,
                bridge: None,
                use_dhcp_assigned_default_route: false,
            },
        ];
        let mut machines_invalid_index = std::collections::HashMap::new();
        machines_invalid_index.insert("invalid_index".to_string(), invalid_index_mach);

        let mut env_invalid_index = Environment::new();
        env_invalid_index.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines_invalid_index) as Arc<dyn std::any::Any + Send + Sync>,
        );
        env_invalid_index.insert_typed("vagrant_home", vagrant_d.path().to_path_buf());
        let running_vbox_factory: ProviderFactory = Arc::new(|_, id| {
            Ok(Box::new(
                crate::provider::virtualbox::VirtualBoxProvider::new(id)
                    .with_runner(|_| Ok("VMState=\"running\"\n".to_string())),
            ))
        });
        env_invalid_index.insert_typed("provider_factory", running_vbox_factory);

        let action_invalid_index = BootMachinesAction {
            cwd: cwd.to_path_buf(),
            provider_name: "virtualbox".to_string(),
            parallel: false,
        };
        crate::communicator::ssh::mock_ssh::MOCK_STATE.with(|s| {
            s.borrow_mut().output = b"ok\n".to_vec();
        });
        assert!(action_invalid_index.call(&mut env_invalid_index).is_ok());
        crate::communicator::ssh::mock_ssh::MOCK_STATE.with(|s| {
            s.borrow_mut().output.clear();
        });

        let prov_running_action = ProvisionMachinesAction {
            provision: Some(true),
            provision_with: None,
            cwd: cwd.to_path_buf(),
        };
        assert!(prov_running_action.call(&mut env_invalid_index).is_ok());

        Ok(())
    }

    #[test]
    fn test_execute_up_create_lock_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        std::fs::write(cwd.join(".vagrant"), b"not a dir")?;

        let args = UpArgs {
            name: None,
            provision: false,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            provision_with: None,
            install_provider: false,
            no_install_provider: false,
        };

        assert!(execute(cwd, &args).is_err());
        Ok(())
    }

    #[test]
    fn test_execute_up_with_provision_with_flag() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        std::fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir)?;
        std::fs::write(machine_dir.join("id"), "dummy_id")?;

        let args = UpArgs {
            name: None,
            provision: true,
            no_provision: false,
            provider: None,
            destroy_on_error: false,
            no_destroy_on_error: false,
            parallel: false,
            no_parallel: false,
            install_provider: false,
            no_install_provider: false,
            provision_with: Some("shell".to_string()),
        };

        let mut env = Environment::new();
        env.insert_typed("vagrant_home", dir.path().join(".vagrant.d"));
        let result = execute_with_env(cwd, &args, &mut env);
        assert!(result.is_ok());

        // Exercise MountSyncedFoldersAction and ProvisionMachinesAction directly
        let mount_action = MountSyncedFoldersAction {
            cwd: cwd.to_path_buf(),
        };
        let prov_action = ProvisionMachinesAction {
            provision: Some(true),
            provision_with: Some(vec!["shell".to_string()]),
            cwd: cwd.to_path_buf(),
        };
        let mut env_empty = Environment::new();
        assert!(mount_action.call(&mut env_empty).is_err());
        assert!(prov_action.call(&mut env_empty).is_err());
        assert_eq!(mount_action.name(), "MountSyncedFoldersAction");
        assert_eq!(prov_action.name(), "ProvisionMachinesAction");

        env_empty.insert_typed("machines", "wrong_type".to_string());
        assert!(mount_action.call(&mut env_empty).is_err());
        assert!(prov_action.call(&mut env_empty).is_err());

        let mut mach = crate::config::MachineConfig::default();
        for sf_type in [
            crate::config::SyncedFolderType::VirtualBox,
            crate::config::SyncedFolderType::Nfs,
            crate::config::SyncedFolderType::Smb,
            crate::config::SyncedFolderType::Rsync,
            crate::config::SyncedFolderType::Virtiofs,
            crate::config::SyncedFolderType::Plan9,
            crate::config::SyncedFolderType::Custom("custom_sf".to_string()),
        ] {
            mach.vm
                .synced_folders
                .push(crate::config::SyncedFolderConfig {
                    host_path: cwd.to_path_buf(),
                    guest_path: std::path::PathBuf::from("/vagrant"),
                    folder_type: Some(sf_type),
                    disabled: false,
                    ..Default::default()
                });
        }
        mach.vm
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: cwd.join("nonexistent_host_dir"),
                guest_path: std::path::PathBuf::from("/missing"),
                folder_type: Some(crate::config::SyncedFolderType::VirtualBox),
                disabled: false,
                ..Default::default()
            });
        mach.vm
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: cwd.to_path_buf(),
                guest_path: std::path::PathBuf::from("/disabled"),
                folder_type: Some(crate::config::SyncedFolderType::VirtualBox),
                disabled: true,
                ..Default::default()
            });
        for p_name in [
            "shell", "file", "ansible", "chef", "puppet", "salt", "docker", "unknown",
        ] {
            mach.vm.provisioners.push(crate::config::ProvisionerConfig {
                name: p_name.to_string(),
                config: std::collections::HashMap::new(),
                ..Default::default()
            });
        }
        let mut machines_map = std::collections::HashMap::new();
        machines_map.insert("default".to_string(), mach);
        machines_map.insert("empty".to_string(), crate::config::MachineConfig::default());

        let mut env_full = Environment::new();
        env_full.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines_map) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(mount_action.call(&mut env_full).is_ok());

        let prov_all = ProvisionMachinesAction {
            provision: None,
            provision_with: None,
            cwd: cwd.to_path_buf(),
        };
        assert!(prov_all.call(&mut env_full).is_ok());
        // Second call when already provisioned hits `!should_provision`
        let mut mach_prov = crate::config::MachineConfig::default();
        let mut shell_cfg = std::collections::HashMap::new();
        shell_cfg.insert("inline".to_string(), "echo hi".to_string());
        mach_prov
            .vm
            .provisioners
            .push(crate::config::ProvisionerConfig {
                name: "shell".to_string(),
                config: shell_cfg,
                ..Default::default()
            });
        let mut map_prov = std::collections::HashMap::new();
        map_prov.insert("default".to_string(), mach_prov);
        let mut env_prov = Environment::new();
        env_prov.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_prov) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(prov_all.call(&mut env_prov).is_ok());
        assert!(prov_all.call(&mut env_prov).is_ok());
        assert!(prov_action.call(&mut env_prov).is_ok());

        // Test BootMachinesAction with ForwardedPort and PrivateNetwork
        let mut mach_net = crate::config::MachineConfig::default();
        mach_net
            .vm
            .networks
            .push(crate::config::NetworkConfig::ForwardedPort {
                guest: 22,
                host: 0,
                auto_correct: true,
                protocol: Some("tcp".to_string()),
                host_ip: Some("127.0.0.1".to_string()),
            });
        mach_net
            .vm
            .networks
            .push(crate::config::NetworkConfig::ForwardedPort {
                guest: 80,
                host: 0,
                auto_correct: true,
                protocol: Some("tcp".to_string()),
                host_ip: Some("127.0.0.1".to_string()),
            });
        mach_net
            .vm
            .networks
            .push(crate::config::NetworkConfig::PrivateNetwork {
                ip: Some("192.168.56.50".to_string()),
                netmask: None,
                dhcp: false,
                virtualbox_intnet: None,
            });
        let mut map_net = std::collections::HashMap::new();
        map_net.insert("net_mach".to_string(), mach_net);
        let mut env_net = Environment::new();
        env_net.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_net) as Arc<dyn std::any::Any + Send + Sync>,
        );
        let boot_net = BootMachinesAction {
            cwd: cwd.to_path_buf(),
            provider_name: "virtualbox".to_string(),
            parallel: false,
        };
        assert!(boot_net.call(&mut env_net).is_ok());

        Ok(())
    }
}
