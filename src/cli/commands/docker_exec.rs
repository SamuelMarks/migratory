//! Semantic implementation of the `docker-exec` command.
//!
//! This module provides the logic to execute a command on an already-running
//! docker container via the docker provider.

use crate::cli::DockerExecArgs;
use crate::error::MigratoryError;
use crate::state::local::LocalStateManager;
use crate::ui::{ConsoleUi, Ui};
use std::path::Path;
use std::process::Command;

/// Maps a standard I/O error into a `MigratoryError`.
///
/// # Arguments
///
/// * `e` - The underlying I/O error.
///
/// # Returns
///
/// Returns a `MigratoryError::Generic` containing the error message.
fn map_docker_error(e: std::io::Error) -> MigratoryError {
    MigratoryError::Generic(e.to_string())
}

/// Executes the `docker-exec` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `docker-exec` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found, the container ID is missing,
/// or if command execution fails.
pub fn execute(cwd: &Path, args: &DockerExecArgs) -> Result<(), MigratoryError> {
    let bin = if cfg!(test) { "echo" } else { "docker" };
    execute_with_bin(cwd, args, bin)
}

/// Executes the `docker-exec` command using a custom `docker` binary.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `docker-exec` command.
/// * `docker_bin` - Executable name or path for `docker`.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found, the container ID is missing,
/// or if command execution fails.
pub fn execute_with_bin(
    cwd: &Path,
    args: &DockerExecArgs,
    docker_bin: &str,
) -> Result<(), MigratoryError> {
    let path = cwd.join("Vagrantfile");
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let ui = ConsoleUi;
    let state_mgr = LocalStateManager::new(cwd.join(".vagrant"));

    let target_machine = args.name.as_deref().unwrap_or("default");

    let machine_id = match state_mgr.read_id(target_machine, "docker") {
        Ok(Some(id)) if !id.trim().is_empty() => id,
        _ => {
            return Err(MigratoryError::NotFound(format!(
                "Docker container ID for machine '{}' not found. Is the container running?",
                target_machine
            )));
        }
    };

    ui.info(target_machine, "Executing command on docker container...");

    let mut cmd_args = vec!["exec"];

    if args.interactive {
        cmd_args.push("-i");
    }
    if args.tty {
        cmd_args.push("-t");
    }
    if args.detach {
        cmd_args.push("-d");
    }
    if let Some(user) = &args.user {
        cmd_args.push("-u");
        cmd_args.push(user);
    }

    cmd_args.push(&machine_id);

    // Pass the user's command and args to docker
    if let Some(command) = &args.command {
        cmd_args.push(command);
    }
    for arg in &args.args {
        cmd_args.push(arg);
    }

    // Default to 'sh' if no command was provided for interactive sessions
    if args.command.is_none() {
        cmd_args.push("sh");
    }

    run_docker_exec(docker_bin, &cmd_args, &ui, target_machine)?;

    Ok(())
}

/// Spawns and executes the `docker exec` command with the given argument list.
///
/// # Arguments
///
/// * `docker_bin` - Executable name or path for `docker`.
/// * `cmd_args` - The command-line arguments to pass to `docker`.
/// * `ui` - The console UI interface for emitting warnings/logs.
/// * `machine_name` - The target machine name for logging.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if spawning the process fails or the process exits with a non-zero status.
fn run_docker_exec(
    docker_bin: &str,
    cmd_args: &[&str],
    ui: &ConsoleUi,
    machine_name: &str,
) -> Result<(), MigratoryError> {
    let status = Command::new(docker_bin)
        .args(cmd_args)
        .status()
        .map_err(map_docker_error)?;

    if !status.success() {
        ui.warn(machine_name, "Docker exec failed.");
        return Err(MigratoryError::Generic(format!(
            "Docker exec exited with status: {}",
            status
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    #[cfg(unix)]
    fn test_execute_invalid_utf8_path() -> Result<(), MigratoryError> {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let dir = tempfile::tempdir()?;
        let mut path = dir.path().to_path_buf();
        let invalid_utf8 = vec![0xFF, 0xFF, 0xFF];
        path.push(OsString::from_vec(invalid_utf8));

        let args = DockerExecArgs::default();
        let res = super::execute(&path, &args);
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_docker_exec_missing_vagrantfile() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = DockerExecArgs {
            name: None,
            command: None,
            args: vec![],
            user: None,
            interactive: false,
            tty: false,
            detach: false,
            prefix: false,
            no_interactive: false,
            no_detach: false,
            no_prefix: false,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_docker_exec_missing_container_id() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = DockerExecArgs {
            name: None,
            command: None,
            args: vec![],
            user: None,
            interactive: false,
            tty: false,
            detach: false,
            prefix: false,
            no_interactive: false,
            no_detach: false,
            no_prefix: false,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_docker_exec_empty_container_id() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let state_dir = cwd.join(".vagrant/machines/default/docker");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "   ")?;

        let args = DockerExecArgs {
            name: None,
            command: None,
            args: vec![],
            user: None,
            interactive: false,
            tty: false,
            detach: false,
            prefix: false,
            no_interactive: false,
            no_detach: false,
            no_prefix: false,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_docker_exec_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let state_dir = cwd.join(".vagrant/machines/default/docker");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "container-123")?;

        let args = DockerExecArgs {
            name: None,
            command: None,
            args: vec![],
            user: Some("root".to_string()),
            interactive: false,
            tty: false,
            detach: false,
            prefix: false,
            no_interactive: false,
            no_detach: false,
            no_prefix: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_docker_exec_success_no_user() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let state_dir = cwd.join(".vagrant/machines/default/docker");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "container-123")?;

        let args = DockerExecArgs {
            name: None,
            command: None,
            args: vec![],
            user: None,
            interactive: false,
            tty: false,
            detach: false,
            prefix: false,
            no_interactive: false,
            no_detach: false,
            no_prefix: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_docker_exec_all_flags() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let state_dir = cwd.join(".vagrant/machines/test-machine/docker");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "container-test-machine")?;

        let args = DockerExecArgs {
            name: Some("test-machine".to_string()),
            command: Some("ls".to_string()),
            args: vec!["-l".to_string(), "-a".to_string()],
            user: Some("ubuntu".to_string()),
            interactive: true,
            tty: true,
            detach: true,
            prefix: true,
            no_interactive: false,
            no_detach: false,
            no_prefix: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_docker_exec_failure_propagation() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let state_dir = cwd.join(".vagrant/machines/default/docker");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "container-123")?;

        let args = DockerExecArgs {
            name: None,
            command: Some("failing_cmd".to_string()),
            args: vec![],
            user: None,
            interactive: false,
            tty: false,
            detach: false,
            prefix: false,
            no_interactive: false,
            no_detach: false,
            no_prefix: false,
        };

        let result = execute_with_bin(cwd, &args, "false");
        assert!(result.is_err());
        let missing_res = execute_with_bin(cwd, &args, "nonexistent_docker_bin_xyz");
        assert!(missing_res.is_err());
        Ok(())
    }
}
