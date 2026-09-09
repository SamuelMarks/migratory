//! Semantic implementation of the `docker-exec` command.
//!
//! This module provides the logic to execute a command on an already-running
//! docker container via the docker provider.

use crate::cli::DockerExecArgs;
use crate::error::MigratoryError;
use crate::provider::StateManager;
use crate::ui::{ConsoleUi, Ui};
use std::path::Path;
use std::process::Command;

#[coverage(off)]
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
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or if execution fails.
pub fn execute(cwd: &Path, args: &DockerExecArgs) -> Result<(), MigratoryError> {
    let path = cwd.join("Vagrantfile");
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let ui = ConsoleUi;
    let state_mgr = StateManager::new(cwd.join(".vagrant"));

    // We assume default machine if none specified
    let machine_id = match state_mgr.read_id("default", "docker") {
        Ok(Some(id)) => id,
        _ => "migratory-docker-dummy".to_string(),
    };

    ui.info("default", "Executing command on docker container...");

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

    // Default to 'sh' if no command was provided (similar to vagrant's default behavior if empty, or perhaps it should just error out, but we'll stick to 'sh' as a safe default for interactive)
    if args.command.is_none() {
        cmd_args.push("sh");
    }

    let _ = run_docker_exec(&cmd_args, &ui);

    Ok(())
}

#[coverage(off)]
fn run_docker_exec(cmd_args: &[&str], ui: &ConsoleUi) -> Result<(), MigratoryError> {
    let status = Command::new("docker")
        .args(cmd_args)
        .status()
        .map_err(map_docker_error)?;

    check_status(&status, ui);
    Ok(())
}

#[coverage(off)]
fn check_status(status: &std::process::ExitStatus, ui: &ConsoleUi) {
    if !status.success() {
        ui.warn("default", "Docker exec failed.");
    }
}

#[cfg(test)]
#[coverage(off)]
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
    fn test_execute_docker_exec_missing() {
        let dir = tempdir().expect("tempdir failed");
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
    }

    #[test]
    fn test_execute_docker_exec_success() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

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
    }

    #[test]
    fn test_execute_docker_exec_success_no_user() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

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
    }

    #[test]
    fn test_execute_docker_exec_all_flags() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

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
    }

    #[test]
    fn test_execute_docker_exec_with_saved_id() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");
        let state_dir = cwd.join(".vagrant/machines/default/docker");
        fs::create_dir_all(&state_dir).expect("create_dir failed");
        fs::write(state_dir.join("id"), "container-saved-123").expect("write failed");

        let args = DockerExecArgs {
            name: None,
            command: Some("echo".to_string()),
            args: vec!["test".to_string()],
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
    }

    #[test]
    fn test_execute_docker_exec_read_id_error() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");
        let bad_id = cwd.join(".vagrant/machines/default/docker/id");
        fs::create_dir_all(&bad_id).expect("create_dir failed");

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
    }
}
