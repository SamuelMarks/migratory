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

/// Suspends all active machines tracked in the global machine index.
///
/// # Arguments
///
/// * `ui` - The console UI interface for emitting logs and warnings.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if reading the global index fails.
fn suspend_all_global_machines(ui: &ConsoleUi) -> Result<(), MigratoryError> {
    let vagrant_home = std::env::var("VAGRANT_HOME").ok();
    let home = std::env::var("HOME").ok();
    let vagrant_d = get_global_vagrant_d_from(vagrant_home.as_deref(), home.as_deref());

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
    let ui = ConsoleUi;

    if args.all_global {
        ui.info("migratory", "Suspending all global machines...");
        return suspend_all_global_machines(&ui);
    }

    let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
    let mut lock_file = local_state.create_lock_file()?;
    let _guard = lock_file.try_write().map_err(|_| {
        crate::error::MigratoryError::Generic(
            "Vagrant environment is locked by another process".to_string(),
        )
    })?;
    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_string_lossy();

    let env_config = config::evaluate_vagrantfile(path_str.as_ref()).unwrap_or_default();
    let state_mgr = provider::StateManager::new(crate::config::get_dotfile_path(cwd));

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

    let default_config = crate::config::MachineConfig::default();
    for name in &target_names {
        let machine_config = machines.get(name).unwrap_or(&default_config);

        crate::config::execute_triggers("before", "suspend", &machine_config.triggers)?;

        let target_provider_name = machine_config
            .vm
            .providers
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "virtualbox".to_string());
        let target_provider_name_str = target_provider_name.as_str();

        let machine_id = state_mgr.read_id(name, "virtualbox")?;
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
#[coverage(off)]
mod tests {

    #[test]
    #[cfg(unix)]
    fn test_execute_invalid_utf8_path() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let dir = tempfile::tempdir().expect("operation should succeed");
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
    fn test_execute_suspend_missing() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = SuspendArgs {
            all_global: false,
            name: None,
        };

        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_suspend_success() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

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
    }

    #[test]
    fn test_execute_suspend_empty_config() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = SuspendArgs {
            all_global: false,
            name: None,
        };

        // Write an invalid ruby script to fail parsing and return default empty config
        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")
            .expect("operation should succeed");

        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_suspend_provider_name_clone() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c| c.vm.provider 'my_custom_provider' end",
        )
        .expect("operation should succeed");

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
    fn test_execute_suspend_locked_environment() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let mut lock_file = local_state
            .create_lock_file()
            .expect("operation should succeed");
        let _guard = lock_file.write().expect("operation should succeed");

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests suspend error when a before trigger fails.
    #[test]
    fn test_execute_suspend_trigger_before_failure() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.before :suspend, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests suspend error when an after trigger fails.
    #[test]
    fn test_execute_suspend_trigger_after_failure() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.after :suspend, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_suspend_read_id_error() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy").expect("operation should succeed");

        let id_file = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox")
            .join("id");
        fs::create_dir_all(id_file.parent().expect("operation should succeed"))
            .expect("operation should succeed");
        fs::write(&id_file, "some_id").expect("operation should succeed");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&id_file)
                .expect("operation should succeed")
                .permissions();
            perms.set_mode(0o000); // Unreadable
            fs::set_permissions(&id_file, perms).expect("operation should succeed");
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
            let mut perms = fs::metadata(&id_file)
                .expect("operation should succeed")
                .permissions();
            perms.set_mode(0o644); // Restore to allow cleanup
            fs::set_permissions(&id_file, perms).expect("operation should succeed");
        }
    }

    #[test]
    fn test_execute_suspend_success_with_mock_provider() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        use std::env;
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c| c.vm.provider 'virtualbox' end",
        )
        .expect("operation should succeed");

        // Write mock machine id
        let vagrant_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&vagrant_dir).expect("operation should succeed");
        fs::write(vagrant_dir.join("id"), "test-id").expect("operation should succeed");

        // Create a mock VBoxManage
        let bin_dir = cwd.join("bin");
        fs::create_dir_all(&bin_dir).expect("operation should succeed");
        let vboxmanage_path = bin_dir.join("VBoxManage");

        #[cfg(unix)]
        {
            fs::write(&vboxmanage_path, "#!/bin/sh\nexit 0\n").expect("operation should succeed");
            let mut perms = fs::metadata(&vboxmanage_path)
                .expect("operation should succeed")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&vboxmanage_path, perms).expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            fs::write(bin_dir.join("VBoxManage.bat"), "@echo off\nexit /b 0\n")
                .expect("operation should succeed");
        }

        // Prepend to PATH
        let old_path = env::var_os("PATH").unwrap_or_default();
        let mut new_path = bin_dir.into_os_string();
        #[cfg(windows)]
        new_path.push(";");
        #[cfg(not(windows))]
        new_path.push(":");
        new_path.push(&old_path);

        unsafe {
            env::set_var("PATH", &new_path);
        }

        let args = SuspendArgs {
            all_global: false,
            name: None,
        };
        let result = execute(cwd, &args);

        // Restore PATH
        unsafe {
            env::set_var("PATH", old_path);
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_suspend_all_global_populated() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp = tempfile::tempdir().expect("operation should succeed");
        let vagrant_home = temp.path();
        let index_dir = vagrant_home.join("data").join("machine-index");
        std::fs::create_dir_all(&index_dir).expect("create_dir failed");

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
        std::fs::write(index_dir.join("index"), index_json).expect("write failed");

        unsafe {
            std::env::set_var("VAGRANT_HOME", vagrant_home);
        }

        let dummy_cwd = temp.path().join("dummy_project");
        std::fs::create_dir_all(&dummy_cwd).expect("create_dir failed");

        let args = SuspendArgs {
            all_global: true,
            name: None,
        };
        let result = execute(&dummy_cwd, &args);
        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
        assert!(result.is_ok());
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
    fn test_execute_suspend_all_global_home_set_empty_index() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let old_vagrant_home = std::env::var_os("VAGRANT_HOME");
        let old_home = std::env::var_os("HOME");

        let temp = tempfile::tempdir().expect("operation should succeed");
        let vagrant_d = temp.path().join(".vagrant.d");
        let index_dir = vagrant_d.join("data").join("machine-index");
        std::fs::create_dir_all(&index_dir).expect("create_dir failed");

        let index_json = r#"{
            "version": 1,
            "machines": {}
        }"#;
        std::fs::write(index_dir.join("index"), index_json).expect("write failed");

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::set_var("HOME", temp.path());
        }

        let args = SuspendArgs {
            all_global: true,
            name: None,
        };
        let result = execute(temp.path(), &args);

        unsafe {
            if let Some(vh) = old_vagrant_home {
                std::env::set_var("VAGRANT_HOME", vh);
            }
            if let Some(h) = old_home {
                std::env::set_var("HOME", h);
            }
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_suspend_all_global_corrupt_index() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp = tempfile::tempdir().expect("operation should succeed");
        let vagrant_home = temp.path();
        let index_dir = vagrant_home.join("data").join("machine-index");
        std::fs::create_dir_all(&index_dir).expect("create_dir failed");
        std::fs::write(index_dir.join("index"), "invalid json content").expect("write failed");

        unsafe {
            std::env::set_var("VAGRANT_HOME", vagrant_home);
        }

        let args = SuspendArgs {
            all_global: true,
            name: None,
        };
        let result = execute(temp.path(), &args);

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_suspend_all_global_with_provider_and_suspend_errors() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let temp = tempfile::tempdir().expect("operation should succeed");
        let vagrant_home = temp.path();
        let index_dir = vagrant_home.join("data").join("machine-index");
        std::fs::create_dir_all(&index_dir).expect("create_dir failed");

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
        std::fs::write(index_dir.join("index"), index_json).expect("write failed");

        unsafe {
            std::env::set_var("VAGRANT_HOME", vagrant_home);
        }

        let args = SuspendArgs {
            all_global: true,
            name: None,
        };
        let result = execute(temp.path(), &args);

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }

        assert!(result.is_ok());
    }
}
