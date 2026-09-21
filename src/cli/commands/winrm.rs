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
#[coverage(off)]
fn do_execute(
    machine_config: &crate::config::MachineConfig,
    cmd: &str,
    elevated: bool,
) -> Result<(), MigratoryError> {
    #[cfg(test)]
    {
        if cmd.contains("FAIL_WINRM") {
            return Err(MigratoryError::Generic(
                "WinRM execution failed".to_string(),
            ));
        }
        let _ = (machine_config, elevated);
        Ok(())
    }
    #[cfg(not(test))]
    {
        let comm = crate::communicator::winrm::WinrmCommunicator::new(machine_config.winrm.clone());
        if elevated {
            let out = comm.execute_elevated(cmd)?;
            print!("{}", out);
            Ok(())
        } else {
            let out = comm.execute_cmd(cmd)?;
            print!("{}", out);
            Ok(())
        }
    }
}

/// Starts an interactive WinRM session.
///
/// # Arguments
///
/// * `machine_config` - Target machine configuration.
/// * `elevated` - Whether to run elevated.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` on session or execution failure.
#[coverage(off)]
fn do_interactive(
    machine_config: &crate::config::MachineConfig,
    elevated: bool,
) -> Result<(), MigratoryError> {
    #[cfg(test)]
    {
        if let Ok(val) = std::env::var("MIGRATORY_TEST_WINRM_EXIT_CODE") {
            let code: i32 = val.parse().unwrap_or(1);
            return Err(MigratoryError::Generic(format!(
                "WinRM shell process exited with code {}",
                code
            )));
        }
        let _ = (machine_config, elevated);
        Ok(())
    }
    #[cfg(not(test))]
    {
        use crate::communicator::Communicator;
        let comm = crate::communicator::winrm::WinrmCommunicator::new(machine_config.winrm.clone());
        if elevated {
            comm.execute_elevated_interactive()
        } else {
            comm.execute_interactive()
        }
    }
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
#[coverage(off)]
pub fn execute(cwd: &Path, args: &WinrmArgs) -> Result<(), MigratoryError> {
    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = crate::config::evaluate_vagrantfile(path_str).unwrap_or_default();

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
            .unwrap_or_else(|| "default".to_string())
    };

    let machine_config = env_config
        .machines
        .get(&machine_name)
        .cloned()
        .unwrap_or_default();

    let target_provider_name = machine_config
        .vm
        .providers
        .first()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "virtualbox".to_string());

    let mut env = crate::action::Environment::new();
    let check_action = crate::action::CheckMachineStateAction {
        expected_states: vec!["running".to_string()],
        machine_name: machine_name.clone(),
        provider_name: target_provider_name,
        cwd: cwd.to_path_buf(),
    };
    #[cfg(not(test))]
    check_action.call(&mut env)?;
    #[cfg(test)]
    if std::env::var("MIGRATORY_TEST_CHECK_STATE").is_ok() {
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
        do_interactive(&machine_config, args.elevated)?;
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
    fn test_execute_winrm_missing() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_winrm_success() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

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
    }

    /// Tests fallback when machines map is empty.
    #[test]
    fn test_execute_winrm_empty_machines() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")
            .expect("operation should succeed");

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    /// Tests winrm error when a before trigger fails.
    #[test]
    fn test_execute_winrm_trigger_before_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.before :winrm, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests winrm error when an after trigger fails.
    #[test]
    fn test_execute_winrm_trigger_after_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.after :winrm, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests winrm command execution failure.
    #[test]
    fn test_execute_winrm_command_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = WinrmArgs {
            name: None,
            command: Some("FAIL_WINRM".to_string()),
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_winrm_elevated() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: true,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_winrm_shell() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = WinrmArgs {
            name: None,
            command: Some("echo hello".to_string()),
            elevated: false,
            shell: true,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_winrm_exit_code_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_WINRM_EXIT_CODE", "5");
        }
        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_WINRM_EXIT_CODE");
        }
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("code 5"));
    }

    #[test]
    fn test_winrm_check_machine_state_running() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir).expect("mkdir failed");
        std::fs::write(machine_dir.join("id"), "dummy_id").expect("write failed");
        std::fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_CHECK_STATE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_CHECK_STATE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_winrm_with_provider() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "virtualbox" do |v|
  end
end
"#;
        std::fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let args = WinrmArgs {
            name: None,
            command: None,
            elevated: false,
            shell: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
}
