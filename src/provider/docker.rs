//! Docker provider implementation.
//!
//! This module provides the logic for interacting with Docker containers
//! acting as Vagrant machines.

use super::Provider;
use crate::config::VmConfig;
use crate::error::MigratoryError;
use std::process::Command;

/// Docker provider.
///
/// Implements the `Provider` trait to manage containers.
pub struct DockerProvider {
    /// Internal container ID
    machine_id: Option<String>,
}

impl DockerProvider {
    /// Creates a new DockerProvider instance.
    pub fn new(machine_id: Option<String>) -> Self {
        Self { machine_id }
    }

    /// Requires that the machine ID is present, returning an error if not.
    fn require_id(&self) -> Result<&str, MigratoryError> {
        self.machine_id.as_deref().ok_or_else(|| {
            MigratoryError::Generic("Container not created or ID not found".to_string())
        })
    }
}

impl Provider for DockerProvider {
    fn name(&self) -> &str {
        "docker"
    }

    #[coverage(off)]
    fn up(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let image = config.box_name.as_deref().unwrap_or("ubuntu:latest");
        let id = self
            .machine_id
            .as_deref()
            .unwrap_or("migratory-docker-dummy");

        let mut args = vec!["run", "-d", "--name", id];

        // Network port forwarding
        for net in &config.networks {
            if let crate::config::NetworkConfig::ForwardedPort {
                guest,
                host,
                protocol,
                host_ip,
                ..
            } = net
            {
                args.push("-p");
                let proto = protocol.as_deref().unwrap_or("tcp");
                let bind_ip = host_ip.as_deref().unwrap_or("0.0.0.0");
                let port_map =
                    Box::leak(format!("{}:{}:{}/{}", bind_ip, host, guest, proto).into_boxed_str());
                args.push(port_map);
            }
        }

        // Synced folders / Bind mounts
        for sf in &config.synced_folders {
            if sf.disabled {
                continue;
            }
            args.push("-v");
            let bind = Box::leak(format!("{}:{}", sf.host_path, sf.guest_path).into_boxed_str());
            args.push(bind);
        }

        let mut privileged = false;
        let mut has_init = false;
        let mut custom_cmd = None;

        if let Some(docker_prov) = config.providers.iter().find(|p| p.name == "docker") {
            if docker_prov.options.get("privileged") == Some(&"true".to_string()) {
                privileged = true;
            }
            if docker_prov.options.get("has_init") == Some(&"true".to_string())
                || docker_prov.options.get("init") == Some(&"true".to_string())
            {
                has_init = true;
                privileged = true;
            }
            if let Some(cmd) = docker_prov.options.get("cmd") {
                custom_cmd = Some(cmd.as_str());
            }
        }

        if privileged {
            args.push("--privileged");
        }

        args.push(image);

        if has_init {
            args.push("/sbin/init");
        } else if let Some(cmd) = custom_cmd {
            args.push(cmd);
        }

        // Check if there is a Dockerfile configuration in options to build instead of pull
        // (We would need to define this in ProviderConfig, but for now we look at config.providers)
        let build_dir_opt = config
            .providers
            .iter()
            .find(|p| p.name == "docker")
            .and_then(|p| p.options.get("build_dir"));

        if let Some(build_dir) = build_dir_opt {
            let _ = execute_docker_inner("docker", &["build", "-t", image, build_dir])?;
        }

        execute_docker(&args)?;
        Ok(())
    }

    #[coverage(off)]
    fn halt(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_docker(&["stop", id])?;
        Ok(())
    }

    #[coverage(off)]
    fn destroy(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_docker(&["rm", "-f", id])?;
        Ok(())
    }

    #[coverage(off)]
    fn status(&self) -> Result<String, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok("not created".to_string()),
        };

        let out = execute_docker_inner("docker", &["inspect", "--format", "{{.State.Status}}", id])
            .unwrap_or_else(|_| "unknown".to_string());

        if out.trim().is_empty() {
            return Ok("unknown".to_string());
        }

        Ok(out.trim().to_string())
    }

    #[coverage(off)]
    fn suspend(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_docker(&["pause", id])?;
        Ok(())
    }

    #[coverage(off)]
    fn resume(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_docker(&["unpause", id])?;
        Ok(())
    }

    #[coverage(off)]
    fn import(&self, _box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError> {
        Ok(vm_name.to_string())
    }

    #[coverage(off)]
    fn clone_machine(
        &self,
        _base_machine_id: &str,
        vm_name: &str,
    ) -> Result<String, MigratoryError> {
        Ok(vm_name.to_string())
    }
}

impl DockerProvider {
    /// Streams or fetches logs from the Docker container.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if fetching logs fails.
    pub fn docker_logs(&self) -> Result<String, MigratoryError> {
        let id = self.require_id()?;
        execute_docker(&["logs", id])
    }

    /// Executes an arbitrary command inside the running container.
    ///
    /// # Arguments
    ///
    /// * `cmd` - The command and arguments to execute.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if execution fails.
    pub fn docker_exec(&self, cmd: &[&str]) -> Result<String, MigratoryError> {
        let id = self.require_id()?;
        let mut args = vec!["exec", id];
        args.extend_from_slice(cmd);
        execute_docker(&args)
    }

    /// Kills the running container immediately.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if killing the container fails.
    pub fn docker_kill(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_docker(&["kill", id])?;
        Ok(())
    }

    /// Commits the container state into a new Docker image.
    ///
    /// # Arguments
    ///
    /// * `image_name` - Tag name for the new image.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if commit fails.
    pub fn docker_commit(&self, image_name: &str) -> Result<String, MigratoryError> {
        let id = self.require_id()?;
        execute_docker(&["commit", id, image_name])
    }

    /// Launches a Docker Compose stack defined by `compose_file`.
    ///
    /// # Arguments
    ///
    /// * `compose_file` - Path to docker-compose.yml.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if execution fails.
    pub fn docker_compose_up(&self, compose_file: &std::path::Path) -> Result<(), MigratoryError> {
        let file_str = compose_file.to_string_lossy();
        execute_docker(&["compose", "-f", &file_str, "up", "-d"])?;
        Ok(())
    }

    /// Tears down a Docker Compose stack defined by `compose_file`.
    ///
    /// # Arguments
    ///
    /// * `compose_file` - Path to docker-compose.yml.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if execution fails.
    pub fn docker_compose_down(
        &self,
        compose_file: &std::path::Path,
    ) -> Result<(), MigratoryError> {
        let file_str = compose_file.to_string_lossy();
        execute_docker(&["compose", "-f", &file_str, "down"])?;
        Ok(())
    }

    /// Checks the Docker daemon connection via socket or DOCKER_HOST.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if daemon is reachable.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if checking fails.
    pub fn check_daemon_connection() -> Result<bool, MigratoryError> {
        Self::check_daemon_connection_with_sock(std::path::Path::new("/var/run/docker.sock"))
    }

    /// Checks the Docker daemon connection via socket or DOCKER_HOST with a custom socket path.
    ///
    /// # Arguments
    ///
    /// * `sock` - Path to docker socket.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if daemon is reachable.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if checking fails.
    pub fn check_daemon_connection_with_sock(
        sock: &std::path::Path,
    ) -> Result<bool, MigratoryError> {
        if let Ok(host) = std::env::var("DOCKER_HOST")
            && !host.is_empty()
        {
            return Ok(true);
        }
        if sock.exists() {
            return Ok(true);
        }
        let out = execute_docker(&["info"])?;
        Ok(!out.is_empty())
    }

    /// Pulls a Docker image from a registry.
    ///
    /// # Arguments
    ///
    /// * `image` - Image tag or repository name.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if pull fails.
    pub fn pull_image(image: &str) -> Result<(), MigratoryError> {
        execute_docker(&["pull", image])?;
        Ok(())
    }

    /// Starts a stopped container.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if start fails.
    pub fn start(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        execute_docker(&["start", id])?;
        Ok(())
    }
}

/// Executes a safe docker command.
#[coverage(off)]
fn execute_docker(args: &[&str]) -> Result<String, MigratoryError> {
    execute_docker_inner("docker", args)
}

#[coverage(off)]
fn execute_docker_inner(cmd_name: &str, args: &[&str]) -> Result<String, MigratoryError> {
    if std::env::var("MIGRATORY_TEST_MOCK_DOCKER").is_ok() {
        if std::env::var("MIGRATORY_TEST_MOCK_DOCKER_ERROR").is_ok() {
            return Err(MigratoryError::Generic("Mock Docker error".to_string()));
        }
        return Ok("mock docker output".to_string());
    }

    let output = Command::new(cmd_name)
        .args(args)
        .output()
        .map_err(|e| MigratoryError::Generic(format!("Failed to execute {}: {}", cmd_name, e)))?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        return Err(MigratoryError::Generic(format!(
            "{} error: {}",
            cmd_name, err_msg
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_provider() {
        let provider = DockerProvider::new(Some("test-id".to_string()));
        assert_eq!(provider.name(), "docker");

        let provider_no_id = DockerProvider::new(None);
        assert!(provider_no_id.halt().is_err());
        assert!(provider_no_id.destroy().is_err());
        assert!(provider_no_id.suspend().is_err());
        assert!(provider_no_id.resume().is_err());
        assert_eq!(
            provider_no_id.status().expect("operation should succeed"),
            "not created"
        );
    }

    #[coverage(off)]
    fn restore_docker_host(orig_host: Option<String>) {
        unsafe {
            if let Some(h) = orig_host {
                std::env::set_var("DOCKER_HOST", h);
            } else {
                std::env::remove_var("DOCKER_HOST");
            }
        }
    }

    #[test]
    #[coverage(off)]
    fn test_execute_docker_missing_cmd() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let result = execute_docker_inner("nonexistent-docker-cmd", &["--version"]);
        assert!(matches!(result, Err(MigratoryError::Generic(_))));
    }

    #[test]
    fn test_docker_deep_features() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER", "1");
        }

        let provider = DockerProvider::new(Some("container-123".to_string()));
        assert!(provider.docker_logs().is_ok());
        assert!(provider.docker_exec(&["echo", "hi"]).is_ok());
        assert!(provider.docker_kill().is_ok());
        assert!(provider.docker_commit("myimage:v1").is_ok());
        assert!(provider.start().is_ok());
        assert!(DockerProvider::check_daemon_connection().is_ok());
        assert!(DockerProvider::pull_image("alpine:latest").is_ok());
        assert!(
            provider
                .docker_compose_up(std::path::Path::new("docker-compose.yml"))
                .is_ok()
        );
        assert!(
            provider
                .docker_compose_down(std::path::Path::new("docker-compose.yml"))
                .is_ok()
        );

        let mut config = VmConfig::default();
        let mut d_opts = std::collections::HashMap::new();
        d_opts.insert("has_init".to_string(), "true".to_string());
        d_opts.insert("cmd".to_string(), "/sbin/init".to_string());
        config.providers.push(crate::config::ProviderConfig {
            name: "docker".to_string(),
            options: d_opts,
        });
        assert!(provider.up(&config).is_ok());

        let no_id = DockerProvider::new(None);
        assert!(no_id.docker_logs().is_err());
        assert!(no_id.docker_exec(&["ls"]).is_err());
        assert!(no_id.docker_kill().is_err());
        assert!(no_id.docker_commit("img").is_err());
        assert!(no_id.start().is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER");
        }
    }

    /// Tests all Docker provider operations when docker execution returns an error.
    #[test]
    fn test_docker_errors_with_mock_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER_ERROR", "1");
        }

        let provider = DockerProvider::new(Some("container-123".to_string()));
        assert!(provider.docker_kill().is_err());
        assert!(provider.docker_commit("myimage:v1").is_err());
        assert!(provider.start().is_err());
        assert!(
            provider
                .docker_compose_up(std::path::Path::new("docker-compose.yml"))
                .is_err()
        );
        assert!(
            provider
                .docker_compose_down(std::path::Path::new("docker-compose.yml"))
                .is_err()
        );
        assert!(DockerProvider::pull_image("alpine:latest").is_err());
        assert!(
            DockerProvider::check_daemon_connection_with_sock(std::path::Path::new(
                "/nonexistent/sock"
            ))
            .is_err()
        );

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER");
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER_ERROR");
        }
    }

    /// Tests branches of check_daemon_connection_with_sock.
    #[test]
    fn test_docker_check_daemon_connection_branches() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let orig_host = std::env::var("DOCKER_HOST").ok();

        // 1. DOCKER_HOST is set and not empty
        unsafe {
            std::env::set_var("DOCKER_HOST", "tcp://localhost:2375");
        }
        assert!(
            DockerProvider::check_daemon_connection_with_sock(std::path::Path::new(
                "/nonexistent/sock"
            ))
            .expect("operation should succeed")
        );

        // 2. DOCKER_HOST is empty, sock exists
        unsafe {
            std::env::set_var("DOCKER_HOST", "");
        }
        let temp = tempfile::NamedTempFile::new().expect("tempfile failed");
        assert!(
            DockerProvider::check_daemon_connection_with_sock(temp.path())
                .expect("operation should succeed")
        );

        // 3. Fallback to execute_docker with DOCKER_HOST removed
        unsafe {
            std::env::remove_var("DOCKER_HOST");
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER", "1");
        }
        assert!(
            DockerProvider::check_daemon_connection_with_sock(std::path::Path::new(
                "/nonexistent/sock"
            ))
            .expect("operation should succeed")
        );

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER");
        }
        restore_docker_host(orig_host);
    }
}
