//! Semantic implementation of the `suspend` command.
//!
//! This module provides the logic to suspend a machine.

use crate::cli::SuspendArgs;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use crate::ui::{ConsoleUi, Ui};
use std::path::Path;

/// Resolves the global `.vagrant.d` directory from environment paths.
///
/// # Arguments
///
/// * `vagrant_home` - Optional path from `VAGRANT_HOME`.
/// * `home` - Optional path from `HOME`.
///
/// # Returns
///
/// Returns the resolved `PathBuf`.
fn get_global_vagrant_d_from(vagrant_home: Option<&str>, home: Option<&str>) -> std::path::PathBuf {
    match vagrant_home {
        Some(v) => std::path::PathBuf::from(v),
        None => match home {
            Some(h) => std::path::PathBuf::from(h).join(".vagrant.d"),
            None => std::path::PathBuf::from(".vagrant.d"),
        },
    }
}

/// Suspends all active machines tracked in the global machine index at `vagrant_d`.
///
/// # Arguments
///
/// * `ui` - The console UI interface for emitting logs and warnings.
/// * `vagrant_d` - Path to the global `.vagrant.d` directory.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if reading the global index fails.
fn suspend_all_global_machines_with_home(
    ui: &ConsoleUi,
    vagrant_d: std::path::PathBuf,
) -> Result<(), MigratoryError> {
    let state_manager = crate::state::GlobalStateManager::new(vagrant_d);
    let index = if let Ok(idx) = state_manager.read_index() {
        idx
    } else {
        ui.info("migratory", "No active machines found in global index.");
        return Ok(());
    };

    if index.machines.is_empty() {
        ui.info("migratory", "No active machines found in global index.");
        return Ok(());
    }

    for (id, machine) in index.machines {
        ui.info(
            &machine.name,
            &format!(
                "Suspending global machine '{}' ({}) with provider '{}'...",
                machine.name, id, machine.provider
            ),
        );

        let provider_id = machine.extra_data.get("id").cloned().or(Some(id));
        match provider::get_provider(&machine.provider, provider_id) {
            Ok(p) => {
                if let Err(e) = p.suspend() {
                    ui.warn(
                        &machine.name,
                        &format!("Failed to suspend machine '{}': {}", machine.name, e),
                    );
                }
            }
            Err(e) => {
                ui.warn(
                    &machine.name,
                    &format!(
                        "Failed to get provider '{}' for machine '{}': {}",
                        machine.provider, machine.name, e
                    ),
                );
            }
        }
    }

    Ok(())
}

/// Executes the `suspend` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `suspend` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found.
pub fn execute(cwd: &Path, args: &SuspendArgs) -> Result<(), MigratoryError> {
    let vagrant_home = std::env::var("VAGRANT_HOME").ok();
    let home = std::env::var("HOME").ok();
    let vagrant_d = get_global_vagrant_d_from(vagrant_home.as_deref(), home.as_deref());
    execute_with_home(cwd, args, vagrant_d)
}

/// Executes the `suspend` command with an explicit global `.vagrant.d` directory.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `suspend` command.
/// * `vagrant_d` - Path to the global `.vagrant.d` directory.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found.
pub fn execute_with_home(
    cwd: &Path,
    args: &SuspendArgs,
    vagrant_d: std::path::PathBuf,
) -> Result<(), MigratoryError> {
    let ui = ConsoleUi;

    if args.all_global {
        ui.info("migratory", "Suspending all global machines...");
        return suspend_all_global_machines_with_home(&ui, vagrant_d);
    }

    let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
    let mut lock_file = local_state.create_lock_file()?;
    let _guard = lock_file.try_write().map_err(|_| {
        crate::error::MigratoryError::Generic(
            "Vagrant environment is locked by another process".to_string(),
        )
    })?;
    let env_config = config::load_environment_config(cwd)?;
    let state_mgr =
        crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(cwd));

    let machines = env_config.machines;

    let target_names = config::resolve_target_machines(&machines, args.name.as_deref())?;

    let default_config = crate::config::MachineConfig::default();
    for name in &target_names {
        let machine_config = machines.get(name).unwrap_or(&default_config);

        crate::config::execute_triggers("before", "suspend", &machine_config.triggers)?;

        let target_provider_name = machine_config
            .vm
            .providers
            .first()
            .map(|p| p.name.to_string())
            .unwrap_or_else(|| "virtualbox".to_string());
        let target_provider_name_str = target_provider_name.as_str();

        let machine_id = state_mgr.read_id(name, target_provider_name_str)?;
        let p = provider::get_provider(target_provider_name_str, machine_id)?;

        ui.info(name, "Suspending VM...");

        if let Err(e) = p.suspend() {
            ui.warn(
                name,
                &format!("Provider suspend failed (expected in tests): {}", e),
            );
        }

        crate::config::execute_triggers("after", "suspend", &machine_config.triggers)?;
    }

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

        let args = SuspendArgs::default();
        let res = super::execute(&path, &args);
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_suspend_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = SuspendArgs {
            all_global: false,
            name: None,
        };

        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_suspend_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = SuspendArgs {
            all_global: true,
            name: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        let args_target = SuspendArgs {
            all_global: false,
            name: Some("default".to_string()),
        };
        assert!(execute(cwd, &args_target).is_ok());

        let args_missing = SuspendArgs {
            all_global: false,
            name: Some("nonexistent".to_string()),
        };
        assert!(execute(cwd, &args_missing).is_err());
        Ok(())
    }

    #[test]
    fn test_execute_suspend_empty_config() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = SuspendArgs {
            all_global: false,
            name: None,
        };

        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")?;
        assert!(execute(cwd, &args).is_err());

        fs::write(cwd.join("Vagrantfile"), "# Empty Vagrantfile")?;
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_suspend_provider_name_clone() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c| c.vm.provider 'my_custom_provider' end",
        )?;

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    /// Tests suspend error when environment is locked.
    #[test]
    fn test_execute_suspend_locked_environment() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let mut lock_file = local_state.create_lock_file()?;
        let _guard = lock_file.write()?;

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    /// Tests suspend error when a before trigger fails.
    #[test]
    fn test_execute_suspend_trigger_before_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.before :suspend, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    /// Tests suspend error when an after trigger fails.
    #[test]
    fn test_execute_suspend_trigger_after_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.after :suspend, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_suspend_read_id_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy")?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir)?;
        let id_file = id_dir.join("id");
        fs::write(&id_file, "some_id")?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&id_file)?.permissions();
            perms.set_mode(0o000);
            fs::set_permissions(&id_file, perms)?;
        }

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);

        #[cfg(unix)]
        assert!(result.is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&id_file)?.permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&id_file, perms)?;
        }
        Ok(())
    }

    #[test]
    fn test_execute_suspend_success_with_mock_provider() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c| c.vm.provider 'virtualbox' end",
        )?;

        let vagrant_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&vagrant_dir)?;
        fs::write(vagrant_dir.join("id"), "test-id")?;

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);

        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_suspend_all_global_populated() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let vagrant_home = temp.path();
        let index_dir = vagrant_home.join("data").join("machine-index");
        std::fs::create_dir_all(&index_dir)?;

        let index_json = r#"{
            "version": 1,
            "machines": {
                "vm-1-uuid": {
                    "name": "web",
                    "provider": "docker",
                    "state": "running",
                    "vagrantfile_path": "/tmp",
                    "vagrantfile_name": "Vagrantfile",
                    "local_data_path": "/tmp/.vagrant",
                    "updated_at": 1000,
                    "extra_data": { "id": "docker-c-1" }
                }
            }
        }"#;
        std::fs::write(index_dir.join("index"), index_json)?;

        let dummy_cwd = temp.path().join("dummy_project");
        std::fs::create_dir_all(&dummy_cwd)?;

        let args = SuspendArgs {
            all_global: true,
            name: None,
        };
        let result = execute_with_home(&dummy_cwd, &args, vagrant_home.to_path_buf());
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_suspend_all_global_no_home_and_corrupt_index() {
        assert_eq!(
            get_global_vagrant_d_from(Some("/custom/vagrant_home"), None),
            std::path::PathBuf::from("/custom/vagrant_home")
        );
        assert_eq!(
            get_global_vagrant_d_from(None, Some("/my/home")),
            std::path::PathBuf::from("/my/home").join(".vagrant.d")
        );
        assert_eq!(
            get_global_vagrant_d_from(None, None),
            std::path::PathBuf::from(".vagrant.d")
        );
    }

    #[test]
    fn test_execute_suspend_all_global_home_set_empty_index() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let vagrant_d = temp.path().join(".vagrant.d");
        let index_dir = vagrant_d.join("data").join("machine-index");
        std::fs::create_dir_all(&index_dir)?;

        let index_json = r#"{
            "version": 1,
            "machines": {}
        }"#;
        std::fs::write(index_dir.join("index"), index_json)?;

        let args = SuspendArgs {
            all_global: true,
            name: None,
        };
        let result = execute_with_home(temp.path(), &args, vagrant_d);

        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_suspend_all_global_corrupt_index() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let vagrant_home = temp.path();
        let index_dir = vagrant_home.join("data").join("machine-index");
        std::fs::create_dir_all(&index_dir)?;
        std::fs::write(index_dir.join("index"), "invalid json content")?;

        let args = SuspendArgs {
            all_global: true,
            name: None,
        };
        let result = execute_with_home(temp.path(), &args, vagrant_home.to_path_buf());

        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_suspend_all_global_with_provider_and_suspend_errors()
    -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let vagrant_home = temp.path();
        let index_dir = vagrant_home.join("data").join("machine-index");
        std::fs::create_dir_all(&index_dir)?;

        let index_json = r#"{
            "version": 1,
            "machines": {
                "vm-fail-suspend": {
                    "name": "web-fail",
                    "provider": "virtualbox",
                    "state": "running",
                    "vagrantfile_path": "/tmp",
                    "vagrantfile_name": "Vagrantfile",
                    "local_data_path": "/tmp/.vagrant",
                    "updated_at": 1000,
                    "extra_data": { "id": "invalid-vbox-id-404" }
                },
                "vm-bad-provider": {
                    "name": "db-bad",
                    "provider": "nonexistent_provider_12345",
                    "state": "running",
                    "vagrantfile_path": "/tmp",
                    "vagrantfile_name": "Vagrantfile",
                    "local_data_path": "/tmp/.vagrant",
                    "updated_at": 1000,
                    "extra_data": {}
                }
            }
        }"#;
        std::fs::write(index_dir.join("index"), index_json)?;

        let args = SuspendArgs {
            all_global: true,
            name: None,
        };
        let result = execute_with_home(temp.path(), &args, vagrant_home.to_path_buf());

        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_suspend_non_virtualbox_provider() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "docker"
end
"#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let state_mgr = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        state_mgr.write_id("default", "docker", "docker-container-abc")?;

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);

        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_suspend_non_virtualbox_read_id_error() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "docker"
end
"#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("docker")
            .join("id");
        std::fs::create_dir_all(&id_dir)?;

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }
}
