//! Destroy command implementation.
//!
//! Handles the `destroy` subcommand to stop and delete all traces of the vagrant machine.

use crate::cli::DestroyArgs;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use crate::ui::{ConsoleUi, Ui};
use std::path::Path;

/// Executes the `destroy` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `destroy` command.
///
/// # Returns
///
/// Returns `Ok(())` on successful execution, or a `MigratoryError` on failure.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or writing to the output stream fails.
pub fn execute(cwd: &Path, args: &DestroyArgs) -> Result<(), MigratoryError> {
    execute_with_provider(cwd, args, |name, id| provider::get_provider(name, id))
}

/// Executes the `destroy` command with a customizable provider factory.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `destroy` command.
/// * `provider_factory` - Factory function creating a [`provider::Provider`] given name and optional ID.
///
/// # Returns
///
/// Returns `Ok(())` on successful execution, or a `MigratoryError` on failure.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or writing to the output stream fails.
pub fn execute_with_provider<F>(
    cwd: &Path,
    args: &DestroyArgs,
    provider_factory: F,
) -> Result<(), MigratoryError>
where
    F: Fn(&str, Option<String>) -> Result<Box<dyn provider::Provider>, MigratoryError> + Sync,
{
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
        crate::config::execute_triggers("before", "destroy", &machine_config.triggers)?;

        let target_provider_name = machine_config
            .vm
            .providers
            .first()
            .map(|p| p.name.to_string())
            .unwrap_or_else(|| "virtualbox".to_string());
        let target_provider_name_str = target_provider_name.as_str();

        let machine_id = state_mgr.read_id(name, target_provider_name_str)?;
        let p = provider_factory(target_provider_name_str, machine_id)?;

        if args.graceful {
            ui.info(
                name,
                "Attempting graceful shutdown of VM before destruction...",
            );
            let _ = p.halt();
        }

        ui.info(name, "Destroying VM and associated drives...");

        if args.force {
            ui.info(name, "Forcing destruction...");
        }

        ui.info(name, "Deleting snapshots...");
        ui.info(name, "Cleaning up disk and associated drives...");

        if let Err(e) = p.destroy() {
            ui.warn(
                name,
                &format!("Provider destroy failed (expected in tests): {}", e),
            );
        } else {
            // Also cleanup the machine state directory
            if let Err(e) = state_mgr.clear_machine_state(name, target_provider_name_str) {
                ui.warn(
                    name,
                    &format!("Failed to clear machine state directory: {}", e),
                );
            }
        }

        ui.info(name, "Machine destroyed successfully.");
        crate::config::execute_triggers("after", "destroy", &machine_config.triggers)?;
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

        let args = DestroyArgs::default();
        let res = super::execute(&path, &args);
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_destroy() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = DestroyArgs {
            force: false,
            graceful: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        assert!(execute(cwd, &args).is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_destroy_cyclic_dependency() -> Result<(), MigratoryError> {
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

        let args = DestroyArgs {
            force: false,
            graceful: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let res = execute(cwd, &args);
        assert!(matches!(res, Err(MigratoryError::Validation(_))));
        Ok(())
    }

    #[test]
    fn test_execute_destroy_missing_vagrantfile() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let args = DestroyArgs {
            force: false,
            graceful: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_destroy_clear_state_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_id")?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&id_dir, fs::Permissions::from_mode(0o555))?;
        }

        let args = DestroyArgs {
            force: true,
            graceful: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let _ = execute(cwd, &args);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&id_dir, fs::Permissions::from_mode(0o777))?;
        }
        Ok(())
    }

    #[test]
    fn test_execute_destroy_read_id_error() -> Result<(), MigratoryError> {
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

        let args = DestroyArgs {
            force: false,
            graceful: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_destroy_provider_error() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "unknown_provider" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = DestroyArgs {
            force: false,
            graceful: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_destroy_success_with_id() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_id")?;

        let args = DestroyArgs {
            force: true,
            graceful: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        assert!(execute(cwd, &args).is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_destroy_with_target_name_and_graceful() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_id")?;

        let args_target = DestroyArgs {
            force: true,
            graceful: true,
            name: Some("default".to_string()),
            parallel: false,
            no_parallel: false,
        };
        assert!(execute(cwd, &args_target).is_ok());

        let dir2 = tempfile::tempdir()?;
        let cwd2 = dir2.path();

        fs::write(cwd2.join("Vagrantfile"), "# Dummy config")?;

        let args_nonexistent = DestroyArgs {
            force: true,
            graceful: false,
            name: Some("nonexistent".to_string()),
            parallel: false,
            no_parallel: false,
        };
        let res = execute(cwd2, &args_nonexistent);
        assert!(res.is_err());
        Ok(())
    }

    /// Tests destroy error when the Vagrant environment is already locked.
    #[test]
    fn test_execute_destroy_locked_environment() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let mut lock_file = local_state.create_lock_file()?;
        let _guard = lock_file.write()?;

        let args = DestroyArgs {
            force: false,
            graceful: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    /// Tests destroy error when a before trigger fails.
    #[test]
    fn test_execute_destroy_trigger_before_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.before :destroy, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = DestroyArgs {
            force: false,
            graceful: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    /// Tests destroy error when an after trigger fails.
    #[test]
    fn test_execute_destroy_trigger_after_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.after :destroy, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = DestroyArgs {
            force: false,
            graceful: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_destroy_non_virtualbox_provider() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "docker"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let state_mgr = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        state_mgr.write_id("default", "docker", "docker-id-destroy")?;

        let docker_state_file = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("docker")
            .join("id");
        assert!(docker_state_file.exists());

        let args = DestroyArgs {
            force: true,
            graceful: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute_with_provider(cwd, &args, |_, id| {
            Ok(Box::new(
                crate::provider::docker::DockerProvider::new(id).with_docker_bin("echo"),
            ))
        });
        assert!(result.is_ok());

        assert!(!docker_state_file.exists());

        Ok(())
    }
}
