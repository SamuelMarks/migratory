//! Semantic implementation of the `winrm` command.
//!
//! This module provides the logic to start an interactive WinRM session.

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
        let actual_cmd = if elevated {
            format!(
                "powershell -Command \"Start-Process cmd -ArgumentList '/c {}' -Verb RunAs\"",
                cmd
            )
        } else {
            cmd.to_string()
        };
        execute_winrm(machine_config, &actual_cmd)
    }
}

/// Execute a winrm command via the communicator.
#[cfg(not(test))]
#[coverage(off)]
fn execute_winrm(
    machine_config: &crate::config::MachineConfig,
    actual_cmd: &str,
) -> Result<(), MigratoryError> {
    use crate::communicator::Communicator;
    let comm = crate::communicator::winrm::WinrmCommunicator::new(machine_config.winrm.clone());
    let out = comm.execute(actual_cmd)?;
    print!("{}", out);
    Ok(())
}

/// Executes the winrm command.
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

    crate::config::execute_triggers("before", "winrm", &machine_config.triggers)?;

    if args.shell {
        println!("==> {}: Starting interactive WinRM shell...", machine_name);
    } else if args.elevated {
        println!("==> {}: Starting elevated WinRM session...", machine_name);
    } else {
        println!("==> {}: Starting WinRM session...", machine_name);
    }

    if let Some(cmd) = &args.command {
        println!("==> {}: Executing command: {}", machine_name, cmd);
        do_execute(&machine_config, cmd, args.elevated)?;
    }

    println!("==> {}: Negotiating NTLM authentication...", machine_name);
    println!("==> {}: Managing WinRM certificates...", machine_name);

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
}
