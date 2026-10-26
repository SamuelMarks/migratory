//! Semantic implementation of the `docker-logs` command.
//!
//! This module provides the logic to output the logs from a running Docker container
//! managed by the Docker provider.

use crate::cli::DockerLogsArgs;
use crate::error::MigratoryError;
use crate::state::local::LocalStateManager;
use crate::ui::Ui;
use std::path::Path;
use std::process::Command;

/// Converts a `std::io::Error` encountered while spawning `docker logs` into a [`MigratoryError`].
///
/// # Arguments
///
/// * `e` - The underlying I/O error.
///
/// # Returns
///
/// Returns [`MigratoryError::Generic`] containing the error description.
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
/// Returns a [`MigratoryError`] if the `Vagrantfile` cannot be found, if the container ID is missing, or if execution fails.
pub fn execute(cwd: &Path, args: &DockerLogsArgs) -> Result<(), MigratoryError> {
    let bin = if cfg!(test) { "echo" } else { "docker" };
    execute_with_bin(cwd, args, bin)
}

/// Executes the `docker-logs` command using a custom `docker` binary.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `docker-logs` command.
/// * `docker_bin` - Executable name or path for `docker`.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if the `Vagrantfile` cannot be found, if the container ID is missing, or if execution fails.
pub fn execute_with_bin(
    cwd: &Path,
    args: &DockerLogsArgs,
    docker_bin: &str,
) -> Result<(), MigratoryError> {
    let env_config = crate::config::load_environment_config(cwd)?;

    let base_ui = crate::ui::default_ui();
    let ui = crate::ui::ConcurrentUi::new(base_ui);
    let machine_name = env_config
        .machines
        .iter()
        .find(|(_, m)| m.vm.providers.iter().any(|p| p.name == "docker"))
        .map(|(name, _)| name.clone())
        .or_else(|| env_config.machines.keys().next().cloned())
        .unwrap_or(String::from("default"));

    let state_mgr = LocalStateManager::new(cwd.join(".vagrant"));

    let machine_id = match state_mgr.read_id(&machine_name, "docker")? {
        Some(id) if !id.trim().is_empty() => id,
        _ => {
            return Err(MigratoryError::NotFound(format!(
                "Docker container ID for machine '{machine_name}' not found. Is the container running?"
            )));
        }
    };

    ui.info(&machine_name, "Outputting logs from docker container...");

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

    run_docker_logs(docker_bin, &cmd_args, &ui, &machine_name)?;

    Ok(())
}

/// Invokes the `docker logs` subprocess with the constructed argument list.
///
/// # Arguments
///
/// * `docker_bin` - Executable name or path for `docker`.
/// * `cmd_args` - Slice of CLI arguments to pass to `docker`.
/// * `ui` - Synchronized UI handle for emitting warnings.
/// * `machine_name` - Name of the target machine for UI attribution.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if spawning `docker` fails or exits non-zero.
fn run_docker_logs(
    docker_bin: &str,
    cmd_args: &[&str],
    ui: &crate::ui::ConcurrentUi,
    machine_name: &str,
) -> Result<(), MigratoryError> {
    let status = Command::new(docker_bin)
        .args(cmd_args)
        .status()
        .map_err(map_docker_error)?;

    if !status.success() {
        ui.warn(machine_name, "Docker logs failed.");
        return Err(MigratoryError::Generic(
            "Docker logs failed with exit status".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_docker_logs_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
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
        assert!(
            map_docker_error(std::io::Error::other("err"))
                .to_string()
                .contains("err")
        );
        Ok(())
    }

    #[test]
    fn test_execute_docker_logs_container_not_found() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = DockerLogsArgs {
            follow: false,
            tail: None,
            timestamps: false,
            prefix: false,
            no_follow: false,
            no_prefix: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err_and(|e| {
            e.to_string()
                .contains("Docker container ID for machine 'default' not found")
        }));

        // Also test when id file exists but contains only whitespace
        let state_dir = cwd.join(".vagrant/machines/default/docker");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "   \n  ")?;
        assert!(execute(cwd, &args).is_err());
        Ok(())
    }

    #[test]
    fn test_execute_docker_logs_saved_id() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let state_dir = cwd.join(".vagrant/machines/default/docker");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "saved-docker-id")?;

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
        Ok(())
    }

    #[test]
    fn test_execute_docker_logs_custom_machine_in_vagrantfile() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vf_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "app" do |app|
    app.vm.provider "docker"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vf_content)?;
        let state_dir = cwd.join(".vagrant/machines/app/docker");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "app-container-id")?;

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
        Ok(())
    }

    #[test]
    fn test_execute_docker_logs_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let state_dir = cwd.join(".vagrant/machines/default/docker");
        fs::create_dir_all(&state_dir)?;
        fs::write(state_dir.join("id"), "saved-docker-id")?;

        let args = DockerLogsArgs {
            follow: false,
            tail: None,
            timestamps: false,
            prefix: false,
            no_follow: false,
            no_prefix: false,
        };
        let result = execute_with_bin(cwd, &args, "false");
        assert!(result.is_err());
        Ok(())
    }
}
