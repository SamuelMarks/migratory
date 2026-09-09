//! Docker Communicator implementation.
//!
//! This module provides direct command and file execution transport
//! targeting running Docker containers via `docker exec` and `docker cp`.

use super::Communicator;
use crate::error::MigratoryError;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Communicator implementation for Docker containers.
///
/// Executes commands directly inside containers using `docker exec`
/// and transfers files using `docker cp`.
pub struct DockerCommunicator {
    /// Container identifier (ID or name).
    pub container_id: String,
    /// Path or command name for the Docker binary.
    pub docker_bin: String,
}

impl DockerCommunicator {
    /// Creates a new `DockerCommunicator` for the specified container.
    ///
    /// # Arguments
    ///
    /// * `container_id` - Container name or ID.
    ///
    /// # Returns
    ///
    /// Returns a new `DockerCommunicator` instance.
    pub fn new(container_id: impl Into<String>) -> Self {
        Self {
            container_id: container_id.into(),
            docker_bin: "docker".to_string(),
        }
    }

    /// Sets a custom path for the `docker` binary.
    ///
    /// # Arguments
    ///
    /// * `bin` - The binary path or command name.
    ///
    /// # Returns
    ///
    /// Returns `self` for builder chaining.
    pub fn with_docker_bin(mut self, bin: impl Into<String>) -> Self {
        self.docker_bin = bin.into();
        self
    }

    /// Internal helper to execute a docker CLI command.
    fn run_docker(&self, args: &[&str]) -> Result<String, MigratoryError> {
        if std::env::var("MIGRATORY_TEST_MOCK_DOCKER").is_ok() {
            if std::env::var("MIGRATORY_TEST_MOCK_DOCKER_ERROR").is_ok() {
                return Err(MigratoryError::Generic("Mock Docker error".to_string()));
            }
            return Ok("mock docker output".to_string());
        }

        self.run_docker_real(args)
    }

    #[coverage(off)]
    fn run_docker_real(&self, args: &[&str]) -> Result<String, MigratoryError> {
        let output = Command::new(&self.docker_bin)
            .args(args)
            .output()
            .map_err(|e| MigratoryError::Generic(format!("Failed to execute docker: {}", e)))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Err(MigratoryError::Generic(format!(
                "Docker command failed with status {}: {}",
                output.status, stderr
            )))
        }
    }

    #[coverage(off)]
    fn execute_interactive_real(&self) -> Result<(), MigratoryError> {
        let mut cmd = Command::new(&self.docker_bin);
        cmd.args(["exec", "-i", "-t", &self.container_id, "/bin/sh"]);
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let mut child = cmd
            .spawn()
            .map_err(|e| MigratoryError::Generic(format!("Failed to spawn docker exec: {}", e)))?;
        let status = child.wait().map_err(|e| {
            MigratoryError::Generic(format!("Failed to wait for docker exec: {}", e))
        })?;

        if status.success() {
            Ok(())
        } else {
            Err(MigratoryError::Generic(format!(
                "Interactive docker session exited with status: {}",
                status
            )))
        }
    }
}

impl Communicator for DockerCommunicator {
    /// Executes a command in the container via `docker exec -i`.
    ///
    /// # Arguments
    ///
    /// * `command` - Shell command string to run inside the container.
    ///
    /// # Returns
    ///
    /// Returns the captured standard output on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the command execution fails or exits non-zero.
    fn execute(&self, command: &str) -> Result<String, MigratoryError> {
        self.run_docker(&["exec", "-i", &self.container_id, "/bin/sh", "-c", command])
    }

    /// Copies a file or directory from the host to the container via `docker cp`.
    ///
    /// # Arguments
    ///
    /// * `local_path` - Path on the host filesystem.
    /// * `remote_path` - Destination path inside the container.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the copy operation fails.
    fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), MigratoryError> {
        let local_str = local_path.to_string_lossy();
        let target = format!("{}:{}", self.container_id, remote_path);
        self.run_docker(&["cp", &local_str, &target])?;
        Ok(())
    }

    /// Copies a file or directory from the container to the host via `docker cp`.
    ///
    /// # Arguments
    ///
    /// * `remote_path` - Path inside the container.
    /// * `local_path` - Destination path on the host filesystem.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the copy operation fails.
    fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), MigratoryError> {
        let local_str = local_path.to_string_lossy();
        let src = format!("{}:{}", self.container_id, remote_path);
        self.run_docker(&["cp", &src, &local_str])?;
        Ok(())
    }

    /// Starts an interactive shell session in the container via `docker exec -i -t`.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the session fails or exits with an error.
    fn execute_interactive(&self) -> Result<(), MigratoryError> {
        if std::env::var("MIGRATORY_TEST_MOCK_DOCKER").is_ok() {
            if std::env::var("MIGRATORY_TEST_MOCK_DOCKER_ERROR").is_ok() {
                return Err(MigratoryError::Generic(
                    "Mock interactive error".to_string(),
                ));
            }
            return Ok(());
        }

        self.execute_interactive_real()
    }

    /// Waits until the container is ready and can execute commands.
    ///
    /// # Arguments
    ///
    /// * `timeout` - Maximum duration to wait before timing out.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` when the container responds successfully.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the timeout is exceeded.
    fn wait_for_ready(&self, timeout: Duration) -> Result<(), MigratoryError> {
        let start = Instant::now();
        let sleep_duration = Duration::from_millis(200);

        while start.elapsed() < timeout {
            if self.execute("echo ok").is_ok() {
                return Ok(());
            }
            std::thread::sleep(sleep_duration);
        }

        Err(MigratoryError::Generic(
            "Timed out waiting for Docker container to be ready".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_docker_communicator_new_and_builder() {
        let comm = DockerCommunicator::new("my-container").with_docker_bin("/usr/bin/docker");
        assert_eq!(comm.container_id, "my-container");
        assert_eq!(comm.docker_bin, "/usr/bin/docker");
    }

    #[test]
    fn test_docker_communicator_mock_success() {
        let _guard = TEST_LOCK.lock().expect("operation should succeed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER", "1");
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER_ERROR");
        }

        let comm = DockerCommunicator::new("test-ctr");
        assert_eq!(
            comm.execute("uptime").unwrap_or_default(),
            "mock docker output"
        );

        let tmp = tempfile::tempdir().expect("operation should succeed");
        let src_file = tmp.path().join("src.txt");
        let _ = std::fs::write(&src_file, b"hello");

        assert!(comm.upload(&src_file, "/tmp/dst.txt").is_ok());
        assert!(comm.download("/tmp/dst.txt", &src_file).is_ok());
        assert!(comm.execute_interactive().is_ok());
        assert!(comm.wait_for_ready(Duration::from_secs(1)).is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER");
        }
    }

    #[test]
    fn test_docker_communicator_mock_error() {
        let _guard = TEST_LOCK.lock().expect("operation should succeed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER_ERROR", "1");
        }

        let comm = DockerCommunicator::new("test-ctr");
        assert!(comm.execute("uptime").is_err());

        let tmp = tempfile::tempdir().expect("operation should succeed");
        let src_file = tmp.path().join("src.txt");
        assert!(comm.upload(&src_file, "/tmp/dst.txt").is_err());
        assert!(comm.download("/tmp/dst.txt", &src_file).is_err());
        assert!(comm.execute_interactive().is_err());
        assert!(comm.wait_for_ready(Duration::from_millis(300)).is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER");
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER_ERROR");
        }
    }

    #[test]
    fn test_docker_communicator_real_execution_invalid_binary() {
        let _guard = TEST_LOCK.lock().expect("operation should succeed");
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER");
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER_ERROR");
        }

        let comm = DockerCommunicator::new("invalid-ctr").with_docker_bin("nonexistent_binary_xyz");
        assert!(comm.execute("echo test").is_err());
        assert!(comm.execute_interactive().is_err());
    }
}
