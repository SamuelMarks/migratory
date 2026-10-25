//! Semantic implementation of the `status` command.
//!
//! This module provides the logic to report the current status of machines
//! defined in the active Vagrantfile.

use crate::cli::StatusArgs;
use crate::config;
use crate::error::MigratoryError;
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
    execute_with_provider(
        cwd,
        args,
        crate::ui::is_machine_readable(),
        get_provider_or_default,
    )
}

/// Executes the `status` command with customizable machine-readable flag and provider factory.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `status` command.
/// * `is_machine_readable` - Whether output should be formatted in machine-readable CSV format.
/// * `provider_factory` - Factory function creating a [`crate::provider::Provider`].
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found, read, or evaluated.
pub fn execute_with_provider<F>(
    cwd: &Path,
    args: &StatusArgs,
    is_machine_readable: bool,
    mut provider_factory: F,
) -> Result<(), MigratoryError>
where
    F: FnMut(&str, Option<String>) -> Box<dyn crate::provider::Provider>,
{
    let env = config::load_environment_config(cwd)?;
    let state_mgr =
        crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(cwd));

    let machine_ui = crate::ui::MachineReadableUi;

    if !is_machine_readable {
        println!("Current machine states:\n");
    } else {
        use crate::ui::Ui;
        machine_ui.info("", "Current machine states:\n");
    }

    let machines = env.machines;

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
            .map(|p| p.name.to_string())
            .unwrap_or_else(|| "virtualbox".to_string());
        let target_provider_name_str = target_provider_name.as_str();

        let machine_id_result = state_mgr.read_id(name, target_provider_name_str);
        let machine_id = machine_id_result.unwrap_or(None);

        let p = provider_factory(target_provider_name_str, machine_id.clone());

        let (state_id, state_human) = if machine_id.is_none() {
            ("not_created".to_string(), "not created".to_string())
        } else {
            let st = p.status().unwrap_or(crate::provider::MachineState::Unknown(
                "unknown".to_string(),
            ));
            let st_str = st.as_str().to_string();
            let st_lower = st_str.to_lowercase();
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
            (id, st_str)
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

/// Resolves a provider instance by name and optional machine ID, falling back to VirtualBox.
///
/// # Arguments
///
/// * `name` - The provider name (e.g. `"virtualbox"`, `"docker"`).
/// * `id` - Optional persisted machine identifier.
///
/// # Returns
///
/// Returns a boxed `Provider` implementation.
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
    fn test_execute_status_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = StatusArgs { name: None };

        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_status_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = StatusArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_status_success_with_name() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = StatusArgs {
            name: Some("default".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_status_success_with_mismatched_name() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = StatusArgs {
            name: Some("nonexistent_target".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_status_multimachine_filter() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c|\n  c.vm.define 'web'\n  c.vm.define 'db'\nend",
        )?;

        let args = StatusArgs {
            name: Some("web".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_status_with_machine_id() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let state_mgr =
            crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(cwd));
        state_mgr.write_id("default", "virtualbox", "mock-uuid-1234")?;

        let args = StatusArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_status_empty_config() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")?;

        let args = StatusArgs { name: None };
        assert!(execute(cwd, &args).is_err());

        fs::write(cwd.join("Vagrantfile"), "# Empty Vagrantfile")?;
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_status_provider_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c| c.vm.provider 'invalid_provider' end",
        )?;

        let args = StatusArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_status_provider_name_clone() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c| c.vm.provider 'my_custom_provider' end",
        )?;

        let args = StatusArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_status_heterogeneous_providers() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
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
        fs::write(cwd.join("Vagrantfile"), vf_content)?;

        let state_mgr =
            crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(cwd));
        state_mgr.write_id("web", "docker", "container-1234")?;
        state_mgr.write_id("db", "qemu", "domain-5678")?;
        state_mgr.write_id("win", "hyperv", "vm-uuid-9999")?;
        state_mgr.write_id("esxi", "vmware", "/path/to/vm.vmx")?;

        let args = StatusArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        // Target single non-virtualbox machine
        let target_args = StatusArgs {
            name: Some("db".to_string()),
        };
        let target_res = execute(cwd, &target_args);
        assert!(target_res.is_ok());

        Ok(())
    }

    #[test]
    fn test_execute_status_machine_readable() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vf_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web"
  config.vm.define "db"
  config.vm.define "other"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vf_content)?;

        let state_mgr =
            crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(cwd));
        state_mgr.write_id("web", "virtualbox", "uuid-web")?;

        let args = StatusArgs { name: None };

        for raw_state in [
            "running",
            "poweroff",
            "stopped",
            "shutdown",
            "aborted",
            "saved",
            "suspended",
            "custom_state",
        ] {
            let st_line = format!("VMState=\"{}\"\n", raw_state);
            let res = execute_with_provider(cwd, &args, true, |_, id| {
                let line = st_line.clone();
                Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(move |_| Ok(line.clone())),
                )
            });
            assert!(res.is_ok());
        }

        Ok(())
    }
}
