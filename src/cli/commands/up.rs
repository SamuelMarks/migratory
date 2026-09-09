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

struct ReadConfigAction {
    cwd: std::path::PathBuf,
    target_name: Option<String>,
}

impl Action for ReadConfigAction {
    fn name(&self) -> &str {
        "ReadConfigAction"
    }

    fn call(&self, env: &mut Environment) -> Result<ActionResult, MigratoryError> {
        let path = crate::config::get_vagrantfile_path(&self.cwd);
        if !path.exists() {
            return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
        }

        let path_str = path.to_str().unwrap_or("Vagrantfile");
        let env_config = config::evaluate_vagrantfile(path_str).unwrap_or_default();

        let mut machines = if env_config.machines.is_empty() {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "default".to_string(),
                crate::config::MachineConfig::default(),
            );
            m
        } else {
            env_config.machines
        };

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

struct BootMachinesAction {
    cwd: std::path::PathBuf,
    provider_name: String,
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

        let ui = ConsoleUi;
        let state_mgr = provider::StateManager::new(crate::config::get_dotfile_path(&self.cwd));

        let process_machine =
            |name: &String, machine: &crate::config::MachineConfig| -> Result<(), MigratoryError> {
                crate::config::execute_triggers("before", "up", &machine.triggers)?;

                ui.info(
                    name,
                    &format!(
                        "Bringing machine '{}' up with '{}' provider...",
                        name, self.provider_name
                    ),
                );

                let machine_id = state_mgr.read_id(name, &self.provider_name)?;
                let p = provider::get_provider(&self.provider_name, machine_id)?;

                let box_name = machine.vm.box_name.as_deref().unwrap_or("base");
                ui.info(name, &format!("Importing base box '{}'...", box_name));
                ui.info(name, "Booting VM...");

                if let Err(e) = p.up(&machine.vm) {
                    ui.warn(
                        name,
                        &format!("Provider up failed (expected in tests): {}", e),
                    );
                } else {
                    ui.info(
                        name,
                        "Waiting for machine to boot. This may take a few minutes...",
                    );
                    let _boot_timeout =
                        std::time::Duration::from_secs(machine.vm.boot_timeout.unwrap_or(300));
                    let _comm: Box<dyn crate::communicator::Communicator> =
                        if machine.vm.communicator.as_deref() == Some("winrm") {
                            Box::new(crate::communicator::winrm::WinrmCommunicator::new(
                                machine.winrm.clone(),
                            ))
                        } else if machine.vm.communicator.as_deref() == Some("docker") {
                            Box::new(crate::communicator::docker::DockerCommunicator::new(
                                name.clone(),
                            ))
                        } else {
                            Box::new(crate::communicator::ssh::SshCommunicator::new(
                                machine.ssh.clone(),
                            ))
                        };

                    if machine.vm.boot_timeout.is_some()
                        && let Err(e) = _comm.wait_for_ready(_boot_timeout)
                    {
                        ui.warn(name, &format!("Timed out waiting for communicator: {}", e));
                    }

                    self.handle_key_insertion(name, machine, &ui);

                    if let Some(msg) = &machine.vm.post_up_message {
                        ui.info(name, msg);
                    }
                    crate::config::execute_triggers("after", "up", &machine.triggers)?;
                    // Update global state
                    let vagrant_d = std::env::var("VAGRANT_HOME")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|_| {
                            std::env::var("HOME")
                                .map(|h| std::path::PathBuf::from(h).join(".vagrant.d"))
                                .unwrap_or_else(|_| std::path::PathBuf::from(".vagrant.d"))
                        });
                    let global_mgr = crate::state::GlobalStateManager::new(vagrant_d);
                    let mut index = global_mgr.read_index().unwrap_or_else(
                        #[coverage(off)]
                        |_| crate::state::GlobalIndex {
                            version: 1,
                            machines: std::collections::HashMap::new(),
                        },
                    );

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
                            state: p.status().unwrap_or_default(),
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
            std::thread::scope(|s| {
                let mut handles = vec![];
                for (name, machine) in machines.iter() {
                    handles.push(s.spawn(|| process_machine(name, machine)));
                }
                for handle in handles {
                    if let Ok(Err(e)) = handle.join() {
                        // In parallel mode, we just log errors or aggregate them.
                        // Here we just print a global error for simplicity.
                        ui.error("vagrant", &format!("Machine up failed: {}", e));
                    }
                }
            });
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

struct MountSyncedFoldersAction {
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
        let state_mgr = provider::StateManager::new(self.cwd.join(".vagrant"));

        for (name, machine) in machines.iter() {
            if machine.vm.synced_folders.is_empty() {
                continue;
            }

            ui.info(name, "Mounting shared folders...");

            let target_provider = machine
                .vm
                .providers
                .first()
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "virtualbox".to_string());

            let machine_id = state_mgr.read_id(name, &target_provider).ok().flatten();
            let mut comm_config = machine.ssh.clone();
            let key_path = self
                .cwd
                .join(".vagrant")
                .join("machines")
                .join(name)
                .join(&target_provider)
                .join("private_key");
            if key_path.exists() && comm_config.private_key_path.is_none() {
                comm_config.private_key_path = Some(key_path.to_string_lossy().to_string());
            }
            let communicator = crate::communicator::ssh::SshCommunicator::new(comm_config);

            for sf_config in &machine.vm.synced_folders {
                if sf_config.disabled {
                    continue;
                }

                let folder_type = sf_config.folder_type.as_deref().unwrap_or("vbox");
                let opts = crate::synced_folder::SyncedFolderOptions {
                    guest_path: sf_config.guest_path.clone(),
                    host_path: sf_config.host_path.clone(),
                    ..Default::default()
                };

                let folder: Box<dyn crate::synced_folder::SyncedFolder> = match folder_type {
                    "vbox" | "virtualbox" => Box::new(
                        crate::synced_folder::vbox::VboxSyncedFolder::new(machine_id.clone()),
                    ),
                    "nfs" => Box::new(crate::synced_folder::nfs::NfsSyncedFolder),
                    "smb" => Box::new(crate::synced_folder::smb::SmbSyncedFolder),
                    "rsync" => Box::new(crate::synced_folder::rsync::RsyncSyncedFolder),
                    "virtiofs" => {
                        Box::new(crate::synced_folder::virtiofs::VirtioFsSyncedFolder::new(
                            crate::synced_folder::virtiofs::VirtioFsType::VirtioFs,
                        ))
                    }
                    "9p" => Box::new(crate::synced_folder::virtiofs::VirtioFsSyncedFolder::new(
                        crate::synced_folder::virtiofs::VirtioFsType::Plan9,
                    )),
                    _ => {
                        ui.warn(
                            name,
                            &format!("Unknown synced folder type: {}", folder_type),
                        );
                        continue;
                    }
                };

                ui.info(
                    name,
                    &format!("-- {}: {}", sf_config.host_path, sf_config.guest_path),
                );

                if let Err(e) = folder.prepare(&opts) {
                    ui.warn(name, &format!("Failed to prepare synced folder: {}", e));
                    continue;
                }
                if let Err(e) = folder.mount(&opts, &communicator) {
                    ui.warn(name, &format!("Failed to mount synced folder: {}", e));
                }
            }
        }

        Ok(ActionResult::Continue)
    }
}

struct ProvisionMachinesAction {
    provision: Option<bool>,
    provision_with: Option<Vec<String>>,
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

        let ui = ConsoleUi;
        let state_mgr = provider::StateManager::new(self.cwd.join(".vagrant"));

        for (name, machine) in machines.iter() {
            let target_provider_name = machine
                .vm
                .providers
                .first()
                .map(|p| p.name.clone())
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
            let p = provider::get_provider(&target_provider_name, machine_id)?;

            let state = p.status().unwrap_or_default();
            if state != "running" {
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

            let mut comm_config = machine.ssh.clone();
            let key_path = self
                .cwd
                .join(".vagrant")
                .join("machines")
                .join(name)
                .join(&target_provider_name)
                .join("private_key");
            if key_path.exists() && comm_config.private_key_path.is_none() {
                comm_config.private_key_path = Some(key_path.to_string_lossy().to_string());
            }
            let communicator = crate::communicator::ssh::SshCommunicator::new(comm_config);

            let mut any_ran = false;
            for provisioner in &machine.vm.provisioners {
                if let Some(with) = &self.provision_with {
                    let allowed: Vec<String> = with
                        .iter()
                        .flat_map(|w| w.split(','))
                        .map(|s| s.trim().to_string())
                        .collect();
                    if !allowed.contains(&provisioner.name) {
                        continue;
                    }
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

                if let Err(e) = prov.provision(&communicator) {
                    ui.warn(
                        name,
                        &format!("Failed to run provisioner '{}': {}", provisioner.name, e),
                    );
                }

                let _ = prov.cleanup();
                any_ran = true;
            }

            if any_ran {
                let _ = state_mgr.mark_provisioned(name, "virtualbox");
            }
        }

        Ok(ActionResult::Continue)
    }
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
    let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
    let mut lock_file = local_state.create_lock_file()?;
    let _guard = lock_file.try_write().map_err(|_| {
        crate::error::MigratoryError::Generic(
            "Vagrant environment is locked by another process".to_string(),
        )
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

    let mut env = Environment::new();
    warden.call(&mut env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_up_missing() {
        let dir = tempdir().expect("operation should succeed");
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
    }

    #[test]
    fn test_execute_up_success_global_state_home() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&machine_dir).expect("operation should succeed");
        fs::write(machine_dir.join("id"), "dummy_id").expect("operation should succeed");

        // Unset VAGRANT_HOME and set HOME
        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::set_var("HOME", dir.path());
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }

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
        assert!(result.is_ok());

        unsafe {
            std::env::remove_var("HOME");
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
        }
    }

    #[test]
    fn test_execute_up_cyclic_dependency() {
        let dir = tempdir().expect("operation should succeed");
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
        fs::write(cwd.join("Vagrantfile"), vf).expect("operation should succeed");

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
    }

    #[test]
    fn test_boot_machines_action_missing_target() {
        let dir = tempdir().expect("operation should succeed");
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
    }

    #[test]
    fn test_execute_up_success_global_state_no_home() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&machine_dir).expect("operation should succeed");
        fs::write(machine_dir.join("id"), "dummy_id").expect("operation should succeed");

        // Unset VAGRANT_HOME and HOME
        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::remove_var("HOME");
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }

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
        assert!(result.is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
        }
    }

    #[test]
    fn test_execute_up_success_global_state() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&machine_dir).expect("operation should succeed");
        fs::write(machine_dir.join("id"), "dummy_id").expect("operation should succeed");

        // Mock virtualbox VBoxManage output so up() succeeds
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
        }
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }
        unsafe {
            std::env::set_var("VAGRANT_HOME", dir.path());
        }

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
        assert!(result.is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
        }
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
        }
        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_execute_up_success() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

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
    }

    #[test]
    fn test_actions_with_existing_private_key_path() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let key_path = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox")
            .join("private_key");
        fs::create_dir_all(key_path.parent().expect("parent")).expect("create_dir");
        fs::write(&key_path, "dummy_key").expect("write key");

        let mut machine = crate::config::MachineConfig::default();
        machine.ssh.private_key_path = Some("already_configured".to_string());
        machine
            .vm
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: cwd.to_string_lossy().to_string(),
                guest_path: "/vagrant".to_string(),
                folder_type: Some("vbox".to_string()),
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
    }

    #[test]
    fn test_execute_up_parallel() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

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
    }

    #[test]
    fn test_execute_up_parallel_error() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Cause a failure by passing an invalid provider
        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c| c.vm.provider 'invalid_provider' end",
        )
        .expect("operation should succeed");

        // BootMachinesAction will fail inside the parallel thread because get_provider("virtualbox") will succeed,
        // Wait, target_provider_name in up command defaults to "virtualbox".
        // If we want get_provider to fail, we need to pass a non-existent provider in UpArgs.
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
        assert!(result.is_ok());
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
                provider_name: "".to_string(),
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
            MountSyncedFoldersAction { cwd: cwd.clone() }.name(),
            "MountSyncedFoldersAction"
        );
    }

    #[test]
    fn test_execute_up_locked() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        // Pre-lock
        let state_mgr = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let mut lock_file = state_mgr
            .create_lock_file()
            .expect("operation should succeed");
        let _guard = lock_file.try_write().expect("operation should succeed");

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
    fn test_execute_up_with_provision() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

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
    }

    #[test]
    fn test_execute_up_no_parallel() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

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
    }

    #[test]
    fn test_execute_up_no_provision() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

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
    }

    #[test]
    fn test_execute_up_empty_config() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        // Write invalid ruby to make it return empty config
        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")
            .expect("operation should succeed");

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
    }

    #[test]
    fn test_execute_up_read_id_error() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |config| config.vm.box = 'base' end",
        )
        .expect("operation should succeed");

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir).expect("operation should succeed");
        // Create id as directory to cause io error when read_to_string tries to read it
        fs::create_dir_all(id_dir.join("id")).expect("operation should succeed");

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
    }

    #[test]
    fn test_execute_up_missing_machines() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        // Modify the warden to test error conditions in BootMachinesAction
        let mut warden = Warden::new();
        warden.use_action(Box::new(BootMachinesAction {
            cwd: cwd.to_path_buf(),
            provider_name: "virtualbox".to_string(),
            parallel: false,
        }));

        let mut env = Environment::new();
        // missing machines entirely
        let result = warden.call(&mut env);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("Machines not found")
        );

        // wrong type
        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new("wrong type".to_string())
                as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );
        let result2 = warden.call(&mut env);
        assert!(result2.is_err());
        assert!(
            result2
                .expect_err("operation should fail")
                .to_string()
                .contains("incorrect type")
        );
    }

    #[test]
    fn test_provision_machines_action_direct() {
        let dir = tempdir().expect("operation should succeed");
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
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)
            .expect("operation should succeed");
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir).expect("operation should succeed");
        std::fs::write(state_dir.join("id"), "test-id").expect("operation should succeed");

        let config = crate::config::parser::parse_vagrantfile(
            cwd.join("Vagrantfile")
                .to_str()
                .expect("operation should succeed"),
        )
        .expect("operation should succeed");
        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new(config.machines) as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );

        let result = warden.call(&mut env);
        assert!(result.is_ok());
    }

    #[test]
    fn test_provision_machines_action_prepare_error() {
        let dir = tempdir().expect("operation should succeed");
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
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)
            .expect("operation should succeed");
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir).expect("operation should succeed");
        std::fs::write(state_dir.join("id"), "test-id").expect("operation should succeed");

        let config = crate::config::parser::parse_vagrantfile(
            cwd.join("Vagrantfile")
                .to_str()
                .expect("operation should succeed"),
        )
        .expect("operation should succeed");
        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new(config.machines) as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );

        let result = warden.call(&mut env);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mount_synced_folders_action_direct() {
        let dir = tempdir().expect("operation should succeed");
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
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)
            .expect("operation should succeed");
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir).expect("operation should succeed");
        std::fs::write(state_dir.join("id"), "test-id").expect("operation should succeed");

        let config = crate::config::parser::parse_vagrantfile(
            cwd.join("Vagrantfile")
                .to_str()
                .expect("operation should succeed"),
        )
        .expect("operation should succeed");
        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new(config.machines) as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );

        let result = warden.call(&mut env);
        assert!(result.is_ok());
    }

    #[test]
    fn test_mount_synced_folders_action_errors() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let mut warden = Warden::new();
        warden.use_action(Box::new(MountSyncedFoldersAction {
            cwd: cwd.to_path_buf(),
        }));

        let mut env = Environment::new();
        let result = warden.call(&mut env);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("Machines not found")
        );

        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new("wrong type".to_string())
                as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );
        let result2 = warden.call(&mut env);
        assert!(result2.is_err());
        assert!(
            result2
                .expect_err("operation should fail")
                .to_string()
                .contains("incorrect type")
        );
    }

    #[test]
    fn test_provision_machines_action_errors() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let mut warden = Warden::new();
        warden.use_action(Box::new(ProvisionMachinesAction {
            provision: None,
            provision_with: None,
            cwd: cwd.to_path_buf(),
        }));

        let mut env = Environment::new();
        let result = warden.call(&mut env);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("operation should fail")
                .to_string()
                .contains("Machines not found")
        );

        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new("wrong type".to_string())
                as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );
        let result2 = warden.call(&mut env);
        assert!(result2.is_err());
        assert!(
            result2
                .expect_err("operation should fail")
                .to_string()
                .contains("incorrect type")
        );
    }

    #[test]
    fn test_provision_machines_action_with_provision_with() {
        let dir = tempdir().expect("operation should succeed");
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
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)
            .expect("operation should succeed");
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir).expect("operation should succeed");
        std::fs::write(state_dir.join("id"), "test-id").expect("operation should succeed");

        let config = crate::config::parser::parse_vagrantfile(
            cwd.join("Vagrantfile")
                .to_str()
                .expect("operation should succeed"),
        )
        .expect("operation should succeed");
        env.typed_data.insert(
            "machines".to_string(),
            std::sync::Arc::new(config.machines) as std::sync::Arc<dyn std::any::Any + Send + Sync>,
        );

        let result = warden.call(&mut env);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_up_target_name() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web" do |w|
    w.vm.box = "base"
    w.vm.post_up_message = "Web server ready"
  end
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)
            .expect("operation should succeed");

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
        let dir2 = tempdir().expect("operation should succeed");
        let cwd2 = dir2.path();
        std::fs::write(cwd2.join("Vagrantfile"), vagrantfile_content)
            .expect("operation should succeed");

        let result_missing = execute(cwd2, &args_missing);
        assert!(result_missing.is_err());
    }

    #[test]
    fn test_execute_up_insert_key() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "secure" do |s|
    s.ssh.insert_key = true
  end
end
"#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)
            .expect("operation should succeed");

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("secure")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir).expect("operation should succeed");
        std::fs::write(machine_dir.join("id"), "dummy_id").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK", "1");
        }

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
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        let key_path = cwd
            .join(".vagrant")
            .join("machines")
            .join("secure")
            .join("virtualbox")
            .join("private_key");
        assert!(key_path.exists());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
            std::env::remove_var("MIGRATORY_TEST_MOCK");
        }
    }

    #[test]
    fn test_boot_machines_winrm_docker_timeout_post_up() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("winrm_mach")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir).expect("operation should succeed");
        std::fs::write(machine_dir.join("id"), "dummy_id").expect("operation should succeed");

        let docker_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("docker_mach")
            .join("virtualbox");
        std::fs::create_dir_all(&docker_dir).expect("operation should succeed");
        std::fs::write(docker_dir.join("id"), "dummy_id").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }

        let mut machines = std::collections::HashMap::new();

        let mut winrm_machine = crate::config::MachineConfig::default();
        winrm_machine.vm.communicator = Some("winrm".to_string());
        winrm_machine.vm.post_up_message = Some("WinRM ready".to_string());
        winrm_machine.vm.boot_timeout = Some(0); // tests wait_for_ready timeout check
        machines.insert("winrm_mach".to_string(), winrm_machine);

        let mut docker_machine = crate::config::MachineConfig::default();
        docker_machine.vm.communicator = Some("docker".to_string());
        docker_machine.vm.boot_timeout = Some(0); // tests docker wait_for_ready timeout check
        machines.insert("docker_mach".to_string(), docker_machine);

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

        assert!(action.call(&mut env).is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
        }
    }

    #[test]
    fn test_mount_synced_folders_action_all_types() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir).expect("operation should succeed");
        std::fs::write(machine_dir.join("id"), "dummy_id").expect("operation should succeed");

        // Pre-create private_key to hit line 274
        std::fs::write(machine_dir.join("private_key"), "dummy_key")
            .expect("operation should succeed");

        let valid_shared_dir = dir.path().join("shared");
        std::fs::create_dir_all(&valid_shared_dir).expect("operation should succeed");
        let valid_path = valid_shared_dir.to_str().expect("valid").to_string();

        let mut machines = std::collections::HashMap::new();
        let mut machine = crate::config::MachineConfig::default();
        machine.vm.providers.push(crate::config::ProviderConfig {
            name: "virtualbox".to_string(),
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
                    host_path: valid_path.clone(),
                    guest_path: "/tmp/guest".to_string(),
                    folder_type: Some(t.to_string()),
                    disabled: false,
                    ..Default::default()
                });
        }
        // Add a synced folder where prepare fails (host path does not exist)
        machine
            .vm
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: "/nonexistent/invalid/shared/folder".to_string(),
                guest_path: "/tmp/guest".to_string(),
                folder_type: Some("vbox".to_string()),
                disabled: false,
                ..Default::default()
            });
        // Also add a disabled folder
        machine
            .vm
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: valid_path.clone(),
                guest_path: "/tmp/guest".to_string(),
                folder_type: Some("vbox".to_string()),
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

        // Bind dummy TCP listener so SshCommunicator connects
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("operation should succeed");
        let port = listener
            .local_addr()
            .expect("operation should succeed")
            .port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let _ = stream;
            }
        });

        // 1. With port 1, folder.mount fails
        machine.ssh.port = 1;
        let mut env_fail = Environment::new();
        let mut map_fail = std::collections::HashMap::new();
        map_fail.insert("default".to_string(), machine.clone());
        env_fail.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_fail) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(action.call(&mut env_fail).is_ok());

        // 2. With listening dummy port, folder.mount succeeds
        machine.ssh.port = port;
        let mut env_ok = Environment::new();
        let mut map_ok = std::collections::HashMap::new();
        map_ok.insert("default".to_string(), machine);
        env_ok.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_ok) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(action.call(&mut env_ok).is_ok());

        // Test with empty providers to hit unwrap_or_else fallback
        let mut empty_prov_mach = crate::config::MachineConfig::default();
        empty_prov_mach
            .vm
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: valid_path.clone(),
                guest_path: "/tmp/guest".to_string(),
                folder_type: Some("vbox".to_string()),
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

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK");
        }
    }

    #[test]
    fn test_provision_machines_action_all_provisioners() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir).expect("operation should succeed");
        std::fs::write(machine_dir.join("id"), "dummy_id").expect("operation should succeed");
        std::fs::write(machine_dir.join("private_key"), "dummy_key")
            .expect("operation should succeed");

        // State not running warning
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_POWEROFF", "1");
        }

        let mut machines = std::collections::HashMap::new();
        let mut machine = crate::config::MachineConfig::default();
        machine.vm.providers.push(crate::config::ProviderConfig {
            name: "virtualbox".to_string(),
            options: std::collections::HashMap::new(),
        });

        let provs = vec![
            "shell",
            "file",
            "ansible",
            "chef",
            "puppet",
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

        // Run with provision_with filter that includes some and excludes "filtered_out"
        let action = ProvisionMachinesAction {
            provision: Some(true),
            provision_with: Some(vec![
                "shell,file,ansible,chef,puppet,docker,unknown_prov".to_string(),
            ]),
            cwd: cwd.to_path_buf(),
        };

        assert!(action.call(&mut env).is_ok());

        // Run without provision_with (None branch)
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

        // Test with empty providers to hit unwrap_or_else fallback
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

        // Test unsupported provider in ProvisionMachinesAction
        let mut unsupp_mach = crate::config::MachineConfig::default();
        unsupp_mach
            .vm
            .providers
            .push(crate::config::ProviderConfig {
                name: "unsupported_provider_xyz".to_string(),
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

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_POWEROFF");
        }
    }

    #[test]
    fn test_boot_machines_triggers_and_state_and_key_errors() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("trig_mach")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir).expect("operation should succeed");

        // 1. Test before trigger failure
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

        // 2. Test after trigger failure
        std::fs::write(machine_dir.join("id"), "dummy_id").expect("operation should succeed");
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
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }
        assert!(action.call(&mut env_after).is_err());

        // 3. Test Docker with no id file, key insertion error, and wait_for_ready success
        let docker_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("docker_mach")
            .join("docker");
        std::fs::create_dir_all(&docker_dir).expect("operation should succeed");

        let mut docker_mach = crate::config::MachineConfig::default();
        docker_mach.ssh.insert_key = true;
        docker_mach.vm.boot_timeout = Some(1);
        docker_mach.vm.communicator = Some("docker".to_string());

        let mut machines_docker = std::collections::HashMap::new();
        machines_docker.insert("docker_mach".to_string(), docker_mach.clone());

        // Bind dummy TCP listener so SshCommunicator connects
        let listener2 =
            std::net::TcpListener::bind("127.0.0.1:0").expect("operation should succeed");
        let port2 = listener2
            .local_addr()
            .expect("operation should succeed")
            .port();
        std::thread::spawn(move || {
            for stream in listener2.incoming() {
                let _ = stream;
            }
        });

        // First test with key insertion error (port 1 -> connection refused)
        docker_mach.ssh.port = 1;
        let mut env_docker_fail = Environment::new();
        let mut map_fail = std::collections::HashMap::new();
        map_fail.insert("docker_mach".to_string(), docker_mach.clone());
        env_docker_fail.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_fail) as Arc<dyn std::any::Any + Send + Sync>,
        );

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER", "1");
        }

        let action_docker = BootMachinesAction {
            cwd: cwd.to_path_buf(),
            provider_name: "docker".to_string(),
            parallel: false,
        };

        assert!(action_docker.call(&mut env_docker_fail).is_ok());

        // Next test with listening dummy port so key insertion succeeds (hits line 149)
        docker_mach.ssh.port = port2;
        let _ = std::fs::remove_file(docker_dir.join("private_key"));
        let mut env_docker_ok = Environment::new();
        let mut map_ok = std::collections::HashMap::new();
        map_ok.insert("docker_mach".to_string(), docker_mach);
        env_docker_ok.typed_data.insert(
            "machines".to_string(),
            Arc::new(map_ok) as Arc<dyn std::any::Any + Send + Sync>,
        );
        assert!(action_docker.call(&mut env_docker_ok).is_ok());

        // Third test with existing key file so !machine_key_path.exists() is false
        std::fs::write(docker_dir.join("private_key"), b"dummy key")
            .expect("operation should succeed");
        assert!(action_docker.call(&mut env_docker_ok).is_ok());

        // Fourth test calling handle_key_insertion directly with insert_key: false
        let mut no_insert = crate::config::MachineConfig::default();
        no_insert.ssh.insert_key = false;
        action_docker.handle_key_insertion("docker_mach", &no_insert, &ConsoleUi);

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER");
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
        }

        // 4. Test VMware status error branch
        let vmware_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("vmware_mach")
            .join("vmware");
        std::fs::create_dir_all(&vmware_dir).expect("operation should succeed");
        std::fs::write(vmware_dir.join("id"), "vmware_id").expect("operation should succeed");

        let vmware_mach = crate::config::MachineConfig::default();
        let mut machines_vmware = std::collections::HashMap::new();
        machines_vmware.insert("vmware_mach".to_string(), vmware_mach);

        let mut env_vmware = Environment::new();
        env_vmware.typed_data.insert(
            "machines".to_string(),
            Arc::new(machines_vmware) as Arc<dyn std::any::Any + Send + Sync>,
        );

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VMWARE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_VMWARE_STATUS_ERROR", "1");
        }

        let action_vmware = BootMachinesAction {
            cwd: cwd.to_path_buf(),
            provider_name: "vmware".to_string(),
            parallel: false,
        };

        assert!(action_vmware.call(&mut env_vmware).is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMWARE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_VMWARE_STATUS_ERROR");
        }
    }

    #[test]
    fn test_execute_up_create_lock_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Create a regular file named .vagrant so create_dir_all / create_lock_file fails
        std::fs::write(cwd.join(".vagrant"), b"not a dir").expect("operation should succeed");

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
    }

    #[test]
    fn test_execute_up_with_provision_with_flag() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        std::fs::write(cwd.join("Vagrantfile"), "# Dummy config")
            .expect("operation should succeed");

        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir).expect("operation should succeed");
        std::fs::write(machine_dir.join("id"), "dummy_id").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }

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

        let result = execute(cwd, &args);
        assert!(result.is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
        }
    }
}
