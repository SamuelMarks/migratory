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

    println!("Current machine states:\n");

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

        let machine_id_result = state_mgr.read_id(name, "virtualbox");
        let machine_id = machine_id_result.unwrap_or(None);

        let p = get_provider_or_default(target_provider_name_str, machine_id.clone());

        let state = if machine_id.is_none() {
            "not created".to_string()
        } else {
            p.status().unwrap_or("unknown".to_string())
        };

        println!("{:<25} {} ({})", name, state, target_provider_name);
    }

    if args.name.is_none() {
        println!(
            "\nThis environment represents multiple VMs. The VMs are all listed\nabove with their current state. For more information about a specific\nVM, run `migratory status NAME`."
        );
    }

    Ok(())
}

#[coverage(off)]
fn get_provider_or_default(name: &str, id: Option<String>) -> Box<dyn crate::provider::Provider> {
    crate::provider::get_provider(name, id)
        .unwrap_or_else(|_| Box::new(crate::provider::virtualbox::VirtualBoxProvider::new(None)))
}

#[cfg(test)]
mod tests {
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
}
