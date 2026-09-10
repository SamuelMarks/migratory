//! Docker provisioner implementation.

use super::Provisioner;
use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::collections::HashMap;

/// Docker provisioner.
pub struct DockerProvisioner {
    images: Vec<String>,
    run: Option<String>,
    compose: Option<String>,
    install: bool,
    build_image: Option<String>,
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
        if let Some(images) = config.get("images") {
            self.images = images
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        self.run = config.get("run").cloned();
        self.compose = config.get("compose").cloned();
        if let Some(inst) = config.get("install") {
            self.install = inst.to_lowercase() == "true" || inst == "1";
        }
        self.build_image = config.get("build_image").cloned();
        self.build_path = config.get("build_path").cloned();
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
    use std::path::Path;
    use std::time::Duration;

    struct MockComm;

    impl Communicator for MockComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            Ok("".to_string())
        }
        #[coverage(off)]
        fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn execute_interactive(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn wait_for_ready(&self, _timeout: Duration) -> Result<(), MigratoryError> {
            Ok(())
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

    struct FailingComm;

    impl Communicator for FailingComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            Err(MigratoryError::Generic("Command failed".to_string()))
        }
        #[coverage(off)]
        fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn execute_interactive(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn wait_for_ready(&self, _timeout: Duration) -> Result<(), MigratoryError> {
            Ok(())
        }
    }

    /// Tests failure when installing docker.
    #[test]
    fn test_docker_provisioner_install_failure() {
        let mut prov = DockerProvisioner::default();
        prov.install = true;
        let comm = FailingComm;
        assert!(prov.provision(&comm).is_err());
    }

    /// Tests failure when pulling an image.
    #[test]
    fn test_docker_provisioner_pull_failure() {
        let mut prov = DockerProvisioner::default();
        prov.install = false;
        prov.images = vec!["alpine".to_string()];
        let comm = FailingComm;
        assert!(prov.provision(&comm).is_err());
    }

    /// Tests failure and default path when building an image.
    #[test]
    fn test_docker_provisioner_build_failure() {
        let mut prov = DockerProvisioner::default();
        prov.install = false;
        prov.build_image = Some("myapp".to_string());
        prov.build_path = None; // covers unwrap_or(".")
        let comm = FailingComm;
        assert!(prov.provision(&comm).is_err());

        // Also test build success with default path
        let ok_comm = MockComm;
        assert!(prov.provision(&ok_comm).is_ok());
    }

    /// Tests failure when running a container.
    #[test]
    fn test_docker_provisioner_run_failure() {
        let mut prov = DockerProvisioner::default();
        prov.install = false;
        prov.run = Some("-d nginx".to_string());
        let comm = FailingComm;
        assert!(prov.provision(&comm).is_err());
    }

    /// Tests failure when executing compose.
    #[test]
    fn test_docker_provisioner_compose_failure() {
        let mut prov = DockerProvisioner::default();
        prov.install = false;
        prov.compose = Some("docker-compose.yml".to_string());
        let comm = FailingComm;
        assert!(prov.provision(&comm).is_err());
    }
}
