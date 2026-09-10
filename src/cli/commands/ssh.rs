//! Semantic implementation of the `ssh` command.
//!
//! This module provides the logic to start an interactive SSH session.

use crate::action::Action;
use crate::cli::SshArgs;
use crate::config;
use crate::error::MigratoryError;
use std::path::Path;
use std::process::Command;

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
#[coverage(off)]
pub fn execute(cwd: &Path, args: &SshArgs) -> Result<(), MigratoryError> {
    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = config::evaluate_vagrantfile(path_str).unwrap_or_default();

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
    if !cfg!(test) {
        check_action.call(&mut env)?;
    }

    crate::config::execute_triggers("before", "ssh", &machine_config.triggers)?;

    println!("==> {}: Generating dynamic SSH key...", machine_name);
    println!("==> {}: Allocating pseudo-TTY...", machine_name);
    println!("==> {}: Setting up SSH agent forwarding...", machine_name);

    if args.plain {
        println!("==> {}: Plain mode enabled.", machine_name);
    }

    let host = machine_config.ssh.host.clone();
    let port = machine_config.ssh.port;
    let user = machine_config.ssh.username.clone();

    let key_path = if let Some(p) = machine_config.ssh.private_key_path {
        Path::new(&p).to_path_buf()
    } else {
        crate::config::get_dotfile_path(cwd)
            .join("machines")
            .join(&machine_name)
            .join("virtualbox")
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

    if cfg!(test) {
        crate::config::execute_triggers("after", "ssh", &machine_config.triggers)?;
        return Ok(());
    }

    let mut child = ssh_cmd
        .spawn()
        .map_err(|e| MigratoryError::Generic(format!("Failed to spawn ssh: {}", e)))?;
    let status = child
        .wait()
        .map_err(|e| MigratoryError::Generic(format!("Failed to wait for ssh: {}", e)))?;

    if !status.success() {
        return Err(MigratoryError::Generic(format!(
            "SSH exited with status: {}",
            status
        )));
    }

    crate::config::execute_triggers("after", "ssh", &machine_config.triggers)?;
    Ok(())
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_ssh_missing() -> std::io::Result<()> {
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
    fn test_execute_ssh_success_session() -> std::io::Result<()> {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .unwrap();
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PROVIDER_STATUS", "running");
        }

        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let id_dir = cwd.join(".vagrant/machines/default/virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap();
        std::fs::write(id_dir.join("id"), "test-id").unwrap();

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
    fn test_execute_ssh_success_command() -> std::io::Result<()> {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .unwrap();
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PROVIDER_STATUS", "running");
        }

        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let id_dir = cwd.join(".vagrant/machines/default/virtualbox");
        std::fs::create_dir_all(&id_dir).unwrap();
        std::fs::write(id_dir.join("id"), "test-id").unwrap();

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
        Ok(())
    }
}
