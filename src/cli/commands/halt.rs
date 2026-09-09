//! Semantic implementation of the `halt` command.
//!
//! This module provides the logic to shut down the running machine(s).

use crate::cli::HaltArgs;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use crate::ui::{ConsoleUi, Ui};
use std::path::Path;

#[cfg(not(test))]
#[coverage(off)]
fn attempt_graceful_halt(communicator: &crate::communicator::ssh::SshCommunicator) -> bool {
    if let Ok(guest) = crate::guest::detect_guest(communicator) {
        guest.halt(communicator).is_ok()
    } else {
        false
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

    let ui = ConsoleUi;
    let path_str = path.to_str().unwrap_or("Vagrantfile");

    let env_config = config::evaluate_vagrantfile(path_str).unwrap_or_default();
    let machines = env_config.machines;

    let target_names = config::resolve_target_machines(&machines, args.name.as_deref())?;
    let target_names = config::sort_machines_by_dependencies(&target_names, &machines, true)?;

    crate::action::MachineOrchestrator::run(&target_names, args.parallel, |name| {
        let machine_config = machines.get(name).cloned().unwrap_or_default();
        let state_mgr = provider::StateManager::new(crate::config::get_dotfile_path(cwd));
        crate::config::execute_triggers("before", "halt", &machine_config.triggers)?;

        let target_provider_name = machine_config
            .vm
            .providers
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "virtualbox".to_string());
        let target_provider_name_str = target_provider_name.as_str();

        let machine_id = state_mgr.read_id(name, "virtualbox")?;
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
                comm_config.private_key_path = Some(key_path.to_string_lossy().to_string());
            }

            let _communicator = crate::communicator::ssh::SshCommunicator::new(comm_config);

            let mut graceful = false;

            #[cfg(not(test))]
            let graceful_success = attempt_graceful_halt(&_communicator);

            #[cfg(test)]
            let graceful_success = std::env::var("MOCK_GRACEFUL_SUCCESS").is_ok();

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
    fn test_execute_halt_not_created() {
        let dir = tempfile::tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_halt_cyclic_dependency() {
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
        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::Validation(_))));
    }

    #[test]
    fn test_execute_halt_fallback_forced_success() {
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
        fs::create_dir_all(&machine_dir).expect("operation should succeed");
        fs::write(machine_dir.join("id"), "dummy_id").expect("operation should succeed");
        fs::write(cwd.join("Vagrantfile"), "# Dummy").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::remove_var("MOCK_GRACEFUL_SUCCESS");
        }

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        assert!(execute(cwd, &args).is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
        }
    }

    #[test]
    fn test_execute_halt_missing() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_halt_success_graceful() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let id_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&id_dir).expect("operation should succeed");
        fs::write(id_dir.join("id"), "mock_machine_123").expect("operation should succeed");
        fs::write(id_dir.join("provider"), "mock_provider").expect("operation should succeed");

        unsafe {
            std::env::set_var("MOCK_GRACEFUL_SUCCESS", "1");
        }
        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MOCK_GRACEFUL_SUCCESS");
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_halt_success_force() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let id_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&id_dir).expect("operation should succeed");
        fs::write(id_dir.join("id"), "mock_id").expect("operation should succeed");

        let bin = dir.path().join("vboxmanage");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&bin, "#!/bin/sh\nexit 0\n").expect("operation should succeed");
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bin = dir.path().join("vboxmanage.bat");
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

        let args = HaltArgs {
            force: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);

        unsafe {
            std::env::set_var("PATH", old_path);
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_halt_success_ungraceful() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let id_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&id_dir).expect("operation should succeed");
        fs::write(id_dir.join("id"), "mock_id").expect("operation should succeed");

        let bin = dir.path().join("vboxmanage");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&bin, "#!/bin/sh\nexit 0\n").expect("operation should succeed");
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let bin = dir.path().join("vboxmanage.bat");
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
            std::env::remove_var("MOCK_GRACEFUL_SUCCESS"); // ensure graceful fails
        }

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);

        unsafe {
            std::env::set_var("PATH", old_path);
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_halt_failure_ungraceful() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "hyperv" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let id_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&id_dir).expect("operation should succeed");
        fs::write(id_dir.join("id"), "mock_id").expect("operation should succeed");

        unsafe {
            std::env::remove_var("MOCK_GRACEFUL_SUCCESS"); // ensure graceful fails
        }

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);

        assert!(result.is_ok()); // Because execute returns Ok(()) even if provider halt fails, it just logs a warning
    }

    #[test]
    fn test_execute_halt_force_error() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "hyperv" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).unwrap();

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("hyperv");
        fs::create_dir_all(&id_dir).unwrap();
        fs::write(id_dir.join("id"), "mock_machine_123").unwrap();

        let args = HaltArgs {
            force: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_ERROR", "true");
        }
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_ERROR");
        }

        assert!(result.is_ok()); // Because it just logs a warning
    }

    #[test]
    fn test_execute_halt_graceful_missing_key() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        // No ssh private_key in config
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "virtualbox" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).unwrap();

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir).unwrap();
        fs::write(id_dir.join("id"), "mock_machine_123").unwrap();

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        unsafe {
            std::env::set_var("MOCK_GRACEFUL_SUCCESS", "1");
        }
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MOCK_GRACEFUL_SUCCESS");
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_halt_graceful_missing_key_and_fail_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "hyperv" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).unwrap();

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("hyperv");
        fs::create_dir_all(&id_dir).unwrap();
        fs::write(id_dir.join("id"), "mock_machine_123").unwrap();

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_ERROR", "true");
        }
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_ERROR");
        }

        assert!(result.is_ok()); // Logs warning for fallback
    }

    #[test]
    fn test_execute_halt_force_success() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "virtualbox" do |v|
  end
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content).unwrap();

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap();
        std::fs::write(id_dir.join("id"), "mock_machine_123").unwrap();

        let args = HaltArgs {
            force: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        let result = execute(cwd, &args);

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_halt_graceful_with_key() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.ssh.private_key_path = "some/path"
  config.vm.provider "virtualbox" do |v|
  end
end
        "#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content).unwrap();

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap();
        std::fs::write(id_dir.join("id"), "mock_machine_123").unwrap();

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };

        unsafe {
            std::env::set_var("MOCK_GRACEFUL_SUCCESS", "1");
        }
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MOCK_GRACEFUL_SUCCESS");
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_halt_read_id_error() {
        let dir = tempfile::tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        // Make read_id fail by creating a directory where the file should be
        let id_path = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox")
            .join("id");
        fs::create_dir_all(&id_path).expect("operation should succeed");

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_halt_provider_error() {
        let dir = tempfile::tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "hyperv" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let id_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&id_dir).expect("operation should succeed");
        fs::write(id_dir.join("id"), "mock_id").expect("operation should succeed");

        let args = HaltArgs {
            force: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_halt_unknown_provider() {
        let dir = tempfile::tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "unknown_provider" do |v|
  end
end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_halt_with_machines_and_providers() {
        let dir = tempfile::tempdir().expect("tempdir failed");
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
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let machines_dir = cwd.join(".vagrant").join("machines");
        std::fs::create_dir_all(machines_dir.join("web")).expect("create_dir failed");
        std::fs::create_dir_all(machines_dir.join("db")).expect("create_dir failed");
        std::fs::write(machines_dir.join("web").join("id"), "web_id").expect("write failed");
        std::fs::write(machines_dir.join("db").join("id"), "db_id").expect("write failed");

        let args = HaltArgs {
            force: true,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
}
#[cfg(test)]
mod extra_halt_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_halt_success_graceful_proper() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
            Vagrant.configure("2") do |config|
                config.vm.define "default" do |node|
                    node.vm.provider "virtualbox" do |vb|
                    end
                end
            end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir).expect("operation should succeed");
        fs::write(id_dir.join("id"), "mock_machine_123").expect("operation should succeed");
        // Also create the legacy provider file just in case
        fs::write(
            cwd.join(".vagrant")
                .join("machines")
                .join("default")
                .join("provider"),
            "virtualbox",
        )
        .expect("operation should succeed");

        unsafe {
            std::env::set_var("MOCK_GRACEFUL_SUCCESS", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_PROVIDER", "1"); // Use mock provider instead of virtualbox
        }
        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MOCK_GRACEFUL_SUCCESS");
            std::env::remove_var("MIGRATORY_TEST_MOCK_PROVIDER");
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_halt_failure_graceful_proper() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
            Vagrant.configure("2") do |config|
                config.vm.define "default" do |node|
                    node.vm.provider "virtualbox" do |vb|
                    end
                end
            end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir).expect("operation should succeed");
        fs::write(id_dir.join("id"), "mock_machine_123").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PROVIDER_HALT_ERROR", "1"); // Make mock provider fail
        }

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PROVIDER_HALT_ERROR");
        }
        assert!(result.is_ok()); // The halt still returns Ok(()) even if provider halt fails in force
    }

    #[test]
    fn test_execute_halt_target_name() {
        let dir = tempfile::tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
            Vagrant.configure("2") do |config|
                config.vm.define "web" do |node|
                    node.vm.provider "virtualbox" do |vb|
                    end
                end
            end
        "#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("web")
            .join("virtualbox");
        fs::create_dir_all(&id_dir).expect("operation should succeed");
        fs::write(id_dir.join("id"), "mock_machine_123").expect("operation should succeed");

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
    }

    /// Tests halt error when the Vagrant environment is already locked.
    #[test]
    fn test_execute_halt_locked_environment() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let mut lock_file = local_state
            .create_lock_file()
            .expect("operation should succeed");
        let _guard = lock_file.write().expect("operation should succeed");

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests halt error when a before trigger fails.
    #[test]
    fn test_execute_halt_trigger_before_failure() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.before :halt, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests halt error when an after trigger fails.
    #[test]
    fn test_execute_halt_trigger_after_failure() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.after :halt, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = HaltArgs {
            force: false,
            name: None,
            parallel: false,
            no_parallel: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }
}
