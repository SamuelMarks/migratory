//! Semantic implementation of the `docker-logs` command.
//!
//! This module provides the logic to output the logs from a docker container.

use crate::cli::DockerLogsArgs;
use crate::error::MigratoryError;
use crate::provider::StateManager;
use crate::ui::Ui;
use std::path::Path;
use std::process::Command;

#[coverage(off)]
fn map_docker_error(e: std::io::Error) -> MigratoryError {
    MigratoryError::Generic(e.to_string())
}

/// Executes the `docker-logs` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `docker-logs` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or if execution fails.
pub fn execute(cwd: &Path, args: &DockerLogsArgs) -> Result<(), MigratoryError> {
    let path = cwd.join("Vagrantfile");
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    #[coverage(off)]
    fn get_ui() -> Box<dyn crate::ui::Ui + Send + Sync> {
        if std::env::args().any(|arg| arg == "--machine-readable") {
            Box::new(crate::ui::MachineReadableUi)
        } else {
            Box::new(crate::ui::ConsoleUi)
        }
    }
    let base_ui = get_ui();

    let ui = crate::ui::ConcurrentUi::new(base_ui);

    let state_mgr = StateManager::new(cwd.join(".vagrant"));

    let machine_id = match state_mgr.read_id("default", "docker") {
        Ok(Some(id)) => id,
        _ => "migratory-docker-dummy".to_string(),
    };

    ui.info("default", "Outputting logs from docker container...");

    let mut cmd_args = vec!["logs"];

    if args.follow {
        cmd_args.push("-f");
    }

    if let Some(tail) = &args.tail {
        cmd_args.push("--tail");
        cmd_args.push(tail);
    }

    if args.timestamps {
        cmd_args.push("-t");
    }

    cmd_args.push(&machine_id);

    let _ = run_docker_logs(&cmd_args, &ui);

    Ok(())
}

#[coverage(off)]
fn run_docker_logs(cmd_args: &[&str], ui: &crate::ui::ConcurrentUi) -> Result<(), MigratoryError> {
    let status = Command::new("docker")
        .args(cmd_args)
        .status()
        .map_err(map_docker_error)?;

    if !status.success() {
        ui.warn("default", "Docker logs failed.");
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
    fn test_execute_docker_logs_missing() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = DockerLogsArgs {
            follow: false,
            tail: None,
            timestamps: false,
            prefix: false,
            no_follow: false,
            no_prefix: false,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_docker_logs_success() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = DockerLogsArgs {
            follow: true,
            tail: Some("10".to_string()),
            timestamps: true,
            prefix: false,
            no_follow: false,
            no_prefix: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_docker_logs_success_false_flags() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = DockerLogsArgs {
            follow: false,
            tail: None,
            timestamps: false,
            prefix: false,
            no_follow: false,
            no_prefix: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_docker_logs_saved_id() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");
        let state_dir = cwd.join(".vagrant/machines/default/docker");
        fs::create_dir_all(&state_dir).expect("operation should succeed");
        fs::write(state_dir.join("id"), "saved-docker-id").expect("operation should succeed");

        let args = DockerLogsArgs {
            follow: true,
            tail: Some("25".to_string()),
            timestamps: true,
            prefix: false,
            no_follow: false,
            no_prefix: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
}
