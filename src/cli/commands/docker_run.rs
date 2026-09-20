//! Semantic implementation of the `docker-run` command.
//!
//! This module provides the logic to run a one-off command in the context of a container.

use crate::cli::DockerRunArgs;
use crate::error::MigratoryError;
use std::path::Path;
use std::process::Command;

/// Resolves the Docker image name to run from the parsed environment configuration.
///
/// # Arguments
///
/// * `env_config` - The evaluated Vagrantfile configuration.
///
/// # Returns
///
/// Returns the Docker image name as a `String`.
fn resolve_docker_image(env_config: &crate::config::EnvironmentConfig) -> String {
    for machine in env_config.machines.values() {
        if let Some(box_name) = &machine.vm.box_name
            && !box_name.trim().is_empty()
        {
            return box_name.clone();
        }
    }
    "alpine:latest".to_string()
}

/// Spawns and executes the `docker run` process with the provided arguments.
///
/// # Arguments
///
/// * `args` - The command-line arguments to pass to `docker`.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if spawning the process fails or the process exits with a non-zero code.
#[coverage(off)]
fn run_docker_command(args: &[String]) -> Result<(), MigratoryError> {
    if cfg!(test) {
        if std::env::var("MIGRATORY_TEST_MOCK_DOCKER_RUN_ERROR").is_ok() {
            return Err(MigratoryError::Generic(
                "Docker run failed with exit status".to_string(),
            ));
        }
        return Ok(());
    }

    let status = Command::new("docker")
        .args(args)
        .status()
        .map_err(|e| MigratoryError::Generic(format!("Failed to execute docker: {}", e)))?;

    if !status.success() {
        return Err(MigratoryError::Generic(format!(
            "Docker run exited with status: {}",
            status
        )));
    }
    Ok(())
}

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

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = crate::config::evaluate_vagrantfile(path_str).unwrap_or_default();
    let image_name = resolve_docker_image(&env_config);

    if let Some(command) = &args.command {
        println!(
            "Running one-off command '{}' on docker container '{}'...",
            command, image_name
        );
        if !args.args.is_empty() {
            println!("Args: {:?}", args.args);
        }
    } else {
        println!("Running one-off container from image '{}'...", image_name);
    }

    let mut cmd_args: Vec<String> = vec!["run".to_string()];

    if !args.no_rm {
        cmd_args.push("--rm".to_string());
    }

    if args.detach && !args.no_detach {
        cmd_args.push("-d".to_string());
    }

    if args.tty {
        cmd_args.push("-t".to_string());
        cmd_args.push("-i".to_string());
    }

    cmd_args.push(image_name);

    if let Some(command) = &args.command {
        cmd_args.push(command.clone());
    }
    for arg in &args.args {
        cmd_args.push(arg.clone());
    }

    run_docker_command(&cmd_args)?;

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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = DockerRunArgs {
            command: Some("echo".to_string()),
            args: vec!["hello".to_string()],
            rm: true,
            detach: false,
            tty: true,
            no_detach: false,
            no_rm: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_docker_run_success_no_command() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = DockerRunArgs {
            command: None,
            args: vec![],
            rm: false,
            detach: true,
            tty: false,
            no_detach: false,
            no_rm: true,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_docker_run_with_box_config() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.box = "ubuntu:22.04"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = DockerRunArgs {
            command: Some("whoami".to_string()),
            args: vec![],
            rm: false,
            detach: false,
            tty: false,
            no_detach: true,
            no_rm: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_docker_run_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER_RUN_ERROR", "1");
        }

        let args = DockerRunArgs {
            command: Some("exit".to_string()),
            args: vec!["1".to_string()],
            rm: false,
            detach: false,
            tty: false,
            no_detach: false,
            no_rm: false,
        };
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER_RUN_ERROR");
        }
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_docker_run_detach_override_and_empty_box() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.box = "   "
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = DockerRunArgs {
            command: Some("echo".to_string()),
            args: vec!["test".to_string()],
            rm: false,
            detach: true,
            tty: false,
            no_detach: true,
            no_rm: true,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
}
