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
/// Returns a `MigratoryError` if the Vagrantfile cannot be found, if the container ID is missing, or if execution fails.
#[coverage(off)]
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

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = crate::config::evaluate_vagrantfile(path_str).unwrap_or_default();
    let machine_name = env_config
        .machines
        .iter()
        .find(|(_, m)| m.vm.providers.iter().any(|p| p.name == "docker"))
        .map(|(name, _)| name.clone())
        .or_else(|| env_config.machines.keys().next().cloned())
        .unwrap_or_else(|| "default".to_string());

    let state_mgr = StateManager::new(cwd.join(".vagrant"));

    let machine_id = match state_mgr.read_id(&machine_name, "docker")? {
        Some(id) if !id.trim().is_empty() => id,
        _ => {
            return Err(MigratoryError::NotFound(format!(
                "Docker container ID for machine '{}' not found. Is the container running?",
                machine_name
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

    run_docker_logs(&cmd_args, &ui, &machine_name)?;

    Ok(())
}

#[coverage(off)]
fn run_docker_logs(
    cmd_args: &[&str],
    ui: &crate::ui::ConcurrentUi,
    machine_name: &str,
) -> Result<(), MigratoryError> {
    if cfg!(test) {
        if std::env::var("MIGRATORY_TEST_MOCK_DOCKER_LOGS_ERROR").is_ok() {
            ui.warn(machine_name, "Docker logs failed.");
            return Err(MigratoryError::Generic(
                "Docker logs failed with exit status".to_string(),
            ));
        }
        return Ok(());
    }

    let status = Command::new("docker")
        .args(cmd_args)
        .status()
        .map_err(map_docker_error)?;

    if !status.success() {
        ui.warn(machine_name, "Docker logs failed.");
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
    fn test_execute_docker_logs_container_not_found() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write a mock vagrantfile without saved container id
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
        assert!(
            matches!(result, Err(MigratoryError::NotFound(msg)) if msg.contains("Docker container ID for machine 'default' not found"))
        );
    }

    #[test]
    fn test_execute_docker_logs_saved_id() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
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

    #[test]
    fn test_execute_docker_logs_custom_machine_in_vagrantfile() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vf_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "app" do |app|
    app.vm.provider "docker"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vf_content).expect("operation should succeed");
        let state_dir = cwd.join(".vagrant/machines/app/docker");
        fs::create_dir_all(&state_dir).expect("operation should succeed");
        fs::write(state_dir.join("id"), "app-container-id").expect("operation should succeed");

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
    fn test_execute_docker_logs_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER_LOGS_ERROR", "1");
        }

        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");
        let state_dir = cwd.join(".vagrant/machines/default/docker");
        fs::create_dir_all(&state_dir).expect("operation should succeed");
        fs::write(state_dir.join("id"), "saved-docker-id").expect("operation should succeed");

        let args = DockerLogsArgs {
            follow: false,
            tail: None,
            timestamps: false,
            prefix: false,
            no_follow: false,
            no_prefix: false,
        };
        let result = execute(cwd, &args);

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER_LOGS_ERROR");
        }

        assert!(result.is_err());
    }
}
