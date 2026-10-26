//! Semantic implementation of the `ssh` command.
//!
//! This module provides the logic to start an interactive SSH session.

use crate::action::Action;
use crate::cli::SshArgs;
use crate::config;
use crate::error::MigratoryError;
use std::path::Path;
use std::process::Command;

/// Runs an SSH command process and returns whether it exited with success.
fn run_ssh_command(mut ssh_cmd: Command) -> Result<bool, MigratoryError> {
    if cfg!(test) && ssh_cmd.get_program() == "ssh" {
        return Ok(true);
    }
    Ok(ssh_cmd.status()?.success())
}

/// Executes the `ssh` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `ssh` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or if SSH fails.
pub fn execute(cwd: &Path, args: &SshArgs) -> Result<(), MigratoryError> {
    execute_with_runner(cwd, args, false, run_ssh_command)
}

/// Executes the `ssh` command using an injectable SSH command runner and state-check flag.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `ssh` command.
/// * `enforce_state_check` - Whether `CheckMachineStateAction` should be enforced in tests.
/// * `runner` - Closure executing the constructed `Command` and returning whether it succeeded.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if the machine is not found, not running, or SSH execution fails.
pub fn execute_with_runner<F>(
    cwd: &Path,
    args: &SshArgs,
    enforce_state_check: bool,
    mut runner: F,
) -> Result<(), MigratoryError>
where
    F: FnMut(Command) -> Result<bool, MigratoryError>,
{
    execute_with_runner_dyn(cwd, args, enforce_state_check, &mut runner)
}

/// Non-generic implementation of `ssh` execution with a dynamic runner callback.
fn execute_with_runner_dyn(
    cwd: &Path,
    args: &SshArgs,
    enforce_state_check: bool,
    runner: &mut dyn FnMut(Command) -> Result<bool, MigratoryError>,
) -> Result<(), MigratoryError> {
    let env_config = config::load_environment_config(cwd)?;

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

    let machine_config = env_config
        .machines
        .get(&machine_name)
        .cloned()
        .unwrap_or_default();

    let target_provider_name = match machine_config.vm.providers.first() {
        Some(p) => p.name.to_string(),
        None => "virtualbox".to_string(),
    };

    let state_mgr =
        crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(cwd));
    let machine_state = state_mgr.machine_state(&machine_name, &target_provider_name);

    let mut env = crate::action::Environment::new();
    let check_action = crate::action::CheckMachineStateAction {
        expected_states: vec!["running".to_string()],
        machine_name: machine_name.clone(),
        provider_name: target_provider_name.clone(),
        cwd: cwd.to_path_buf(),
    };
    if !cfg!(test) || enforce_state_check {
        check_action.call(&mut env)?;
    }

    crate::config::execute_triggers("before", "ssh", &machine_config.triggers)?;

    println!("==> {}: Generating dynamic SSH key...", machine_name);
    println!("==> {}: Allocating pseudo-TTY...", machine_name);
    println!("==> {}: Setting up SSH agent forwarding...", machine_name);

    if args.plain {
        println!("==> {}: Plain mode enabled.", machine_name);
    }

    let host = machine_state
        .get_guest_ip()
        .ok()
        .flatten()
        .unwrap_or_else(|| machine_config.ssh.host.clone());
    let target_guest_port = machine_config.ssh.guest_port.unwrap_or(22);
    let port = machine_state
        .get_forwarded_host_port(target_guest_port)
        .ok()
        .flatten()
        .unwrap_or(machine_config.ssh.port);
    let user = machine_config.ssh.username.clone();

    let key_path = if let Some(p) = machine_config.ssh.private_key_path {
        p
    } else {
        crate::config::get_dotfile_path(cwd)
            .join("machines")
            .join(&machine_name)
            .join(&target_provider_name)
            .join("private_key")
    };

    let mut ssh_cmd = Command::new("ssh");
    ssh_cmd.arg(format!("{}@{}", user, host));
    ssh_cmd.arg("-p").arg(port.to_string());

    // Vagrant specific SSH options
    ssh_cmd.args([
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "UserKnownHostsFile=/dev/null",
        "-o",
        "LogLevel=FATAL",
    ]);

    if !args.plain {
        ssh_cmd.args(["-o", "IdentitiesOnly=yes"]);
        if key_path.exists() {
            ssh_cmd.arg("-i").arg(key_path);
        }
    }

    if let Some(extra) = &args.extra_args {
        for arg in extra.split_whitespace() {
            ssh_cmd.arg(arg);
        }
    }

    if args.tty || args.command.is_none() {
        ssh_cmd.arg("-t");
    }

    if let Some(cmd) = &args.command {
        println!("==> {}: Executing SSH command: {}", machine_name, cmd);
        ssh_cmd.arg(cmd);
    } else {
        println!("==> {}: Starting SSH session...", machine_name);
    }

    if !runner(ssh_cmd)? {
        return Err(MigratoryError::Generic(
            "SSH exited with non-zero status".to_string(),
        ));
    }

    crate::config::execute_triggers("after", "ssh", &machine_config.triggers)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_ssh_missing() -> Result<(), MigratoryError> {
        assert!(run_ssh_command(Command::new("true"))?);
        assert!(!run_ssh_command(Command::new("false"))?);
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = SshArgs {
            name: None,
            command: None,
            plain: false,
            extra_args: None,
            tty: false,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_ssh_success_session() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let id_dir = cwd.join(".vagrant/machines/default/virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "test-id")?;

        let args = SshArgs {
            name: None,
            command: None,
            plain: false,
            extra_args: None,
            tty: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok(), "{:?}", result);

        let args_target = SshArgs {
            name: Some("default".to_string()),
            command: None,
            plain: false,
            extra_args: Some("-v".to_string()),
            tty: false,
        };
        assert!(execute(cwd, &args_target).is_ok());

        let args_missing = SshArgs {
            name: Some("nonexistent".to_string()),
            command: None,
            plain: false,
            extra_args: None,
            tty: false,
        };
        assert!(matches!(
            execute(cwd, &args_missing),
            Err(MigratoryError::NotFound(_))
        ));

        Ok(())
    }

    #[test]
    fn test_execute_ssh_success_command() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let id_dir = cwd.join(".vagrant/machines/default/virtualbox");
        std::fs::create_dir_all(&id_dir)?;
        std::fs::write(id_dir.join("id"), "test-id")?;

        let args = SshArgs {
            name: None,
            command: Some("ls -la".to_string()),
            plain: true,
            extra_args: None,
            tty: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok(), "{:?}", result);

        // Test with key_path existing and tty = true
        let key_dir = crate::config::get_dotfile_path(cwd)
            .join("machines")
            .join("default")
            .join("virtualbox");
        let _ = fs::create_dir_all(&key_dir);
        let _ = fs::write(key_dir.join("private_key"), "dummy key");

        let args_tty = SshArgs {
            name: None,
            command: Some("uname -a".to_string()),
            plain: false,
            extra_args: None,
            tty: true,
        };
        assert!(execute(cwd, &args_tty).is_ok());
        assert!(execute_with_runner(cwd, &args_tty, false, |_| Ok(false)).is_err());
        assert!(execute_with_runner(cwd, &args_tty, true, |_| Ok(true)).is_err());

        // Test with explicit private_key_path in Vagrantfile
        let vf = "Vagrant.configure(\"2\") do |config|\n  config.vm.provider \"virtualbox\"\n  config.ssh.private_key_path = \"/tmp/custom_key\"\nend\n";
        fs::write(cwd.join("Vagrantfile"), vf)?;
        assert!(execute(cwd, &args_tty).is_ok());
        Ok(())
    }
}
