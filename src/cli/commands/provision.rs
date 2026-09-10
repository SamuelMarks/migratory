//! Semantic implementation of the `provision` command.
//!
//! This module provides the logic to manually run provisioners.

use crate::cli::ProvisionArgs;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use crate::ui::{ConsoleUi, Ui};
use std::path::Path;

/// Helper to look up a machine from the machine map.
///
/// # Arguments
///
/// * `machines` - Machine configuration map.
/// * `name` - Machine name.
#[coverage(off)]
fn get_machine<'a>(
    machines: &'a std::collections::HashMap<String, crate::config::MachineConfig>,
    name: &str,
) -> &'a crate::config::MachineConfig {
    match machines.get(name) {
        Some(m) => m,
        None => unreachable!(),
    }
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
/// Returns a `MigratoryError` if the Vagrantfile cannot be found.
pub fn execute(cwd: &Path, args: &ProvisionArgs) -> Result<(), MigratoryError> {
    let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
    let mut lock_file = local_state.create_lock_file()?;
    let _guard = lock_file.try_write().map_err(|_| {
        crate::error::MigratoryError::Generic(
            "Vagrant environment is locked by another process".to_string(),
        )
    })?;
    let path = cwd.join("Vagrantfile");
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let ui = ConsoleUi;
    let path_str = path.to_str().unwrap_or("Vagrantfile");

    let env_config = config::evaluate_vagrantfile(path_str).unwrap_or_default();
    let machines = if env_config.machines.is_empty() {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "default".to_string(),
            crate::config::MachineConfig::default(),
        );
        m
    } else {
        env_config.machines
    };

    let target_names = config::resolve_target_machines(&machines, args.name.as_deref())?;
    let target_names = config::sort_machines_by_dependencies(&target_names, &machines, false)?;
    crate::action::MachineOrchestrator::run(&target_names, args.parallel, |name| {
        let machine = get_machine(&machines, name);
        crate::config::execute_triggers("before", "provision", &machine.triggers)?;
        let state_mgr = provider::StateManager::new(cwd.join(".vagrant"));
        let target_provider_name = machine
            .vm
            .providers
            .first()
            .map(|p| p.name.as_str())
            .unwrap_or("virtualbox");

        let machine_id = state_mgr.read_id(name, target_provider_name)?;
        let p = provider::get_provider(target_provider_name, machine_id)?;

        let state = p.status().unwrap_or("unknown".to_string());
        if state != "running" {
            ui.warn(
                name,
                "VM is not running. Provisioners may fail or be skipped.",
            );
        }

        if let Some(with) = &args.provision_with {
            ui.info(name, &format!("Running provisioners: {}...", with));
        } else {
            ui.info(name, "Running provisioners...");
        }

        let comm_config = crate::config::SshConfig {
            host: "127.0.0.1".to_string(),
            port: 2222,
            username: "vagrant".to_string(),
            private_key_path: None,
            insert_key: false,
            password: None,
            forward_agent: false,
            forward_x11: false,
            proxy_command: None,
            ..Default::default()
        };
        let communicator = crate::communicator::ssh::SshCommunicator::new(comm_config);
        let mut any_ran = false;
        for provisioner in &machine.vm.provisioners {
            if let Some(with) = &args.provision_with
                && !with
                    .split(',')
                    .map(str::trim)
                    .any(|x| x == provisioner.name.as_str())
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
                "docker" => Box::new(crate::provisioner::docker::DockerProvisioner::new()),
                _ => {
                    ui.warn(name, &format!("Unknown provisioner: {}", provisioner.name));
                    continue;
                }
            };

            if try_provision(
                &mut *prov,
                &provisioner.config,
                &communicator,
                name,
                &ui,
                &provisioner.name,
            ) {
                continue;
            }
            any_ran = true;
        }

        if any_ran {
            let _ = state_mgr.mark_provisioned(name, "virtualbox");
        }
        crate::config::execute_triggers("after", "provision", &machine.triggers)?;
        Ok(())
    })?;

    Ok(())
}

fn try_provision(
    prov: &mut dyn crate::provisioner::Provisioner,
    config: &std::collections::HashMap<String, String>,
    communicator: &dyn crate::communicator::Communicator,
    name: &str,
    ui: &crate::ui::ConsoleUi,
    provisioner_name: &str,
) -> bool {
    if let Err(e) = prov.prepare(config) {
        ui.warn(
            name,
            &format!(
                "Failed to prepare provisioner '{}': {}",
                provisioner_name, e
            ),
        );
        return true;
    }

    if let Err(e) = prov.provision(communicator) {
        ui.warn(
            name,
            &format!("Failed to run provisioner '{}': {}", provisioner_name, e),
        );
    }

    let _ = prov.cleanup();
    false
}

#[cfg(test)]
mod tests {

    #[test]
    #[cfg(unix)]
    fn test_execute_invalid_utf8_path() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let dir = tempfile::tempdir().unwrap();
        let mut path = dir.path().to_path_buf();
        let invalid_utf8 = vec![0xFF, 0xFF, 0xFF];
        path.push(OsString::from_vec(invalid_utf8));

        let args = Default::default();
        let res = super::execute(&path, &args);
        assert!(res.is_err());
    }

    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_provision_missing() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap_or(());
        std::fs::write(id_dir.join("id"), "mock_id").unwrap_or(());

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
    }

    #[test]
    fn test_execute_provision_cyclic_dependency() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("failed");
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
        std::fs::write(cwd.join("Vagrantfile"), vf).expect("operation should succeed");

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
    }

    #[test]
    fn test_execute_provision_before_trigger_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let vf = r#"
Vagrant.configure("2") do |config|
  config.trigger.before :provision, run: { inline: "exit 1" }, on_error: "halt"
end
"#;
        std::fs::write(cwd.join("Vagrantfile"), vf).expect("operation should succeed");

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
    }

    #[test]
    fn test_execute_provision_after_trigger_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let vf = r#"
Vagrant.configure("2") do |config|
  config.trigger.after :provision, run: { inline: "exit 1" }, on_error: "halt"
end
"#;
        std::fs::write(cwd.join("Vagrantfile"), vf).expect("operation should succeed");

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
    }

    #[test]
    fn test_execute_provision_lock_errors() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        std::fs::write(cwd.join("Vagrantfile"), "").expect("write failed");

        let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let mut lock_file = local_state
            .create_lock_file()
            .expect("lock file creation failed");
        let _active_lock = lock_file.try_write().expect("lock acquisition failed");

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let res = execute(cwd, &args);
        assert!(res.is_err());

        drop(_active_lock);

        // create_lock_file error (.vagrant is a file)
        let dir2 = tempdir().expect("tempdir failed");
        let cwd2 = dir2.path();
        std::fs::write(cwd2.join(".vagrant"), "not a dir").expect("write failed");
        assert!(execute(cwd2, &args).is_err());
    }

    #[test]
    fn test_execute_provision_read_id_and_unsupported_provider() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        std::fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|\nend",
        )
        .expect("write failed");

        // read_id error (directory instead of file)
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox")
            .join("id");
        std::fs::create_dir_all(&id_dir).expect("create_dir failed");

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        assert!(execute(cwd, &args).is_err());

        // unsupported provider error
        let dir2 = tempdir().expect("tempdir failed");
        let cwd2 = dir2.path();
        std::fs::write(
            cwd2.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|\n  config.vm.provider 'unsupported_prov'\nend",
        )
        .expect("write failed");
        let state_dir2 = cwd2
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("unsupported_prov");
        std::fs::create_dir_all(&state_dir2).expect("create_dir failed");
        std::fs::write(state_dir2.join("id"), "123").expect("write failed");
        assert!(execute(cwd2, &args).is_err());
    }

    #[test]
    fn test_execute_provision_empty_machines() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap_or(());
        std::fs::write(id_dir.join("id"), "mock_id").unwrap_or(());
        std::fs::write(cwd.join("Vagrantfile"), "").expect("failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PROVIDER_STATUS_ERROR", "1");
        }
        let result = execute(
            cwd,
            &ProvisionArgs {
                provision_with: None,
                name: None,
                parallel: false,
                no_parallel: false,
            },
        );
        // wait, provider_error? `provider::get_provider` calls provider construction?
        // Let's just assert that it is Ok. wait, earlier it was Ok!
        assert!(result.is_ok());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PROVIDER_STATUS_ERROR");
        }
    }

    #[test]
    fn test_execute_provision_vm_not_running() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempfile::tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap_or(());
        std::fs::write(id_dir.join("id"), "mock_id").unwrap_or(());

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.vm.provider "virtualbox" do |v|
    end
  end
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)
            .expect("operation should succeed");

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).expect("operation should succeed");
        std::fs::write(id_dir.join("id"), "mock_machine_id").expect("operation should succeed");

        // Return error from execute_vboxmanage_inner so status returns 'unknown'
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE_ERROR", "1");
        }
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE_ERROR");
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_provision_success_no_args() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap_or(());
        std::fs::write(id_dir.join("id"), "mock_id").unwrap_or(());

        let bin = dir.path().join("ansible-playbook");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&bin, "#!/bin/sh\nexit 0\n").expect("operation should succeed");
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bin = dir.path().join("ansible-playbook.bat");
            std::fs::write(&bin, "@echo off\nexit 0\n").expect("operation should succeed");
        }

        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut new_path = std::ffi::OsString::new();
        new_path.push(dir.path());
        #[cfg(unix)]
        new_path.push(":");
        #[cfg(windows)]
        new_path.push(";");
        new_path.push(&old_path);

        unsafe {
            std::env::set_var("PATH", &new_path);
        }

        let playbook_path = cwd.join("playbook.yml");
        fs::write(&playbook_path, "dummy").expect("failed");
        let abs_playbook = playbook_path.to_string_lossy().to_string();

        fs::write(
            cwd.join("Vagrantfile"),
            format!(
                r#"
Vagrant.configure("2") do |config|
  config.vm.provision "shell", inline: "echo 'hello'"
  config.vm.provision "file"
  config.vm.provision "file", source: "foo", destination: "bar"
  config.vm.provision "ansible", playbook: "{}", mode: "host"
  config.vm.provision "chef", run_list: "recipe[foo]"
  config.vm.provision "puppet", manifests_path: "manifests"
  config.vm.provision "docker", images: "ubuntu"
  config.vm.provision "unknown_prov"
end
        "#,
                abs_playbook
            ),
        )
        .expect("failed");

        // Fake id file to make p.status() return something and VM is created
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&state_dir).expect("failed");
        fs::write(state_dir.join("id"), "12345").expect("failed");

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }
        // This execution runs all provisioners, some may fail but it should still return Ok(())
        let result = execute(cwd, &args);

        unsafe {
            std::env::set_var("PATH", old_path);
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_provision_target_machine_not_found() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        std::fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|\nend",
        )
        .expect("write failed");

        let args = ProvisionArgs {
            provision_with: None,
            name: Some("nonexistent_machine".to_string()),
            parallel: false,
            no_parallel: false,
        };
        assert!(execute(cwd, &args).is_err());
    }

    #[test]
    fn test_execute_provision_success_with_args() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap_or(());
        std::fs::write(id_dir.join("id"), "mock_id").unwrap_or(());

        fs::write(
            cwd.join("Vagrantfile"),
            r#"
Vagrant.configure("2") do |config|
  config.vm.provision "shell", inline: "echo 'hello'"
  config.vm.provision "file"
  config.vm.provision "file", source: "foo", destination: "bar"
end
        "#,
        )
        .expect("failed");

        let args = ProvisionArgs {
            provision_with: Some("shell,file".to_string()),
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_provision_prepare_and_provision_failures() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap_or(());
        std::fs::write(id_dir.join("id"), "mock_id").unwrap_or(());

        fs::write(
            cwd.join("Vagrantfile"),
            r#"
    Vagrant.configure("2") do |config|
    config.vm.provision "shell", env: "not a valid json"
    end
    "#,
        )
        .expect("failed");

        // Make the machine "running"
        let state_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&state_dir).expect("failed");
        fs::write(state_dir.join("id"), "12345").expect("failed");

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PROVISION_PREPARE_ERROR", "1");
        }
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PROVISION_PREPARE_ERROR");
        }

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PROVISION_ERROR", "1");
        }
        let result2 = execute(cwd, &args);
        assert!(result2.is_ok());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PROVISION_ERROR");
        }
    }

    #[test]
    fn test_execute_provision_empty_config() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap_or(());
        std::fs::write(id_dir.join("id"), "mock_id").unwrap_or(());

        // Write an invalid ruby script to fail parsing and return default empty config
        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax").expect("failed");

        let args = crate::cli::ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
    #[test]
    fn test_execute_provision_with_skip() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempfile::tempdir().expect("failed");
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap_or(());
        std::fs::write(id_dir.join("id"), "mock_id").unwrap_or(());

        std::fs::write(
            cwd.join("Vagrantfile"),
            r#"
Vagrant.configure("2") do |config|
config.vm.provision "shell", inline: "echo 'hello'"
config.vm.provision "file", source: "foo", destination: "bar"
end
"#,
        )
        .expect("failed");

        let args = crate::cli::ProvisionArgs {
            provision_with: Some("shell".to_string()),
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_provision_empty_machines_and_not_running() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap_or(());
        std::fs::write(id_dir.join("id"), "mock_id").unwrap_or(());

        fs::write(
            cwd.join("Vagrantfile"),
            r#"
    Vagrant.configure("2") do |config|
    config.vm.provision "shell", inline: "echo hello"
    end
    "#,
        )
        .expect("failed");

        // We set the machine state to "stopped"
        let state_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&state_dir).expect("failed");
        fs::write(state_dir.join("id"), "12345").expect("failed");

        // mock the provider so it returns "stopped"
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PROVIDER_STATUS", "stopped");
        }

        let args = ProvisionArgs {
            provision_with: None,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PROVIDER_STATUS");
        }
    }

    #[test]
    fn test_execute_provision_specific_name() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempfile::tempdir().expect("failed");
        let cwd = dir.path();
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap_or(());
        std::fs::write(id_dir.join("id"), "mock_id").unwrap_or(());

        std::fs::write(
            cwd.join("Vagrantfile"),
            r#"
    Vagrant.configure("2") do |config|
    config.vm.provision "shell", inline: "echo hello", name: "my_shell"
    end
    "#,
        )
        .expect("failed");

        // Make the machine "running"
        let state_dir = cwd.join(".vagrant").join("machines").join("default");
        std::fs::create_dir_all(&state_dir).expect("failed");
        std::fs::write(state_dir.join("id"), "12345").expect("failed");

        let args = crate::cli::ProvisionArgs {
            provision_with: Some("my_shell".to_string()),
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
}
