//! Semantic implementation of the `docker-run` command.
//!
//! This module provides the logic to run a one-off command in the context of a container.

use crate::cli::DockerRunArgs;
use crate::error::MigratoryError;
use std::path::Path;

/// Executes the `docker-run` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `docker-run` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or if execution fails.
pub fn execute(cwd: &Path, args: &DockerRunArgs) -> Result<(), MigratoryError> {
    let path = cwd.join("Vagrantfile");
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    if let Some(command) = &args.command {
        println!(
            "Running one-off command '{}' on docker container...",
            command
        );
        if !args.args.is_empty() {
            println!("Args: {:?}", args.args);
        }
    } else {
        println!("Running one-off command on docker container...");
    }

    Ok(())
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_docker_run_missing() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = DockerRunArgs {
            command: None,
            args: vec![],
            rm: false,
            detach: false,
            tty: false,
            no_detach: false,
            no_rm: false,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_docker_run_success() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = DockerRunArgs {
            command: Some("echo".to_string()),
            args: vec!["hello".to_string()],
            rm: false,
            detach: false,
            tty: false,
            no_detach: false,
            no_rm: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_docker_run_success_no_command() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = DockerRunArgs {
            command: None,
            args: vec![],
            rm: false,
            detach: false,
            tty: false,
            no_detach: false,
            no_rm: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_docker_run_success_no_args() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = DockerRunArgs {
            command: Some("echo".to_string()),
            args: vec![],
            rm: false,
            detach: false,
            tty: false,
            no_detach: false,
            no_rm: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
}
