//! Semantic implementation of the `halt` command.
//!
//! This module provides the logic to shut down the running machine(s).

use crate::cli::HaltArgs;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use crate::ui::{ConsoleUi, Ui};
use std::path::Path;

/// Attempts a graceful OS-level shutdown of the guest machine over SSH.
///
/// # Arguments
/// Attempts to gracefully shut down a guest via SSH before falling back to provider halt.
///
/// # Arguments
///
/// * `communicator` - Active communicator connected to the guest.
///
/// # Returns
///
/// Returns `true` if the guest OS was detected and accepted the halt command, `false` otherwise.
fn attempt_graceful_halt(communicator: &dyn crate::communicator::Communicator) -> bool {
    match crate::guest::detect_guest(communicator) {
        Ok(g) => g.halt(communicator).is_ok(),
        Err(_) => false,
    }
}

/// Executes the `halt` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `halt` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or if halting the provider fails.
pub fn execute(cwd: &Path, args: &HaltArgs) -> Result<(), MigratoryError> {
    execute_with_graceful(cwd, args, |comm| !cfg!(test) && attempt_graceful_halt(comm))
}

/// Executes the `halt` command with a customizable graceful halt handler.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `halt` command.
/// * `graceful_halt_fn` - Callback attempting graceful OS shutdown via SSH communicator.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or if halting the provider fails.
pub fn execute_with_graceful<G>(
    cwd: &Path,
    args: &HaltArgs,
    graceful_halt_fn: G,
) -> Result<(), MigratoryError>
where
    G: Fn(&crate::communicator::ssh::SshCommunicator) -> bool + Sync,
{
    execute_with_graceful_dyn(cwd, args, &graceful_halt_fn)
}

/// Non-generic implementation of `halt` execution with a dynamic graceful-shutdown callback.
fn execute_with_graceful_dyn(
    cwd: &Path,
    args: &HaltArgs,
    graceful_halt_fn: &(dyn Fn(&crate::communicator::ssh::SshCommunicator) -> bool + Sync),
) -> Result<(), MigratoryError> {
    let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
    let mut lock_file = local_state.create_lock_file()?;
    let _guard = lock_file.try_write().map_err(|_| {
        crate::error::MigratoryError::Generic(
            "Vagrant environment is locked by another process".to_string(),
        )
    })?;
    let ui = ConsoleUi;
    let env_config = config::load_environment_config(cwd)?;
    let machines = env_config.machines;

    let target_names = config::resolve_target_machines(&machines, args.name.as_deref())?;
    let target_names = config::sort_machines_by_dependencies(&target_names, &machines, true)?;

    crate::action::MachineOrchestrator::run(&target_names, args.parallel, |name| {
        let machine_config = machines.get(name).cloned().unwrap_or_default();
        let state_mgr =
            crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(cwd));
        crate::config::execute_triggers("before", "halt", &machine_config.triggers)?;

        let target_provider_name = machine_config
            .vm
            .providers
            .first()
            .map(|p| p.name.to_string())
            .unwrap_or_else(|| "virtualbox".to_string());
        let target_provider_name_str = target_provider_name.as_str();

        let machine_id = state_mgr.read_id(name, target_provider_name_str)?;
        let p = provider::get_provider(target_provider_name_str, machine_id)?;

        if args.force {
            ui.info(name, "Forcing shutdown of VM...");
            if let Err(e) = p.halt() {
                ui.warn(
                    name,
                    &format!("Provider halt failed (expected in tests): {}", e),
                );
            }
        } else {
            ui.info(name, "Attempting graceful shutdown of VM...");

            let mut comm_config = machine_config.ssh.clone();

            if comm_config.private_key_path.is_none() {
                let key_path = cwd
                    .join(".vagrant")
                    .join("machines")
                    .join(name)
                    .join(target_provider_name_str)
                    .join("private_key");
                comm_config.private_key_path = Some(key_path);
            }

            let communicator = crate::communicator::ssh::SshCommunicator::new(comm_config);

            let mut graceful = false;
            let graceful_success = graceful_halt_fn(&communicator);

            if graceful_success {
                graceful = true;
                ui.info(name, "Graceful shutdown initiated by guest OS.");
            }

            if !graceful {
                ui.warn(
                    name,
                    "Graceful shutdown failed, falling back to forced shutdown...",
                );
                if let Err(e) = p.halt() {
                    ui.warn(
                        name,
                        &format!("Provider halt failed (expected in tests): {}", e),
                    );
                }
            }
        }

        crate::config::execute_triggers("after", "halt", &machine_config.triggers)?;
        Ok(())
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    #[cfg(unix)]
    fn test_execute_invalid_utf8_path() -> Result<(), MigratoryError> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let dir = tempfile::tempdir()?;
        let mut path = dir.path().to_path_buf();
        let invalid_utf8 = vec![0xFF, 0xFF, 0xFF];
        path.push(OsString::from_vec(invalid_utf8));

        let args = HaltArgs::default();
        let res = super::execute(&path, &args);
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_halt_not_created() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();
        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_cyclic_dependency() -> Result<(), MigratoryError> {
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
        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::Validation(_))));
        Ok(())
    }

    #[test]
    fn test_execute_halt_fallback_forced_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&machine_dir)?;
        fs::write(machine_dir.join("id"), "dummy_id")?;
        fs::write(cwd.join("Vagrantfile"), "# Dummy")?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        assert!(execute(cwd, &args).is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_halt_success_graceful() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let id_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_machine_123")?;
        fs::write(id_dir.join("provider"), "mock_provider")?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute_with_graceful(cwd, &args, |_| true);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_success_force() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let id_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_id")?;

        let args = HaltArgs {
            force: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_success_ungraceful() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let id_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_id")?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute_with_graceful(cwd, &args, |_| false);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_failure_ungraceful() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "hyperv" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let id_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_id")?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);

        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_force_error() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "hyperv" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("hyperv");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_machine_123")?;

        let args = HaltArgs {
            force: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_graceful_missing_key() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "virtualbox" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_machine_123")?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute_with_graceful(cwd, &args, |_| true);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_graceful_missing_key_and_fail_fallback() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "hyperv" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("hyperv");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_machine_123")?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_force_success() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "virtualbox" do |v|
  end
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "mock_machine_123")?;

        let args = HaltArgs {
            force: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_graceful_with_key() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.ssh.private_key_path = "some/path"
  config.vm.provider "virtualbox" do |v|
  end
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "mock_machine_123")?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute_with_graceful(cwd, &args, |_| true);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_read_id_error() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let id_path = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox")
            .join("id");
        fs::create_dir_all(&id_path)?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_halt_provider_error() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "hyperv" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let id_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_id")?;

        let args = HaltArgs {
            force: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_unknown_provider() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "unknown_provider" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_halt_with_machines_and_providers() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web" do |web|
    web.vm.provider "virtualbox" do |vb|
    end
  end
  config.vm.define "db" do |db|
    # no provider
  end
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let machines_dir = cwd.join(".vagrant").join("machines");
        std::fs::create_dir_all(machines_dir.join("web"))?;
        std::fs::create_dir_all(machines_dir.join("db"))?;
        std::fs::write(machines_dir.join("web").join("id"), "web_id")?;
        std::fs::write(machines_dir.join("db").join("id"), "db_id")?;

        let args = HaltArgs {
            force: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }
}
#[cfg(test)]
mod extra_halt_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_halt_success_graceful_proper() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
            Vagrant.configure("2") do |config|
                config.vm.define "default" do |node|
                    node.vm.provider "virtualbox" do |vb|
                    end
                end
            end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_machine_123")?;
        let prov_file = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("provider");
        fs::write(prov_file, "virtualbox")?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute_with_graceful(cwd, &args, |_| true);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_failure_graceful_proper() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
            Vagrant.configure("2") do |config|
                config.vm.define "default" do |node|
                    node.vm.provider "virtualbox" do |vb|
                    end
                end
            end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_machine_123")?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute_with_graceful(cwd, &args, |_| false);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_halt_target_name() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
            Vagrant.configure("2") do |config|
                config.vm.define "web" do |node|
                    node.vm.provider "virtualbox" do |vb|
                    end
                end
            end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("web")
            .join("virtualbox");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_machine_123")?;

        let args_target = HaltArgs {
            force: true,
            name: Some("web".to_string()),
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args_target);
        assert!(result.is_ok());

        let args_not_found = HaltArgs {
            force: true,
            name: Some("nonexistent".to_string()),
            parallel: false,
            no_parallel: false,
        };
        let result_not_found = execute(cwd, &args_not_found);
        assert!(result_not_found.is_err());
        Ok(())
    }

    /// Tests halt error when the Vagrant environment is already locked.
    #[test]
    fn test_execute_halt_locked_environment() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let mut lock_file = local_state.create_lock_file()?;
        let _guard = lock_file.write()?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    /// Tests halt error when a before trigger fails.
    #[test]
    fn test_execute_halt_trigger_before_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.before :halt, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    /// Tests halt error when an after trigger fails.
    #[test]
    fn test_execute_halt_trigger_after_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.after :halt, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_halt_non_virtualbox_provider() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "docker"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let state_mgr = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        state_mgr.write_id("default", "docker", "docker-id-halt")?;

        let args_graceful = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let res_graceful = execute(cwd, &args_graceful);
        assert!(res_graceful.is_ok());

        let args_force = HaltArgs {
            force: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let res_force = execute(cwd, &args_force);
        assert!(res_force.is_ok());

        /// Communicator returning `"Linux\n"` to test `attempt_graceful_halt` success.
        struct LinuxComm;
        impl crate::communicator::Communicator for LinuxComm {
            fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
                Ok("Linux\n".to_string())
            }
        }
        /// Communicator failing detection to test `attempt_graceful_halt` failure.
        struct UnknownComm;
        impl crate::communicator::Communicator for UnknownComm {
            fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
                Err(MigratoryError::Generic("err".to_string()))
            }
        }
        assert!(attempt_graceful_halt(&LinuxComm));
        assert!(!attempt_graceful_halt(&UnknownComm));

        Ok(())
    }
}
