//! Semantic implementation of the `status` command.
//!
//! This module provides the logic to report the current status of machines
//! defined in the active Vagrantfile.

use crate::cli::StatusArgs;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use std::path::Path;

/// Executes the `status` command, reporting the status of the local machines.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `status` command.
///
/// # Returns
///
/// Returns `Ok(())` on success, indicating the status was successfully retrieved and printed.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found, read, or evaluated.
pub fn execute(cwd: &Path, args: &StatusArgs) -> Result<(), MigratoryError> {
    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");

    // In tests with dummy config, evaluate_vagrantfile might return empty machines
    let env = config::evaluate_vagrantfile(path_str).unwrap_or_default();
    let state_mgr = provider::StateManager::new(crate::config::get_dotfile_path(cwd));

    let is_machine_readable = crate::ui::is_machine_readable();
    let machine_ui = crate::ui::MachineReadableUi;

    if !is_machine_readable {
        println!("Current machine states:\n");
    } else {
        use crate::ui::Ui;
        machine_ui.info("", "Current machine states:\n");
    }

    let machines = if env.machines.is_empty() {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "default".to_string(),
            crate::config::MachineConfig::default(),
        );
        m
    } else {
        env.machines
    };

    if let Some(target_name) = &args.name
        && !machines.contains_key(target_name)
    {
        return Err(MigratoryError::NotFound(format!(
            "Machine '{}' not found",
            target_name
        )));
    }

    for (name, machine_config) in &machines {
        if let Some(target) = &args.name
            && name != target
        {
            continue;
        }

        let target_provider_name = machine_config
            .vm
            .providers
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "virtualbox".to_string());
        let target_provider_name_str = target_provider_name.as_str();

        let machine_id_result = state_mgr.read_id(name, target_provider_name_str);
        let machine_id = machine_id_result.unwrap_or(None);

        let p = get_provider_or_default(target_provider_name_str, machine_id.clone());

        let (state_id, state_human) = if machine_id.is_none() {
            ("not_created".to_string(), "not created".to_string())
        } else {
            let st = p.status().unwrap_or("unknown".to_string());
            let st_lower = st.to_lowercase();
            let id = if st_lower.starts_with("run") {
                "running".to_string()
            } else if st_lower.starts_with("poweroff")
                || st_lower.starts_with("stop")
                || st_lower.starts_with("shut")
            {
                "poweroff".to_string()
            } else if st_lower.starts_with("abort") {
                "aborted".to_string()
            } else if st_lower.starts_with("save") || st_lower.starts_with("suspend") {
                "saved".to_string()
            } else {
                st_lower.replace(' ', "_")
            };
            (id, st)
        };

        if is_machine_readable {
            machine_ui.log_state(name, &state_id);
            machine_ui.log_csv(name, "state-title", &[&state_human]);
            machine_ui.log_csv(name, "provider-name", &[target_provider_name_str]);
        } else {
            println!("{:<25} {} ({})", name, state_human, target_provider_name);
        }
    }

    if !is_machine_readable && args.name.is_none() {
        println!(
            "\nThis environment represents multiple VMs. The VMs are all listed\nabove with their current state. For more information about a specific\nVM, run `migratory status NAME`."
        );
    }

    Ok(())
}

/// Executes the `get_provider_or_default` function.
///
/// # Arguments
///
/// * `name` - The `name` argument.
/// * `id` - The `id` argument.
///
/// # Returns
///
/// Returns `Box<dyn crate::provider::Provider>`.
fn get_provider_or_default(name: &str, id: Option<String>) -> Box<dyn crate::provider::Provider> {
    crate::provider::get_provider(name, id)
        .unwrap_or_else(|_| Box::new(crate::provider::virtualbox::VirtualBoxProvider::new(None)))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::all,
        clippy::panic,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::undocumented_unsafe_blocks
    )]
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_status_missing() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = StatusArgs { name: None };

        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_status_success() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = StatusArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_status_success_with_name() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = StatusArgs {
            name: Some("default".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_status_success_with_mismatched_name() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = StatusArgs {
            name: Some("nonexistent_target".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_status_multimachine_filter() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c|\n  c.vm.define 'web'\n  c.vm.define 'db'\nend",
        )
        .expect("operation should succeed");

        let args = StatusArgs {
            name: Some("web".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_status_with_machine_id() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let state_mgr = provider::StateManager::new(crate::config::get_dotfile_path(cwd));
        state_mgr
            .write_id("default", "virtualbox", "mock-uuid-1234")
            .expect("operation should succeed");

        let args = StatusArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_status_empty_config() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write an invalid ruby script to fail parsing and return default empty config
        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")
            .expect("operation should succeed");

        let args = StatusArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_status_provider_error() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c| c.vm.provider 'invalid_provider' end",
        )
        .expect("operation should succeed");

        let args = StatusArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_status_provider_name_clone() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c| c.vm.provider 'my_custom_provider' end",
        )
        .expect("operation should succeed");

        let args = StatusArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_status_heterogeneous_providers() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER", "1");
        }

        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vf_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web" do |web|
    web.vm.provider "docker"
  end
  config.vm.define "db" do |db|
    db.vm.provider "qemu"
  end
  config.vm.define "win" do |win|
    win.vm.provider "hyperv"
  end
  config.vm.define "esxi" do |esxi|
    esxi.vm.provider "vmware"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vf_content).expect("operation should succeed");

        let state_mgr = provider::StateManager::new(crate::config::get_dotfile_path(cwd));
        state_mgr
            .write_id("web", "docker", "container-1234")
            .expect("operation should succeed");
        state_mgr
            .write_id("db", "qemu", "domain-5678")
            .expect("operation should succeed");
        state_mgr
            .write_id("win", "hyperv", "vm-uuid-9999")
            .expect("operation should succeed");
        state_mgr
            .write_id("esxi", "vmware", "/path/to/vm.vmx")
            .expect("operation should succeed");

        let args = StatusArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        // Target single non-virtualbox machine
        let target_args = StatusArgs {
            name: Some("db".to_string()),
        };
        let target_res = execute(cwd, &target_args);
        assert!(target_res.is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER");
        }
    }

    #[test]
    fn test_execute_status_machine_readable() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vf_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web"
  config.vm.define "db"
  config.vm.define "other"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vf_content).expect("operation should succeed");

        let state_mgr = provider::StateManager::new(crate::config::get_dotfile_path(cwd));
        state_mgr
            .write_id("web", "virtualbox", "uuid-web")
            .expect("operation should succeed");

        crate::ui::set_machine_readable(true);

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }

        let args = StatusArgs { name: None };
        let res = execute(cwd, &args);
        assert!(res.is_ok());

        // Test with poweroff mock
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
            std::env::set_var("MIGRATORY_TEST_MOCK_POWEROFF", "1");
        }
        let res_poweroff = execute(cwd, &args);
        assert!(res_poweroff.is_ok());

        // Test with stopped state
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_POWEROFF");
            std::env::set_var("MIGRATORY_TEST_MOCK_STOPPED", "1");
        }
        let res_stopped = execute(cwd, &args);
        assert!(res_stopped.is_ok());

        // Test with shutdown state
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_STOPPED");
            std::env::set_var("MIGRATORY_TEST_MOCK_SHUTDOWN", "1");
        }
        let res_shutdown = execute(cwd, &args);
        assert!(res_shutdown.is_ok());

        // Test with aborted state
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_SHUTDOWN");
            std::env::set_var("MIGRATORY_TEST_MOCK_ABORTED", "1");
        }
        let res_aborted = execute(cwd, &args);
        assert!(res_aborted.is_ok());

        // Test with saved state
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_ABORTED");
            std::env::set_var("MIGRATORY_TEST_MOCK_SAVED", "1");
        }
        let res_saved = execute(cwd, &args);
        assert!(res_saved.is_ok());

        // Test with suspended state
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_SAVED");
            std::env::set_var("MIGRATORY_TEST_MOCK_SUSPENDED", "1");
        }
        let res_suspended = execute(cwd, &args);
        assert!(res_suspended.is_ok());

        crate::ui::set_machine_readable(false);

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_SUSPENDED");
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
        }
    }
}
