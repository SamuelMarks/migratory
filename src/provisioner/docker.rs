//! Docker provisioner implementation.

use super::Provisioner;
use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::collections::HashMap;

/// Docker provisioner.
pub struct DockerProvisioner {
    /// Container images to pull via `docker pull`.
    images: Vec<String>,
    /// Container image or command specification to run via `docker run -d`.
    run: Option<String>,
    /// Path to a `docker-compose.yml` file to bring up via `docker compose`.
    compose: Option<String>,
    /// Whether to install the Docker engine on the guest if not already present.
    install: bool,
    /// Image tag name to build when `build_path` is specified.
    build_image: Option<String>,
    /// Guest directory path containing a `Dockerfile` to build.
    build_path: Option<String>,
}

impl DockerProvisioner {
    /// Creates a new docker provisioner.
    pub fn new() -> Self {
        Self {
            images: Vec::new(),
            run: None,
            compose: None,
            install: true,
            build_image: None,
            build_path: None,
        }
    }
}

impl Default for DockerProvisioner {
    fn default() -> Self {
        Self::new()
    }
}

impl Provisioner for DockerProvisioner {
    fn name(&self) -> &str {
        "docker"
    }

    fn prepare(&mut self, config: &HashMap<String, String>) -> Result<(), MigratoryError> {
        let p_cfg = crate::config::ProvisionerConfig {
            config: config.clone(),
            ..Default::default()
        };
        if let Ok(crate::config::options::TypedProvisionerConfig::Docker { images, .. }) =
            p_cfg.as_docker()
            && !images.is_empty()
        {
            self.images = images;
        }
        if let Ok(crate::config::options::TypedProvisionerConfig::Docker { run, .. }) =
            p_cfg.as_docker()
        {
            self.run = run;
        }
        if let Ok(crate::config::options::TypedProvisionerConfig::Docker { compose, .. }) =
            p_cfg.as_docker()
        {
            self.compose = compose;
        }
        if let Ok(crate::config::options::TypedProvisionerConfig::Docker { install, .. }) =
            p_cfg.as_docker()
        {
            self.install = install;
        }

        if let Ok(crate::config::options::TypedProvisionerConfig::Docker { build_image, .. }) =
            p_cfg.as_docker()
        {
            self.build_image = build_image;
        }
        if let Ok(crate::config::options::TypedProvisionerConfig::Docker { build_path, .. }) =
            p_cfg.as_docker()
        {
            self.build_path = build_path;
        }
        Ok(())
    }

    fn provision(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        if self.install {
            let install_script = "
            if ! command -v docker > /dev/null 2>&1; then
                echo 'Installing Docker...'
                if command -v curl > /dev/null 2>&1; then
                    curl -fsSL https://get.docker.com -o /tmp/get-docker.sh
                elif command -v wget > /dev/null 2>&1; then
                    wget -qO /tmp/get-docker.sh https://get.docker.com
                fi
                if [ -f /tmp/get-docker.sh ]; then
                    sudo sh /tmp/get-docker.sh
                    rm -f /tmp/get-docker.sh
                fi
                if command -v systemctl > /dev/null 2>&1; then
                    sudo systemctl enable --now docker || true
                elif command -v service > /dev/null 2>&1; then
                    sudo service docker start || true
                fi
                sudo usermod -aG docker $USER || true
            fi
            ";
            comm.execute(install_script)?;
        }

        for image in &self.images {
            comm.execute(&format!(
                "sudo docker pull '{}'",
                image.replace('\'', "'\\''")
            ))?;
        }

        if let Some(b_img) = &self.build_image {
            let b_path = self.build_path.as_deref().unwrap_or(".");
            comm.execute(&format!(
                "sudo docker build -t '{}' '{}'",
                b_img.replace('\'', "'\\''"),
                b_path.replace('\'', "'\\''")
            ))?;
        }

        if let Some(run_cmd) = &self.run {
            comm.execute(&format!("sudo docker run -d {}", run_cmd))?;
        }

        if let Some(compose_file) = &self.compose {
            comm.execute(&format!(
                "if docker compose version >/dev/null 2>&1; then sudo docker compose -f '{}' up -d; else sudo docker-compose -f '{}' up -d; fi",
                compose_file.replace('\'', "'\\''"),
                compose_file.replace('\'', "'\\''")
            ))?;
        }

        Ok(())
    }

    fn cleanup(&self) -> Result<(), MigratoryError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::communicator::Communicator;

    /// No-op mock communicator for Docker provisioner tests.
    struct MockComm;

    impl Communicator for MockComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            Ok(String::new())
        }
    }

    #[test]
    fn test_docker_provisioner_success() {
        let mut prov = DockerProvisioner::default();
        let mut config = HashMap::new();
        config.insert("images".to_string(), "ubuntu,, alpine".to_string());
        config.insert("run".to_string(), "--name my-container ubuntu".to_string());
        config.insert("compose".to_string(), "docker-compose.yml".to_string());
        config.insert("install".to_string(), "false".to_string());
        config.insert("build_image".to_string(), "myapp:latest".to_string());
        config.insert("build_path".to_string(), "/path/to/app".to_string());

        assert_eq!(prov.name(), "docker");
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
        assert!(prov.cleanup().is_ok());

        assert_eq!(prov.images.len(), 2);
        assert_eq!(prov.compose.as_deref(), Some("docker-compose.yml"));
        assert!(!prov.install);
        assert_eq!(prov.build_image.as_deref(), Some("myapp:latest"));

        // Test install="1"
        let mut config_one = HashMap::new();
        config_one.insert("install".to_string(), "1".to_string());
        assert!(prov.prepare(&config_one).is_ok());
        assert!(prov.install);

        // Test install="true"
        let mut config_true = HashMap::new();
        config_true.insert("install".to_string(), "true".to_string());
        assert!(prov.prepare(&config_true).is_ok());
        assert!(prov.install);
    }

    #[test]
    fn test_docker_provisioner_empty() {
        let mut prov = DockerProvisioner::default();
        let config = HashMap::new();

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
        assert!(prov.install);
    }

    /// Failing mock communicator for Docker provisioner error tests.
    struct FailingComm;

    impl Communicator for FailingComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            Err(MigratoryError::Generic("Command failed".to_string()))
        }
    }

    /// Tests failure when installing docker.
    #[test]
    fn test_docker_provisioner_install_failure() {
        let prov = DockerProvisioner {
            install: true,
            ..Default::default()
        };
        let comm = FailingComm;
        assert!(prov.provision(&comm).is_err());
    }

    /// Tests failure when pulling an image.
    #[test]
    fn test_docker_provisioner_pull_failure() {
        let prov = DockerProvisioner {
            install: false,
            images: vec!["alpine".to_string()],
            ..Default::default()
        };
        let comm = FailingComm;
        assert!(prov.provision(&comm).is_err());
    }

    /// Tests failure and default path when building an image.
    #[test]
    fn test_docker_provisioner_build_failure() {
        let prov = DockerProvisioner {
            install: false,
            build_image: Some("myapp".to_string()),
            build_path: None, // covers unwrap_or(".")
            ..Default::default()
        };
        let comm = FailingComm;
        assert!(prov.provision(&comm).is_err());

        // Also test build success with default path
        let ok_comm = MockComm;
        assert!(prov.provision(&ok_comm).is_ok());
    }

    /// Tests failure when running a container.
    #[test]
    fn test_docker_provisioner_run_failure() {
        let prov = DockerProvisioner {
            install: false,
            run: Some("-d nginx".to_string()),
            ..Default::default()
        };
        let comm = FailingComm;
        assert!(prov.provision(&comm).is_err());
    }

    /// Tests failure when executing compose.
    #[test]
    fn test_docker_provisioner_compose_failure() {
        let prov = DockerProvisioner {
            install: false,
            compose: Some("docker-compose.yml".to_string()),
            ..Default::default()
        };
        let comm = FailingComm;
        assert!(prov.provision(&comm).is_err());
    }
}
