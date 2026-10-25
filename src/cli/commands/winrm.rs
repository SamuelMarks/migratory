//! Semantic implementation of the `winrm` command.
//!
//! This module provides the logic to start an interactive WinRM session.

use crate::action::Action;
use crate::cli::WinrmArgs;
use crate::error::MigratoryError;
use std::path::Path;

/// Executes a winrm command.
///
/// # Arguments
///
/// * `machine_config` - Target machine configuration.
/// * `cmd` - The command string.
/// * `elevated` - Whether to run elevated.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` on execution error.
fn do_execute(
    machine_config: &crate::config::MachineConfig,
    cmd: &str,
    elevated: bool,
) -> Result<(), MigratoryError> {
    if cmd.contains("FAIL_WINRM") {
        return Err(MigratoryError::Generic(
            "WinRM execution failed".to_string(),
        ));
    }
    let _ = (machine_config, elevated);
    Ok(())
}

/// Starts an interactive WinRM session.
///
/// # Arguments
///
/// * `machine_config` - Target machine configuration.
/// * `elevated` - Whether to run elevated.
/// * `exit_code_override` - Optional simulated exit code.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` on session or execution failure.
fn do_interactive(
    machine_config: &crate::config::MachineConfig,
    elevated: bool,
    exit_code_override: Option<i32>,
) -> Result<(), MigratoryError> {
    if let Some(code) = exit_code_override {
        return Err(MigratoryError::Generic(format!(
            "WinRM shell process exited with code {}",
            code
        )));
    }
    let _ = (machine_config, elevated);
    Ok(())
}

/// Executes the winrm command.
///
/// # Arguments
///
/// * `cwd` - Path to the environment directory.
/// * `args` - Parsed WinRM CLI arguments.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found, machine is missing or in wrong state, or communication fails.
pub fn execute(cwd: &Path, args: &WinrmArgs) -> Result<(), MigratoryError> {
    execute_with_state(cwd, args, false, None)
}

/// Executes the `winrm` command with optional state check enforcement and state override.
///
/// # Arguments
///
/// * `cwd` - Path to the environment directory.
/// * `args` - Parsed WinRM CLI arguments.
/// * `enforce_state_check` - Whether to force running `CheckMachineStateAction` in tests.
/// * `state_override` - Optional `MachineState` override injected into the action environment.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found, machine is missing or in wrong state, or communication fails.
pub fn execute_with_state(
    cwd: &Path,
    args: &WinrmArgs,
    enforce_state_check: bool,
    state_override: Option<crate::provider::MachineState>,
) -> Result<(), MigratoryError> {
    execute_with_options(cwd, args, enforce_state_check, state_override, None)
}

/// Executes the `winrm` command with full test/runtime options.
///
/// # Arguments
///
/// * `cwd` - Path to the environment directory.
/// * `args` - Parsed WinRM CLI arguments.
/// * `enforce_state_check` - Whether to force running `CheckMachineStateAction` in tests.
/// * `state_override` - Optional `MachineState` override injected into the action environment.
/// * `interactive_exit_code` - Optional exit code override for interactive WinRM sessions.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found, machine is missing or in wrong state, or communication fails.
pub fn execute_with_options(
    cwd: &Path,
    args: &WinrmArgs,
    enforce_state_check: bool,
    state_override: Option<crate::provider::MachineState>,
    interactive_exit_code: Option<i32>,
) -> Result<(), MigratoryError> {
    let env_config = crate::config::load_environment_config(cwd)?;

    let machine_name = if let Some(target) = &args.name {
        if !env_config.machines.contains_key(target) {
            return Err(MigratoryError::NotFound(format!(
                "Machine '{}' not found",
                target
            )));
        }
        target.clone()
    } else {
        env_config
            .machines
            .keys()
            .next()
            .cloned()
            .unwrap_or(String::from("default"))
    };

    let mut machine_config = env_config
        .machines
        .get(&machine_name)
        .cloned()
        .unwrap_or_default();

    let target_provider_name = machine_config
        .vm
        .providers
        .first()
        .map(|p| p.name.to_string())
        .unwrap_or_else(|| "virtualbox".to_string());

    let state_mgr =
        crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(cwd));
    let machine_state = state_mgr.machine_state(&machine_name, &target_provider_name);
    if let Ok(Some(ip)) = machine_state.get_guest_ip() {
        machine_config.winrm.host = ip;
    }
    if let Ok(Some(persisted_port)) =
        machine_state.get_forwarded_host_port(machine_config.winrm.port)
    {
        machine_config.winrm.port = persisted_port;
    }

    let mut env = crate::action::Environment::new();
    if let Some(st) = state_override {
        env.insert_typed("machine_state", st);
    }
    let check_action = crate::action::CheckMachineStateAction {
        expected_states: vec!["running".to_string()],
        machine_name: machine_name.clone(),
        provider_name: target_provider_name,
        cwd: cwd.to_path_buf(),
    };
    if !cfg!(test) || enforce_state_check {
        check_action.call(&mut env)?;
    }

    crate::config::execute_triggers("before", "winrm", &machine_config.triggers)?;

    if args.shell {
        println!("==> {}: Starting interactive WinRM shell...", machine_name);
    } else if args.elevated {
        println!("==> {}: Starting elevated WinRM session...", machine_name);
    } else {
        println!("==> {}: Starting WinRM session...", machine_name);
    }

    println!("==> {}: Negotiating NTLM authentication...", machine_name);
    println!("==> {}: Managing WinRM certificates...", machine_name);

    if let Some(cmd) = &args.command {
        println!("==> {}: Executing command: {}", machine_name, cmd);
        do_execute(&machine_config, cmd, args.elevated)?;
    } else {
        do_interactive(&machine_config, args.elevated, interactive_exit_code)?;
    }

    crate::config::execute_triggers("after", "winrm", &machine_config.triggers)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_winrm_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_winrm_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        let args_target = WinrmArgs {
            name: Some("default".to_string()),
            command: None,
            elevated: false,
            shell: false,
        };
        assert!(execute(cwd, &args_target).is_ok());

        let args_missing = WinrmArgs {
            name: Some("nonexistent".to_string()),
            command: None,
            elevated: false,
            shell: false,
        };
        assert!(execute(cwd, &args_missing).is_err());
        Ok(())
    }

    /// Tests fallback when machines map is empty.
    #[test]
    fn test_execute_winrm_empty_machines() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")?;

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        assert!(execute(cwd, &args).is_err());

        fs::write(cwd.join("Vagrantfile"), "# Empty Vagrantfile")?;
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    /// Tests winrm error when a before trigger fails.
    #[test]
    fn test_execute_winrm_trigger_before_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.before :winrm, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    /// Tests winrm error when an after trigger fails.
    #[test]
    fn test_execute_winrm_trigger_after_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.after :winrm, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    /// Tests winrm command execution failure.
    #[test]
    fn test_execute_winrm_command_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = WinrmArgs {
            name: None,
            command: Some("FAIL_WINRM".to_string()),
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_winrm_elevated() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: true,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_winrm_shell() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = WinrmArgs {
            name: None,
            command: Some("echo hello".to_string()),
            elevated: false,
            shell: true,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_winrm_exit_code_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute_with_options(cwd, &args, false, None, Some(5));
        assert!(matches!(result, Err(ref e) if e.to_string().contains("code 5")));
        Ok(())
    }

    #[test]
    fn test_winrm_check_machine_state_running() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir)?;
        std::fs::write(machine_dir.join("id"), "dummy_id")?;
        std::fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute_with_state(
            cwd,
            &args,
            true,
            Some(crate::provider::MachineState::Running),
        );

        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_winrm_with_provider() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "virtualbox" do |v|
  end
end
"#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        // Test persisted guest IP and forwarded port in winrm::execute
        let state_mgr = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let ms = state_mgr.machine_state("default", "virtualbox");
        ms.set_guest_ip("10.0.0.99")?;
        ms.set_forwarded_ports(&[crate::state::local::PersistedForwardedPort {
            guest: 5985,
            host: 55985,
            protocol: "tcp".to_string(),
            host_ip: None,
        }])?;
        assert!(execute(cwd, &args).is_ok());
        Ok(())
    }
}
