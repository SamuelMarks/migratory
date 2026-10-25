//! Semantic implementation of the `provision` command.
//!
//! This module provides the logic to manually run configured provisioners
//! against one or more target machines in the environment.

use crate::cli::ProvisionArgs;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use crate::ui::{ConsoleUi, Ui};
use std::path::Path;

/// Looks up a machine configuration by name from the parsed machine map.
///
/// # Arguments
///
/// * `machines` - Map of machine names to [`crate::config::MachineConfig`].
/// * `name` - Target machine name to look up.
///
/// # Returns
///
/// Returns a reference to the [`crate::config::MachineConfig`].
///
/// # Errors
///
/// Returns [`MigratoryError::NotFound`] if the machine name does not exist in `machines`.
fn get_machine<'a>(
    machines: &'a std::collections::HashMap<String, crate::config::MachineConfig>,
    name: &str,
) -> Result<&'a crate::config::MachineConfig, MigratoryError> {
    machines
        .get(name)
        .ok_or_else(|| MigratoryError::NotFound(format!("Machine '{name}' not found")))
}

/// Executes the `provision` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `provision` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if the environment is locked, the `Vagrantfile` cannot be found,
/// dependency resolution fails, or triggers fail.
pub fn execute(cwd: &Path, args: &ProvisionArgs) -> Result<(), MigratoryError> {
    let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
    let mut lock_file = local_state.create_lock_file()?;
    let _guard = lock_file
        .try_write()
        .map_err(|_| MigratoryError::EnvironmentLocked {
            path: cwd.join(".vagrant"),
        })?;
    let ui = ConsoleUi;
    let env_config = config::load_environment_config(cwd)?;
    let machines = env_config.machines;

    let target_names = config::resolve_target_machines(&machines, args.name.as_deref())?;
    let target_names = config::sort_machines_by_dependencies(&target_names, &machines, false)?;
    crate::action::MachineOrchestrator::run(&target_names, args.parallel, |name| {
        let machine = get_machine(&machines, name)?;
        crate::config::execute_triggers("before", "provision", &machine.triggers)?;
        let state_mgr = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let target_provider_name = machine
            .vm
            .providers
            .first()
            .map_or("virtualbox", |p| p.name.as_str());

        let machine_id = state_mgr.read_id(name, target_provider_name)?;
        let p = provider::get_provider(target_provider_name, machine_id)?;

        let state = p.status().unwrap_or(crate::provider::MachineState::Unknown(
            "unknown".to_string(),
        ));
        if state != crate::provider::MachineState::Running {
            ui.warn(
                name,
                "VM is not running. Provisioners may fail or be skipped.",
            );
        }

        if let Some(with) = &args.provision_with {
            ui.info(name, &format!("Running provisioners: {with}..."));
        } else {
            ui.info(name, "Running provisioners...");
        }

        let communicator = crate::communicator::ssh::SshCommunicator::new(machine.ssh.clone());
        let mut any_ran = false;
        for provisioner in &machine.vm.provisioners {
            if let Some(with) = &args.provision_with
                && !with
                    .split(',')
                    .map(str::trim)
                    .any(|x| x == provisioner.name.as_str() || provisioner.id.as_deref() == Some(x))
            {
                continue;
            }

            ui.info(
                name,
                &format!("Running provisioner: {}...", provisioner.name),
            );
            let mut prov: Box<dyn crate::provisioner::Provisioner> = match provisioner.name.as_str()
            {
                "shell" => Box::new(crate::provisioner::shell::ShellProvisioner::new()),
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

            try_provision(
                &mut *prov,
                &provisioner.config,
                &communicator,
                name,
                &ui,
                &provisioner.name,
            )?;
            any_ran = true;
        }

        if any_ran {
            let _ = state_mgr.mark_provisioned(name, target_provider_name);
        }
        crate::config::execute_triggers("after", "provision", &machine.triggers)?;
        Ok(())
    })?;

    Ok(())
}

/// Prepares, executes, and cleans up a single provisioner instance against a communicator.
///
/// # Arguments
///
/// * `prov` - Mutable reference to the provisioner implementation.
/// * `config` - Key-value configuration map for the provisioner.
/// * `communicator` - Active guest communicator transport.
/// * `name` - Target machine name for UI logging.
/// * `ui` - Console UI instance for status and warning messages.
/// * `provisioner_name` - Name of the provisioner being executed.
///
/// # Returns
///
/// Returns `Ok(())` if preparation and execution succeeded.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if preparation fails, or if provisioning fails when not in a mock/dry-run test context.
fn try_provision(
    prov: &mut dyn crate::provisioner::Provisioner,
    config: &std::collections::HashMap<String, String>,
    communicator: &dyn crate::communicator::Communicator,
    name: &str,
    ui: &crate::ui::ConsoleUi,
    provisioner_name: &str,
) -> Result<(), MigratoryError> {
    if let Err(e) = prov.prepare(config) {
        ui.warn(
            name,
            &format!("Failed to prepare provisioner '{provisioner_name}': {e}"),
        );
        return Err(e);
    }

    if let Err(e) = prov.provision(communicator) {
        ui.warn(
            name,
            &format!("Failed to run provisioner '{provisioner_name}': {e}"),
        );
        let _ = prov.cleanup();
        return Err(e);
    }

    let _ = prov.cleanup();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_get_machine_not_found() {
        let machines = std::collections::HashMap::new();
        assert!(matches!(
            get_machine(&machines, "missing"),
            Err(MigratoryError::NotFound(_))
        ));
    }

    #[test]
    #[cfg(unix)]
    fn test_execute_invalid_utf8_path() -> Result<(), MigratoryError> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let dir = tempfile::tempdir()?;
        let mut path = dir.path().to_path_buf();
        let invalid_utf8 = vec![0xFF, 0xFF, 0xFF];
        path.push(OsString::from_vec(invalid_utf8));

        let args = ProvisionArgs::default();
        let res = super::execute(&path, &args);
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_provision_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "mock_id")?;

        let result = execute(
            cwd,
            &ProvisionArgs {
                provision_with: None,
                name: None,
                parallel: false,
                no_parallel: false,
            },
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_provision_cyclic_dependency() -> Result<(), MigratoryError> {
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
        std::fs::write(cwd.join("Vagrantfile"), vf)?;

        let result = execute(
            cwd,
            &ProvisionArgs {
                provision_with: None,
                name: None,
                parallel: false,
                no_parallel: false,
            },
        );
        assert!(matches!(result, Err(MigratoryError::Validation(_))));
        Ok(())
    }

    #[test]
    fn test_execute_provision_before_trigger_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vf = r#"
Vagrant.configure("2") do |config|
  config.trigger.before :provision, run: { inline: "exit 1" }, on_error: "halt"
end
"#;
        std::fs::write(cwd.join("Vagrantfile"), vf)?;

        let result = execute(
            cwd,
            &ProvisionArgs {
                provision_with: None,
                name: None,
                parallel: false,
                no_parallel: false,
            },
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_provision_after_trigger_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vf = r#"
Vagrant.configure("2") do |config|
  config.trigger.after :provision, run: { inline: "exit 1" }, on_error: "halt"
end
"#;
        std::fs::write(cwd.join("Vagrantfile"), vf)?;

        let result = execute(
            cwd,
            &ProvisionArgs {
                provision_with: None,
                name: None,
                parallel: false,
                no_parallel: false,
            },
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_provision_lock_errors() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        std::fs::write(cwd.join("Vagrantfile"), "")?;

        let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let mut lock_file = local_state.create_lock_file()?;
        let active_lock = lock_file.try_write()?;

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let res = execute(cwd, &args);
        assert!(res.is_err());

        drop(active_lock);

        let dir2 = tempdir()?;
        let cwd2 = dir2.path();
        std::fs::write(cwd2.join(".vagrant"), "not a dir")?;
        assert!(execute(cwd2, &args).is_err());
        Ok(())
    }

    #[test]
    fn test_execute_provision_read_id_and_unsupported_provider() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        std::fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|
end",
        )?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox")
            .join("id");
        std::fs::create_dir_all(&id_dir)?;

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        assert!(execute(cwd, &args).is_err());

        let dir2 = tempdir()?;
        let cwd2 = dir2.path();
        std::fs::write(
            cwd2.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|
  config.vm.provider 'unsupported_prov'
end",
        )?;
        let state_dir2 = cwd2
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("unsupported_prov");
        std::fs::create_dir_all(&state_dir2)?;
        std::fs::write(state_dir2.join("id"), "123")?;
        assert!(execute(cwd2, &args).is_err());
        Ok(())
    }

    #[test]
    fn test_execute_provision_empty_machines() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "mock_id")?;
        std::fs::write(cwd.join("Vagrantfile"), "")?;

        let result = execute(
            cwd,
            &ProvisionArgs {
                provision_with: None,
                name: None,
                parallel: false,
                no_parallel: false,
            },
        );
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_provision_vm_not_running() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "mock_machine_id")?;

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.vm.provider "virtualbox" do |v|
    end
  end
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_provision_success_no_args() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let playbook_path = cwd.join("playbook.yml");
        fs::write(&playbook_path, "dummy")?;
        let abs_playbook = playbook_path.to_string_lossy().to_string();
        let src_file = cwd.join("foo");
        fs::write(&src_file, "file content")?;
        let abs_src = src_file.to_string_lossy().to_string();

        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        std::thread::spawn(move || while listener.accept().is_ok() {});

        fs::write(
            cwd.join("Vagrantfile"),
            format!(
                r#"
Vagrant.configure("2") do |config|
  config.ssh.port = {port}
  config.vm.provision "shell", inline: "echo 'hello'"
  config.vm.provision "file", source: "{abs_src}", destination: "bar"
  config.vm.provision "ansible", playbook: "{abs_playbook}", mode: "guest"
  config.vm.provision "chef", run_list: "recipe[foo]"
  config.vm.provision "puppet", puppet_server: "puppet.example.com"
  config.vm.provision "salt", masterless: "true"
  config.vm.provision "docker", images: "ubuntu"
  config.vm.provision "unknown_prov"
end
        "#
            ),
        )?;

        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "12345")?;

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_provision_target_machine_not_found() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        std::fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|
end",
        )?;

        let args = ProvisionArgs {
            provision_with: None,
            name: Some("nonexistent_machine".to_string()),
            parallel: false,
            no_parallel: false,
        };
        assert!(execute(cwd, &args).is_err());
        Ok(())
    }

    #[test]
    fn test_execute_provision_success_with_args() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "mock_id")?;

        let src_file = cwd.join("foo");
        fs::write(&src_file, "file content")?;
        let abs_src = src_file.to_string_lossy().to_string();

        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        std::thread::spawn(move || while listener.accept().is_ok() {});

        fs::write(
            cwd.join("Vagrantfile"),
            format!(
                r#"
Vagrant.configure("2") do |config|
  config.ssh.port = {port}
  config.vm.provision "shell", inline: "echo 'hello'"
  config.vm.provision "file", source: "{abs_src}", destination: "bar"
end
        "#
            ),
        )?;

        let args = ProvisionArgs {
            provision_with: Some("shell,file".to_string()),
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_provision_prepare_and_provision_failures() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "mock_id")?;

        fs::write(
            cwd.join("Vagrantfile"),
            r#"
    Vagrant.configure("2") do |config|
    config.vm.provision "shell", env: "not a valid json"
    end
    "#,
        )?;

        let state_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "12345")?;

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        assert!(execute(cwd, &args).is_err());

        fs::write(
            cwd.join("Vagrantfile"),
            r#"
    Vagrant.configure("2") do |config|
    config.ssh.port = 1
    config.vm.provision "shell", inline: "echo hi"
    end
    "#,
        )?;

        assert!(execute(cwd, &args).is_err());
        Ok(())
    }

    #[test]
    fn test_execute_provision_empty_config() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "mock_id")?;

        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")?;

        let args = crate::cli::ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        assert!(execute(cwd, &args).is_err());

        fs::write(cwd.join("Vagrantfile"), "# Empty Vagrantfile")?;
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_provision_with_skip() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "mock_id")?;

        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        std::thread::spawn(move || while listener.accept().is_ok() {});

        std::fs::write(
            cwd.join("Vagrantfile"),
            format!(
                r#"
Vagrant.configure("2") do |config|
config.ssh.port = {port}
config.vm.provision "shell", inline: "echo 'hello'"
config.vm.provision "file", source: "foo", destination: "bar"
end
"#
            ),
        )?;

        let args = crate::cli::ProvisionArgs {
            provision_with: Some("shell".to_string()),
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_provision_empty_machines_and_not_running() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "mock_id")?;

        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        std::thread::spawn(move || while listener.accept().is_ok() {});

        fs::write(
            cwd.join("Vagrantfile"),
            format!(
                r#"
    Vagrant.configure("2") do |config|
    config.ssh.port = {port}
    config.vm.provision "shell", inline: "echo hello"
    end
    "#
            ),
        )?;

        let state_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "12345")?;

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_execute_provision_specific_name() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "mock_id")?;

        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        std::thread::spawn(move || while listener.accept().is_ok() {});

        std::fs::write(
            cwd.join("Vagrantfile"),
            format!(
                r#"
    Vagrant.configure("2") do |config|
    config.ssh.port = {port}
    config.vm.provision "shell", inline: "echo hello", name: "my_shell"
    end
    "#
            ),
        )?;

        let state_dir = cwd.join(".vagrant").join("machines").join("default");
        std::fs::create_dir_all(&state_dir)?;
        std::fs::write(state_dir.join("id"), "12345")?;

        let args = crate::cli::ProvisionArgs {
            provision_with: Some("my_shell".to_string()),
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());

        // Test with completely empty Vagrantfile so machines.is_empty() branch is covered
        let empty_dir = tempfile::tempdir()?;
        std::fs::write(empty_dir.path().join("Vagrantfile"), "# empty\n")?;
        let empty_args = crate::cli::ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        assert!(execute(empty_dir.path(), &empty_args).is_ok());
        Ok(())
    }
}
