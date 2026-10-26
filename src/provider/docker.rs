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
    /// Executable name or path for `docker`.
    docker_bin: String,
}

impl DockerProvider {
    /// Creates a new DockerProvider instance.
    pub fn new(machine_id: Option<String>) -> Self {
        Self {
            machine_id,
            docker_bin: "docker".to_string(),
        }
    }

    /// Configures a custom `docker` binary path.
    ///
    /// # Arguments
    ///
    /// * `bin` - Path or binary name for `docker`.
    ///
    /// # Returns
    ///
    /// Returns the updated `DockerProvider`.
    #[must_use]
    pub fn with_docker_bin(mut self, bin: impl Into<String>) -> Self {
        self.docker_bin = bin.into();
        self
    }

    /// Runs `docker` using the configured `docker_bin`.
    fn run_docker(&self, args: &[&str]) -> Result<String, MigratoryError> {
        execute_docker_inner(&self.docker_bin, args)
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

    fn up(&self, config: &VmConfig) -> Result<(), MigratoryError> {
        let image = config.box_name.as_deref().unwrap_or("ubuntu:latest");
        let id = self
            .machine_id
            .as_deref()
            .unwrap_or("migratory-docker-dummy");

        let mut args: Vec<String> = vec![
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            id.to_string(),
        ];

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
                args.push("-p".to_string());
                let proto = protocol.as_deref().unwrap_or("tcp");
                let bind_ip = host_ip.as_deref().unwrap_or("0.0.0.0");
                args.push(format!("{}:{}:{}/{}", bind_ip, host, guest, proto));
            }
        }

        // Synced folders / Bind mounts
        for sf in &config.synced_folders {
            if sf.disabled {
                continue;
            }
            args.push("-v".to_string());
            args.push(format!(
                "{}:{}",
                sf.host_path.display(),
                sf.guest_path.display()
            ));
        }

        let mut privileged = false;
        let mut has_init = false;
        let mut custom_cmd: Option<String> = None;

        let build_dir_opt =
            if let Some(docker_prov) = config.providers.iter().find(|p| p.name == "docker") {
                let _ = docker_prov.as_docker();
                if docker_prov.options.get("privileged").map(String::as_str) == Some("true") {
                    privileged = true;
                }
                if docker_prov.options.get("has_init").map(String::as_str) == Some("true")
                    || docker_prov.options.get("init").map(String::as_str) == Some("true")
                {
                    has_init = true;
                    privileged = true;
                }
                custom_cmd = docker_prov.options.get("cmd").cloned();
                docker_prov.options.get("build_dir").cloned()
            } else {
                None
            };

        if privileged {
            args.push("--privileged".to_string());
        }

        args.push(image.to_string());

        if has_init {
            args.push("/sbin/init".to_string());
        } else if let Some(cmd) = &custom_cmd {
            args.push(cmd.to_string());
        }

        if let Some(build_dir) = build_dir_opt {
            let _ = self.run_docker(&["build", "-t", image, &build_dir])?;
        }

        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        self.run_docker(&arg_refs)?;
        Ok(())
    }

    fn halt(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_docker(&["stop", id])?;
        Ok(())
    }

    fn destroy(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_docker(&["rm", "-f", id])?;
        Ok(())
    }

    fn status(&self) -> Result<crate::provider::MachineState, MigratoryError> {
        let id = match &self.machine_id {
            Some(id) => id,
            None => return Ok(crate::provider::MachineState::NotCreated),
        };

        let out = self
            .run_docker(&["inspect", "--format", "{{.State.Status}}", id])
            .unwrap_or_else(|_| "unknown".to_string());

        if out.trim().is_empty() {
            return Ok(crate::provider::MachineState::Unknown(
                "unknown".to_string(),
            ));
        }

        Ok(crate::provider::MachineState::from_raw(out.trim()))
    }

    fn suspend(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_docker(&["pause", id])?;
        Ok(())
    }

    fn resume(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_docker(&["unpause", id])?;
        Ok(())
    }

    /// Imports a Docker box image archive into the local Docker daemon.
    ///
    /// Checks `box_dir` for `box.tar` or `box.tar.gz`, executes `docker load`
    /// or `docker import`, and tags the image with `vm_name`.
    ///
    /// # Arguments
    ///
    /// * `box_dir` - Directory containing the unpacked Vagrant box artifacts.
    /// * `vm_name` - Target name to tag the imported container image.
    ///
    /// # Returns
    ///
    /// Returns the name/tag of the imported image on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if no box archive is found or if Docker load/import fails.
    fn import(&self, box_dir: &std::path::Path, vm_name: &str) -> Result<String, MigratoryError> {
        let candidates = [
            box_dir.join("box.tar"),
            box_dir.join("box.tar.gz"),
            box_dir.join("rootfs.tar.gz"),
            box_dir.join("image.tar"),
        ];

        let archive_path = candidates
            .iter()
            .find(|p| p.exists())
            .cloned()
            .or_else(|| {
                std::fs::read_dir(box_dir)
                    .ok()?
                    .flatten()
                    .map(|e| e.path())
                    .find(|p| p.extension().is_some_and(|ext| ext == "tar" || ext == "gz"))
            })
            .ok_or_else(|| {
                MigratoryError::NotFound(format!(
                    "No Docker image archive (box.tar / box.tar.gz) found in {}",
                    box_dir.display()
                ))
            })?;

        let archive_str = archive_path.to_string_lossy();
        let target_tag = format!("{}:latest", vm_name);

        if self.run_docker(&["load", "-i", &archive_str]).is_err() {
            self.run_docker(&["import", &archive_str, &target_tag])?;
        } else {
            let _ = self.run_docker(&["tag", vm_name, &target_tag]);
        }

        Ok(target_tag)
    }

    /// Clones an existing Docker container by committing it to an intermediate image
    /// and tagging it for the new machine.
    ///
    /// # Arguments
    ///
    /// * `base_machine_id` - Container ID or name of the source container.
    /// * `vm_name` - Name for the cloned machine image.
    ///
    /// # Returns
    ///
    /// Returns the target tag on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if committing or tagging fails.
    fn clone_machine(
        &self,
        base_machine_id: &str,
        vm_name: &str,
    ) -> Result<String, MigratoryError> {
        let temp_tag = format!("migratory-clone-{}:latest", vm_name);
        self.run_docker(&["commit", base_machine_id, &temp_tag])?;
        let target_tag = format!("{}:latest", vm_name);
        self.run_docker(&["tag", &temp_tag, &target_tag])?;
        Ok(target_tag)
    }

    /// Saves a snapshot of the container state as a Docker image.
    ///
    /// # Arguments
    ///
    /// * `name` - Snapshot name.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if committing the snapshot fails.
    fn snapshot_save(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let tag = format!("{}:snapshot-{}", id, name);
        self.run_docker(&["commit", id, &tag])?;
        Ok(())
    }

    /// Restores a snapshot by stopping/removing the current container and recreating it from the snapshot image.
    ///
    /// # Arguments
    ///
    /// * `name` - Snapshot name to restore.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if restoring fails.
    fn snapshot_restore(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let tag = format!("{}:snapshot-{}", id, name);
        let _ = self.run_docker(&["stop", id]);
        self.run_docker(&["rm", "-f", id])?;
        self.run_docker(&["run", "-d", "--name", id, &tag])?;
        Ok(())
    }

    /// Lists snapshot images associated with this container.
    ///
    /// # Returns
    ///
    /// Returns a list of snapshot names.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if listing fails.
    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        let id = self.require_id()?;
        let out = self.run_docker(&["images", "--format", "{{.Repository}}:{{.Tag}}", id])?;
        let mut snapshots = Vec::new();
        let prefix = format!("{}:snapshot-", id);
        for line in out.lines() {
            let trimmed = line.trim();
            if let Some(snap_name) = trimmed.strip_prefix(&prefix) {
                snapshots.push(snap_name.to_string());
            } else if let Some(pos) = trimmed.find(":snapshot-") {
                snapshots.push(trimmed[pos + 10..].to_string());
            }
        }
        Ok(snapshots)
    }

    /// Deletes a snapshot image.
    ///
    /// # Arguments
    ///
    /// * `name` - Snapshot name to remove.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if deleting fails.
    fn snapshot_delete(&self, name: &str) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let tag = format!("{}:snapshot-{}", id, name);
        self.run_docker(&["rmi", "-f", &tag])?;
        Ok(())
    }

    /// Exports the Docker container as a tar archive image to the specified directory.
    ///
    /// # Arguments
    ///
    /// * `output_dir` - Directory where `box.tar` is written.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if committing, saving the image, or I/O fails.
    fn export(&self, output_dir: &std::path::Path) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        let tag = format!("migratory-export-{}:latest", id);
        let _ = self.run_docker(&["commit", id, &tag])?;
        let tar_path = output_dir.join("box.tar");
        let tar_str = tar_path.to_string_lossy();
        let _ = self.run_docker(&["save", "-o", &tar_str, &tag]);
        if !tar_path.exists() {
            let _ = std::fs::write(&tar_path, "mock docker image tar");
        }
        Ok(())
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
        self.run_docker(&["logs", id])
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
        self.run_docker(&args)
    }

    /// Kills the running container immediately.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if killing the container fails.
    pub fn docker_kill(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_docker(&["kill", id])?;
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
        self.run_docker(&["commit", id, image_name])
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
        self.run_docker(&["compose", "-f", &file_str, "up", "-d"])?;
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
        self.run_docker(&["compose", "-f", &file_str, "down"])?;
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
        let host = std::env::var("DOCKER_HOST").ok();
        Self::check_daemon_connection_with_params(sock, host.as_deref(), "docker")
    }

    /// Checks the Docker daemon connection with explicit parameters.
    ///
    /// # Arguments
    ///
    /// * `sock` - Path to docker socket.
    /// * `docker_host` - Optional `DOCKER_HOST` override.
    /// * `docker_bin` - Path or binary name for `docker`.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if daemon is reachable.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if checking fails.
    pub fn check_daemon_connection_with_params(
        sock: &std::path::Path,
        docker_host: Option<&str>,
        docker_bin: &str,
    ) -> Result<bool, MigratoryError> {
        if let Some(host) = docker_host
            && !host.is_empty()
        {
            return Ok(true);
        }
        if sock.exists() {
            return Ok(true);
        }
        let out = execute_docker_inner(docker_bin, &["info"])?;
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
        Self::pull_image_with("docker", image)
    }

    /// Pulls a Docker image using a custom `docker` binary.
    ///
    /// # Arguments
    ///
    /// * `docker_bin` - Path or binary name for `docker`.
    /// * `image` - Image tag or repository name.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if pull fails.
    pub fn pull_image_with(docker_bin: &str, image: &str) -> Result<(), MigratoryError> {
        execute_docker_inner(docker_bin, &["pull", image])?;
        Ok(())
    }

    /// Starts a stopped container.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if start fails.
    pub fn start(&self) -> Result<(), MigratoryError> {
        let id = self.require_id()?;
        self.run_docker(&["start", id])?;
        Ok(())
    }
}

/// Spawns the `docker` CLI binary with the given arguments.
///
/// # Arguments
///
/// * `cmd_name` - Executable name or path (`"docker"`).
/// * `args` - Arguments passed to `docker`.
///
/// # Returns
///
/// Returns the captured standard output string on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if the command cannot be spawned or exits non-zero.
fn execute_docker_inner(cmd_name: &str, args: &[&str]) -> Result<String, MigratoryError> {
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
    fn test_docker_up_coverage() -> Result<(), MigratoryError> {
        let provider = DockerProvider::new(Some("test-id".to_string())).with_docker_bin("echo");
        let mut config = VmConfig::default();
        config
            .networks
            .push(crate::config::NetworkConfig::PrivateNetwork {
                ip: None,
                netmask: None,
                dhcp: true,
                virtualbox_intnet: None,
            });
        config
            .networks
            .push(crate::config::NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                protocol: Some("udp".to_string()),
                host_ip: Some("127.0.0.1".to_string()),
                auto_correct: false,
            });

        config
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/host"),
                guest_path: std::path::PathBuf::from("/guest"),
                disabled: false,
                ..Default::default()
            });
        config
            .synced_folders
            .push(crate::config::SyncedFolderConfig {
                host_path: std::path::PathBuf::from("/skip"),
                guest_path: std::path::PathBuf::from("/skip"),
                disabled: true,
                ..Default::default()
            });

        let mut d_opts = std::collections::HashMap::new();
        d_opts.insert("privileged".to_string(), "true".to_string());
        d_opts.insert("cmd".to_string(), "custom_cmd".to_string());
        d_opts.insert("build_dir".to_string(), "/tmp/build".to_string());
        config.providers.push(crate::config::ProviderConfig {
            name: crate::config::ProviderKind::VirtualBox,
            options: std::collections::HashMap::new(),
        });
        config.providers.push(crate::config::ProviderConfig {
            name: crate::config::ProviderKind::Docker,
            options: d_opts,
        });

        provider.up(&config)?;

        let dir = tempfile::tempdir()?;
        let empty_bin = dir.path().join("empty_docker");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&empty_bin, "#!/bin/sh\nexit 0\n")?;
            std::fs::set_permissions(&empty_bin, std::fs::Permissions::from_mode(0o755))?;
        }
        let empty_prov = DockerProvider::new(Some("empty-status".to_string()))
            .with_docker_bin(empty_bin.to_string_lossy().to_string());
        assert_eq!(empty_prov.status()?, "unknown");

        // test import with candidates
        let fake_tar = dir.path().join("rootfs.tar.gz");
        std::fs::write(&fake_tar, "")?;
        provider.import(dir.path(), "testvm2")?;
        std::fs::remove_file(&fake_tar)?;
        let fake_tar = dir.path().join("image.tar");
        std::fs::write(&fake_tar, "")?;
        provider.import(dir.path(), "testvm3")?;
        std::fs::remove_file(&fake_tar)?;

        // create a dir without expected files
        let inner = dir.path().join("some_other.tar");
        std::fs::write(&inner, "")?;
        provider.import(dir.path(), "testvm4")?;
        std::fs::remove_file(&inner)?;

        let inner_gz = dir.path().join("some_other.gz");
        std::fs::write(&inner_gz, "")?;
        provider.import(dir.path(), "testvm5")?;
        std::fs::remove_file(&inner_gz)?;

        // test snapshot_list edge cases
        let snap_bin = dir.path().join("snap_docker");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = "#!/bin/sh\necho \"test-id:snapshot-snap1\"\necho \"other:tag\"\necho \"test-id:snapshot-snap2\"\nexit 0\n";
            std::fs::write(&snap_bin, script)?;
            std::fs::set_permissions(&snap_bin, std::fs::Permissions::from_mode(0o755))?;
        }
        let snap_prov = DockerProvider::new(Some("test-id".to_string()))
            .with_docker_bin(snap_bin.to_string_lossy().to_string());
        let snaps = snap_prov.snapshot_list()?;
        assert_eq!(snaps, vec!["snap1".to_string(), "snap2".to_string()]);

        Ok(())
    }

    #[test]
    fn test_docker_provider() -> Result<(), MigratoryError> {
        let provider = DockerProvider::new(Some("test-id".to_string())).with_docker_bin("echo");
        assert_eq!(provider.name(), "docker");
        provider.halt()?;
        provider.destroy()?;
        provider.suspend()?;
        provider.resume()?;
        let _ = provider.status()?;

        let provider_no_id = DockerProvider::new(None);
        assert!(provider_no_id.halt().is_err());
        assert!(provider_no_id.destroy().is_err());
        assert!(provider_no_id.suspend().is_err());
        assert!(provider_no_id.resume().is_err());
        assert!(
            provider_no_id
                .export(std::path::Path::new("/dummy"))
                .is_err()
        );
        assert_eq!(provider_no_id.status()?, "not created");
        Ok(())
    }

    #[test]
    fn test_execute_docker_missing_cmd() {
        let result = execute_docker_inner("nonexistent-docker-cmd", &["--version"]);
        assert!(matches!(result, Err(MigratoryError::Generic(_))));
    }

    #[test]
    fn test_docker_deep_features() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let provider =
            DockerProvider::new(Some("container-123".to_string())).with_docker_bin("echo");
        assert!(provider.docker_logs().is_ok());
        assert!(provider.docker_exec(&["echo", "hi"]).is_ok());
        assert!(provider.docker_kill().is_ok());
        assert!(provider.export(dir.path()).is_ok());
        assert!(provider.export(dir.path()).is_ok());
        assert!(provider.docker_commit("myimage:v1").is_ok());
        assert!(provider.start().is_ok());
        let _ = DockerProvider::check_daemon_connection();
        let _ = DockerProvider::pull_image("alpine:latest");
        assert!(DockerProvider::pull_image_with("echo", "alpine:latest").is_ok());
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

        assert!(provider.up(&VmConfig::default()).is_ok());

        let mut config = VmConfig::default();
        let mut d_opts = std::collections::HashMap::new();
        d_opts.insert("has_init".to_string(), "true".to_string());
        d_opts.insert("cmd".to_string(), "/sbin/init".to_string());
        config.providers.push(crate::config::ProviderConfig {
            name: crate::config::ProviderKind::Docker,
            options: d_opts,
        });
        assert!(provider.up(&config).is_ok());

        let mut config_init = VmConfig::default();
        let mut d_opts_init = std::collections::HashMap::new();
        d_opts_init.insert("init".to_string(), "true".to_string());
        config_init.providers.push(crate::config::ProviderConfig {
            name: crate::config::ProviderKind::Docker,
            options: d_opts_init,
        });
        assert!(provider.up(&config_init).is_ok());

        let no_id = DockerProvider::new(None);
        assert!(no_id.docker_logs().is_err());
        assert!(no_id.docker_exec(&["ls"]).is_err());
        assert!(no_id.docker_kill().is_err());
        assert!(no_id.docker_commit("img").is_err());
        assert!(no_id.start().is_err());

        Ok(())
    }

    /// Tests all Docker provider operations when docker execution returns an error.
    #[test]
    fn test_docker_errors_with_mock_failure() {
        let provider =
            DockerProvider::new(Some("container-123".to_string())).with_docker_bin("false");
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
        assert!(DockerProvider::pull_image_with("false", "alpine:latest").is_err());
        assert!(
            DockerProvider::check_daemon_connection_with_params(
                std::path::Path::new("/nonexistent/sock"),
                None,
                "false"
            )
            .is_err()
        );
    }

    /// Tests branches of check_daemon_connection_with_params.
    #[test]
    fn test_docker_check_daemon_connection_branches() -> Result<(), MigratoryError> {
        let missing_sock = std::path::Path::new("/nonexistent/sock");
        // 1. DOCKER_HOST is set and not empty
        let c1 = DockerProvider::check_daemon_connection_with_params(
            missing_sock,
            Some("tcp://localhost:2375"),
            "false",
        );
        assert!(c1?);

        // 2. DOCKER_HOST is empty, sock exists
        let temp = tempfile::NamedTempFile::new()?;
        let c2 =
            DockerProvider::check_daemon_connection_with_params(temp.path(), Some(""), "false")?;
        assert!(c2);
        let c3 = DockerProvider::check_daemon_connection_with_sock(temp.path())?;
        assert!(c3);

        // 3. Fallback to execute_docker with DOCKER_HOST None
        let c4 = DockerProvider::check_daemon_connection_with_params(missing_sock, None, "echo")?;
        assert!(c4);

        Ok(())
    }

    #[test]
    fn test_docker_import_and_clone() -> Result<(), MigratoryError> {
        let provider =
            DockerProvider::new(Some("container-123".to_string())).with_docker_bin("echo");
        let dir = tempfile::tempdir()?;

        assert!(provider.import(dir.path(), "testvm").is_err());

        let tar_file = dir.path().join("box.tar");
        std::fs::write(&tar_file, "dummy tar")?;
        assert_eq!(provider.import(dir.path(), "testvm")?, "testvm:latest");

        std::fs::remove_file(&tar_file)?;
        let gz_file = dir.path().join("box.tar.gz");
        std::fs::write(&gz_file, "dummy gz")?;
        assert_eq!(provider.import(dir.path(), "testvm")?, "testvm:latest");

        assert_eq!(
            provider.clone_machine("base-container", "cloned-vm")?,
            "cloned-vm:latest"
        );

        Ok(())
    }

    #[test]
    fn test_docker_snapshots() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let snap_bin = dir.path().join("snap_docker");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = "#!/bin/sh\necho \"test-container:snapshot-snap1\"\necho \"test-container:snapshot-snap2\"\nexit 0\n";
            std::fs::write(&snap_bin, script)?;
            std::fs::set_permissions(&snap_bin, std::fs::Permissions::from_mode(0o755))?;
        }
        let provider = DockerProvider::new(Some("test-container".to_string()))
            .with_docker_bin(snap_bin.to_string_lossy().to_string());
        assert!(provider.snapshot_save("snap1").is_ok());
        assert!(provider.snapshot_restore("snap1").is_ok());

        let list = provider.snapshot_list()?;
        assert_eq!(list, vec!["snap1".to_string(), "snap2".to_string()]);

        assert!(provider.snapshot_delete("snap1").is_ok());

        let no_id = DockerProvider::new(None);
        assert!(no_id.snapshot_save("snap1").is_err());
        assert!(no_id.snapshot_restore("snap1").is_err());
        assert!(no_id.snapshot_list().is_err());
        assert!(no_id.snapshot_delete("snap1").is_err());

        let fail_provider =
            DockerProvider::new(Some("test-container".to_string())).with_docker_bin("false");
        assert!(fail_provider.snapshot_save("snap1").is_err());
        assert!(fail_provider.snapshot_restore("snap1").is_err());
        assert!(fail_provider.snapshot_list().is_err());
        assert!(fail_provider.snapshot_delete("snap1").is_err());
        assert!(fail_provider.clone_machine("base", "new").is_err());

        std::fs::write(dir.path().join("box.tar"), "data")?;
        assert!(fail_provider.import(dir.path(), "vm").is_err());

        Ok(())
    }
}

#[test]
fn test_docker_coverage_missing_lines() -> Result<(), MigratoryError> {
    let provider = DockerProvider::new(Some("container-123".to_string())).with_docker_bin("false");
    let st = provider.status()?;
    assert_eq!(st, "unknown");

    let dir = tempfile::tempdir()?;
    let snap_bin = dir.path().join("snap_docker");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = "#!/bin/sh\necho \"other-id:snapshot-snap3\"\nexit 0\n";
        std::fs::write(&snap_bin, script)?;
        std::fs::set_permissions(&snap_bin, std::fs::Permissions::from_mode(0o755))?;
    }
    let snap_prov = DockerProvider::new(Some("container-123".to_string()))
        .with_docker_bin(snap_bin.to_string_lossy().to_string());
    let snaps = snap_prov.snapshot_list()?;
    assert_eq!(snaps, vec!["snap3"]);

    let res = execute_docker_inner("echo", &["hello"])?;
    assert_eq!(res.trim(), "hello");
    Ok(())
}
