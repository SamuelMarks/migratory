//! Ansible provisioner implementation.

use super::Provisioner;
use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

/// Ansible provisioner.
pub struct AnsibleProvisioner {
    playbook: Option<String>,
    inventory_path: Option<String>,
    extra_vars: Option<String>,
    limit: Option<String>,
    tags: Option<String>,
    skip_tags: Option<String>,
    vault_password_file: Option<String>,
    galaxy_role_file: Option<String>,
    galaxy_command: Option<String>,
    roles_path: Option<String>,
    install: bool,
    mode: String,
    ssh_host: Option<String>,
    ssh_port: Option<String>,
    ssh_user: Option<String>,
    ssh_key: Option<String>,
    generated_inventory: Mutex<Option<String>>,
}

impl AnsibleProvisioner {
    /// Creates a new ansible provisioner.
    pub fn new() -> Self {
        Self {
            playbook: None,
            inventory_path: None,
            extra_vars: None,
            limit: None,
            tags: None,
            skip_tags: None,
            vault_password_file: None,
            galaxy_role_file: None,
            galaxy_command: None,
            roles_path: None,
            install: true,
            mode: "host".to_string(),
            ssh_host: None,
            ssh_port: None,
            ssh_user: None,
            ssh_key: None,
            generated_inventory: Mutex::new(None),
        }
    }
}

impl Default for AnsibleProvisioner {
    fn default() -> Self {
        Self::new()
    }
}

impl Provisioner for AnsibleProvisioner {
    fn name(&self) -> &str {
        "ansible"
    }

    fn prepare(&mut self, config: &HashMap<String, String>) -> Result<(), MigratoryError> {
        self.playbook = config.get("playbook").cloned();
        self.inventory_path = config.get("inventory_path").cloned();
        self.extra_vars = config.get("extra_vars").cloned();
        self.limit = config.get("limit").cloned();
        self.tags = config.get("tags").cloned();
        self.skip_tags = config.get("skip_tags").cloned();
        self.vault_password_file = config.get("vault_password_file").cloned();
        self.galaxy_role_file = config.get("galaxy_role_file").cloned();
        self.galaxy_command = config.get("galaxy_command").cloned();
        self.roles_path = config.get("roles_path").cloned();
        if let Some(inst) = config.get("install") {
            self.install = inst.to_lowercase() == "true" || inst == "1";
        }
        if let Some(m) = config.get("mode") {
            self.mode = m.to_lowercase();
        }
        self.ssh_host = config.get("ssh_host").cloned();
        self.ssh_port = config.get("ssh_port").cloned();
        self.ssh_user = config.get("ssh_user").cloned();
        self.ssh_key = config.get("ssh_key").cloned();
        Ok(())
    }

    fn provision(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let playbook = match &self.playbook {
            Some(p) => p,
            None => {
                return Err(MigratoryError::Validation(
                    "Ansible provisioner requires a 'playbook'".to_string(),
                ));
            }
        };

        if self.mode == "guest" || self.mode == "ansible_local" {
            // ansible_local mode
            if self.install {
                let install_cmd = r#"
                if ! command -v ansible-playbook >/dev/null 2>&1; then
                    echo 'Installing Ansible on guest...'
                    if command -v apt-get >/dev/null 2>&1; then
                        sudo apt-get update -y && sudo apt-get install -y ansible
                    elif command -v yum >/dev/null 2>&1; then
                        sudo yum install -y epel-release && sudo yum install -y ansible
                    elif command -v pacman >/dev/null 2>&1; then
                        sudo pacman -Sy --noconfirm ansible
                    elif command -v pip3 >/dev/null 2>&1; then
                        sudo pip3 install ansible
                    fi
                fi
                "#;
                comm.execute(install_cmd)?;
            }

            let local_path = Path::new(playbook);
            if !local_path.exists() {
                return Err(MigratoryError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Playbook not found: {}", playbook),
                )));
            }

            let file_name = local_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("playbook.yml");
            let remote_path = format!("/tmp/{}", file_name);
            comm.upload(local_path, &remote_path)?;

            // Sync roles if specified
            if let Some(roles) = &self.roles_path {
                let rpath = Path::new(roles);
                if rpath.exists() {
                    comm.execute("mkdir -p /tmp/roles")?;
                    comm.upload(rpath, "/tmp/roles")?;
                }
            }

            // Install galaxy roles on guest if role file is specified
            if let Some(role_file) = &self.galaxy_role_file {
                let rf_path = Path::new(role_file);
                if rf_path.exists() {
                    comm.upload(rf_path, "/tmp/galaxy_requirements.yml")?;
                    let gcmd = self
                        .galaxy_command
                        .as_deref()
                        .unwrap_or("ansible-galaxy install -r /tmp/galaxy_requirements.yml");
                    comm.execute(gcmd)?;
                }
            }

            let mut command = format!("ansible-playbook {}", remote_path);
            if let Some(inv) = &self.inventory_path {
                let inv_p = Path::new(inv);
                if inv_p.exists() {
                    comm.upload(inv_p, "/tmp/ansible_inventory")?;
                    command.push_str(" -i /tmp/ansible_inventory");
                } else {
                    command.push_str(&format!(" -i '{}'", inv.replace('\'', "'\\''")));
                }
            } else {
                command.push_str(" -i '127.0.0.1,' -c local");
            }

            if self.roles_path.is_some() {
                command.push_str(" --roles-path /tmp/roles");
            }
            if let Some(vars) = &self.extra_vars {
                command.push_str(&format!(" --extra-vars '{}'", vars.replace('\'', "'\\''")));
            }
            if let Some(limit) = &self.limit {
                command.push_str(&format!(" --limit '{}'", limit.replace('\'', "'\\''")));
            }
            if let Some(tags) = &self.tags {
                command.push_str(&format!(" --tags '{}'", tags.replace('\'', "'\\''")));
            }
            if let Some(skip_tags) = &self.skip_tags {
                command.push_str(&format!(
                    " --skip-tags '{}'",
                    skip_tags.replace('\'', "'\\''")
                ));
            }
            if let Some(vpf) = &self.vault_password_file {
                command.push_str(&format!(
                    " --vault-password-file '{}'",
                    vpf.replace('\'', "'\\''")
                ));
            }

            // Execute on guest
            let _ = comm.execute(&command)?;
            return Ok(());
        }

        // Host mode
        if !Path::new(playbook).exists() {
            return Err(MigratoryError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Playbook not found: {}", playbook),
            )));
        }

        // Run ansible-galaxy if galaxy_role_file is configured
        if let Some(role_file) = &self.galaxy_role_file {
            let mut gcmd = Command::new("ansible-galaxy");
            gcmd.args(["install", "-r", role_file]);
            self.execute_cmd(gcmd)?;
        }

        let mut cmd = Command::new("ansible-playbook");
        cmd.arg(playbook);

        let inv_path = if let Some(inv) = &self.inventory_path {
            inv.clone()
        } else {
            // Generate dynamic inventory
            let host = self.ssh_host.as_deref().unwrap_or("127.0.0.1");
            let port = self.ssh_port.as_deref().unwrap_or("22");
            let user = self.ssh_user.as_deref().unwrap_or("vagrant");
            let mut inventory_content =
                format!("{} ansible_port={} ansible_user={}", host, port, user);

            if let Some(key) = &self.ssh_key {
                inventory_content.push_str(&format!(" ansible_ssh_private_key_file={}", key));
            }

            let vagrant_dir = std::env::temp_dir().join(".vagrant");
            let inv_dir = vagrant_dir.join("provisioners/ansible/inventory");
            let _ = std::fs::create_dir_all(&inv_dir);
            let temp_path = inv_dir.join("vagrant_ansible_inventory");
            let temp_path_str = temp_path.to_string_lossy().to_string();

            let inv_content_full = format!("[default]\n{}\n", inventory_content);
            fs::write(&temp_path, inv_content_full).map_err(MigratoryError::Io)?;

            if let Ok(mut guard) = self.generated_inventory.lock() {
                *guard = Some(temp_path_str.clone());
            }
            temp_path_str
        };

        cmd.arg("-i").arg(inv_path);

        if let Some(vars) = &self.extra_vars {
            cmd.arg("--extra-vars").arg(vars);
        }
        if let Some(limit) = &self.limit {
            cmd.arg("--limit").arg(limit);
        }
        if let Some(tags) = &self.tags {
            cmd.arg("--tags").arg(tags);
        }
        if let Some(skip_tags) = &self.skip_tags {
            cmd.arg("--skip-tags").arg(skip_tags);
        }
        if let Some(vpf) = &self.vault_password_file {
            cmd.arg("--vault-password-file").arg(vpf);
        }

        self.execute_cmd(cmd)
    }

    fn cleanup(&self) -> Result<(), MigratoryError> {
        let path_opt = self
            .generated_inventory
            .lock()
            .ok()
            .and_then(|mut g| g.take());
        if let Some(path) = path_opt {
            let _ = fs::remove_file(path);
        }
        Ok(())
    }
}

impl AnsibleProvisioner {
    #[cfg(not(test))]
    #[coverage(off)]
    fn execute_cmd(&self, mut cmd: Command) -> Result<(), MigratoryError> {
        let status = cmd.status().map_err(MigratoryError::Io)?;
        if !status.success() {
            return Err(MigratoryError::Generic(format!(
                "Ansible playbook execution failed with exit code: {:?}",
                status.code()
            )));
        }
        Ok(())
    }

    #[cfg(test)]
    fn execute_cmd(&self, _cmd: Command) -> Result<(), MigratoryError> {
        if std::env::var("MIGRATORY_TEST_ANSIBLE_CMD_FAIL").is_ok() {
            return Err(MigratoryError::Generic("ansible cmd failed".to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::communicator::Communicator;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    #[derive(Default)]
    struct MockComm {
        fail_upload_path: Option<String>,
        fail_on_cmd: Option<String>,
    }

    impl Communicator for MockComm {
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            if let Some(ref needle) = self.fail_on_cmd {
                if command.contains(needle) {
                    return Err(MigratoryError::Generic(
                        "mock execute targeted failed".to_string(),
                    ));
                }
            }
            Ok("".to_string())
        }

        fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), MigratoryError> {
            if let Some(ref needle) = self.fail_upload_path {
                if remote_path.contains(needle) || local_path.to_string_lossy().contains(needle) {
                    return Err(MigratoryError::Generic("mock upload failed".to_string()));
                }
            }
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
    fn test_ansible_provisioner_success() {
        let mut prov = AnsibleProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir().expect("operation should succeed");
        let pb_path = dir.path().join("playbook.yml");
        let mut file = File::create(&pb_path).expect("operation should succeed");
        writeln!(file, "- hosts: all").expect("operation should succeed");

        let rf_path = dir.path().join("requirements.yml");
        let mut rf = File::create(&rf_path).expect("operation should succeed");
        writeln!(rf, "- src: geerlingguy.apache").expect("operation should succeed");

        let pb_str = pb_path.to_string_lossy().to_string();
        config.insert("playbook".to_string(), pb_str);
        config.insert("inventory_path".to_string(), "inv.ini".to_string());
        config.insert("extra_vars".to_string(), "foo=bar".to_string());
        config.insert("install".to_string(), "1".to_string());
        config.insert(
            "galaxy_role_file".to_string(),
            rf_path.to_string_lossy().to_string(),
        );

        assert_eq!(prov.name(), "ansible");
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_ok());
        assert!(prov.cleanup().is_ok());
    }

    #[test]
    fn test_ansible_provisioner_missing_config() {
        let mut prov = AnsibleProvisioner::default();
        let config = HashMap::new();
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_err());
    }

    #[test]
    fn test_ansible_provisioner_playbook_not_found() {
        let mut prov = AnsibleProvisioner::default();
        let mut config = HashMap::new();
        config.insert("playbook".to_string(), "/nonexistent/pb.yml".to_string());
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_err());
    }

    #[test]
    fn test_ansible_provisioner_guest_mode() {
        let mut prov = AnsibleProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir().expect("operation should succeed");
        let pb_path = dir.path().join("playbook.yml");
        let mut file = File::create(&pb_path).expect("operation should succeed");
        writeln!(file, "- hosts: all").expect("operation should succeed");

        config.insert(
            "playbook".to_string(),
            pb_path.to_string_lossy().to_string(),
        );
        config.insert("mode".to_string(), "guest".to_string());
        config.insert("extra_vars".to_string(), "foo=bar".to_string());
        // Do NOT insert inventory_path so it takes the None else branch:
        // command.push_str(" -i '127.0.0.1,' -c local");

        assert!(prov.prepare(&config).is_ok());
        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_ok());

        // Test non-existent inventory path (e.g. static IP inventory string "127.0.0.1,")
        let mut prov_inv_str = AnsibleProvisioner::default();
        let mut config_inv_str = HashMap::new();
        config_inv_str.insert(
            "playbook".to_string(),
            pb_path.to_string_lossy().to_string(),
        );
        config_inv_str.insert("mode".to_string(), "guest".to_string());
        config_inv_str.insert("inventory_path".to_string(), "127.0.0.1,".to_string());
        assert!(prov_inv_str.prepare(&config_inv_str).is_ok());
        assert!(prov_inv_str.provision(&comm).is_ok());
    }

    #[test]
    fn test_ansible_provisioner_guest_mode_false_branches() {
        let dir = tempdir().expect("operation should succeed");
        let pb_path = dir.path().join("playbook.yml");
        let mut file = File::create(&pb_path).expect("operation should succeed");
        writeln!(file, "- hosts: all").expect("operation should succeed");

        let mut prov = AnsibleProvisioner::default();
        let mut config = HashMap::new();
        config.insert(
            "playbook".to_string(),
            pb_path.to_string_lossy().to_string(),
        );
        config.insert("mode".to_string(), "guest".to_string());
        config.insert("install".to_string(), "false".to_string());
        config.insert("roles_path".to_string(), "/nonexistent/roles".to_string());
        config.insert(
            "galaxy_role_file".to_string(),
            "/nonexistent/roles.yml".to_string(),
        );

        assert!(prov.prepare(&config).is_ok());
        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_ansible_provisioner_guest_mode_missing_playbook() {
        let mut prov = AnsibleProvisioner::default();
        let mut config = HashMap::new();

        config.insert(
            "playbook".to_string(),
            "/nonexistent/playbook.yml".to_string(),
        );
        config.insert("mode".to_string(), "guest".to_string());

        assert!(prov.prepare(&config).is_ok());
        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_err());
    }

    #[test]
    fn test_ansible_provisioner_dynamic_inventory() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let mut prov = AnsibleProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir().expect("operation should succeed");
        let pb_path = dir.path().join("playbook.yml");
        let mut file = File::create(&pb_path).expect("operation should succeed");
        writeln!(file, "- hosts: all").expect("operation should succeed");

        config.insert(
            "playbook".to_string(),
            pb_path.to_string_lossy().to_string(),
        );
        config.insert("ssh_host".to_string(), "10.0.0.1".to_string());
        config.insert("ssh_port".to_string(), "2222".to_string());
        config.insert("ssh_user".to_string(), "ubuntu".to_string());
        config.insert("ssh_key".to_string(), "/path/to/key".to_string());
        config.insert("limit".to_string(), "all".to_string());
        config.insert("tags".to_string(), "deploy".to_string());
        config.insert("skip_tags".to_string(), "debug".to_string());
        config.insert("vault_password_file".to_string(), "/vault.txt".to_string());

        assert!(prov.prepare(&config).is_ok());
        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_ok());
        assert!(prov.cleanup().is_ok());

        // Test dynamic inventory without ssh_key (covers else branch)
        let mut prov_no_key = AnsibleProvisioner::default();
        let mut config_no_key = HashMap::new();
        config_no_key.insert(
            "playbook".to_string(),
            pb_path.to_string_lossy().to_string(),
        );
        assert!(prov_no_key.prepare(&config_no_key).is_ok());
        assert!(prov_no_key.provision(&comm).is_ok());
        assert!(prov_no_key.cleanup().is_ok());
    }

    #[test]
    fn test_ansible_provisioner_guest_mode_all_options() {
        let mut prov = AnsibleProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir().expect("operation should succeed");
        let pb_path = dir.path().join("playbook.yml");
        let mut file = File::create(&pb_path).expect("operation should succeed");
        writeln!(file, "- hosts: all").expect("operation should succeed");

        let roles_dir = dir.path().join("roles");
        std::fs::create_dir(&roles_dir).expect("operation should succeed");

        let inv_file = dir.path().join("inventory.ini");
        let mut inv = File::create(&inv_file).expect("operation should succeed");
        writeln!(inv, "localhost").expect("operation should succeed");

        let rf_path = dir.path().join("galaxy.yml");
        let mut rf = File::create(&rf_path).expect("operation should succeed");
        writeln!(rf, "- src: geerlingguy.mysql").expect("operation should succeed");

        config.insert(
            "playbook".to_string(),
            pb_path.to_string_lossy().to_string(),
        );
        config.insert("mode".to_string(), "ansible_local".to_string());
        config.insert("install".to_string(), "true".to_string());
        config.insert(
            "roles_path".to_string(),
            roles_dir.to_string_lossy().to_string(),
        );
        config.insert(
            "inventory_path".to_string(),
            inv_file.to_string_lossy().to_string(),
        );
        config.insert(
            "galaxy_role_file".to_string(),
            rf_path.to_string_lossy().to_string(),
        );
        config.insert("limit".to_string(), "web".to_string());
        config.insert("tags".to_string(), "build".to_string());
        config.insert("skip_tags".to_string(), "test".to_string());
        config.insert("vault_password_file".to_string(), "/pass.txt".to_string());

        assert!(prov.prepare(&config).is_ok());
        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_ansible_guest_execute_and_upload_failures() {
        let dir = tempdir().expect("operation should succeed");
        let pb_path = dir.path().join("playbook.yml");
        let mut file = File::create(&pb_path).expect("operation should succeed");
        writeln!(file, "- hosts: all").expect("operation should succeed");

        let roles_dir = dir.path().join("roles");
        std::fs::create_dir(&roles_dir).expect("operation should succeed");

        let inv_file = dir.path().join("inventory.ini");
        let mut inv = File::create(&inv_file).expect("operation should succeed");
        writeln!(inv, "localhost").expect("operation should succeed");

        let rf_path = dir.path().join("galaxy.yml");
        let mut rf = File::create(&rf_path).expect("operation should succeed");
        writeln!(rf, "- src: test").expect("operation should succeed");

        let mut config = HashMap::new();
        config.insert(
            "playbook".to_string(),
            pb_path.to_string_lossy().to_string(),
        );
        config.insert("mode".to_string(), "guest".to_string());
        config.insert("install".to_string(), "true".to_string());
        config.insert(
            "roles_path".to_string(),
            roles_dir.to_string_lossy().to_string(),
        );
        config.insert(
            "inventory_path".to_string(),
            inv_file.to_string_lossy().to_string(),
        );
        config.insert(
            "galaxy_role_file".to_string(),
            rf_path.to_string_lossy().to_string(),
        );

        let mut prov = AnsibleProvisioner::default();
        assert!(prov.prepare(&config).is_ok());

        // 1. install_cmd execute fails
        let comm_install_fail = MockComm {
            fail_on_cmd: Some("Installing Ansible".to_string()),
            ..Default::default()
        };
        assert!(prov.provision(&comm_install_fail).is_err());

        // 2. playbook upload fails
        let comm_upload_fail = MockComm {
            fail_upload_path: Some("/tmp/playbook.yml".to_string()),
            ..Default::default()
        };
        assert!(prov.provision(&comm_upload_fail).is_err());

        // 3. roles mkdir fails
        let comm_mkdir_fail = MockComm {
            fail_on_cmd: Some("mkdir -p /tmp/roles".to_string()),
            ..Default::default()
        };
        assert!(prov.provision(&comm_mkdir_fail).is_err());

        // 4. roles upload fails
        let comm_roles_upload_fail = MockComm {
            fail_upload_path: Some("/tmp/roles".to_string()),
            ..Default::default()
        };
        assert!(prov.provision(&comm_roles_upload_fail).is_err());

        // 5. galaxy requirements upload fails
        let comm_galaxy_upload_fail = MockComm {
            fail_upload_path: Some("galaxy_requirements.yml".to_string()),
            ..Default::default()
        };
        assert!(prov.provision(&comm_galaxy_upload_fail).is_err());

        // 6. galaxy command fails
        let comm_galaxy_fail = MockComm {
            fail_on_cmd: Some("ansible-galaxy".to_string()),
            ..Default::default()
        };
        assert!(prov.provision(&comm_galaxy_fail).is_err());

        // 7. inventory upload fails
        let comm_inv_upload_fail = MockComm {
            fail_upload_path: Some("ansible_inventory".to_string()),
            ..Default::default()
        };
        assert!(prov.provision(&comm_inv_upload_fail).is_err());

        // 8. final playbook execution fails
        let comm_pb_fail = MockComm {
            fail_on_cmd: Some("/tmp/playbook.yml".to_string()),
            ..Default::default()
        };
        assert!(prov.provision(&comm_pb_fail).is_err());
    }

    #[coverage(off)]
    fn poison_mutex(mutex: &Mutex<Option<String>>) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = mutex.lock();
            panic!("force poison");
        }));
    }

    #[test]
    fn test_ansible_host_mode_failures() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("operation should succeed");
        let pb_path = dir.path().join("playbook.yml");
        let mut file = File::create(&pb_path).expect("operation should succeed");
        writeln!(file, "- hosts: all").expect("operation should succeed");

        let rf_path = dir.path().join("galaxy.yml");
        let mut rf = File::create(&rf_path).expect("operation should succeed");
        writeln!(rf, "- src: test").expect("operation should succeed");

        let mut config = HashMap::new();
        config.insert(
            "playbook".to_string(),
            pb_path.to_string_lossy().to_string(),
        );
        config.insert(
            "galaxy_role_file".to_string(),
            rf_path.to_string_lossy().to_string(),
        );

        let mut prov = AnsibleProvisioner::default();
        assert!(prov.prepare(&config).is_ok());

        // Command fails
        unsafe {
            std::env::set_var("MIGRATORY_TEST_ANSIBLE_CMD_FAIL", "1");
        }
        let comm = MockComm::default();
        assert!(prov.provision(&comm).is_err());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_ANSIBLE_CMD_FAIL");
        }

        // Dynamic inventory creation fails when inventory path is a directory
        let inv_path = std::env::temp_dir()
            .join(".vagrant/provisioners/ansible/inventory/vagrant_ansible_inventory");
        let _ = std::fs::remove_file(&inv_path);
        let _ = std::fs::create_dir_all(&inv_path);
        let mut prov_inv = AnsibleProvisioner::default();
        let mut config_inv = HashMap::new();
        config_inv.insert(
            "playbook".to_string(),
            pb_path.to_string_lossy().to_string(),
        );
        assert!(prov_inv.prepare(&config_inv).is_ok());
        assert!(prov_inv.provision(&comm).is_err());
        let _ = std::fs::remove_dir_all(&inv_path);

        // Lock poisoned test
        let mut prov_poison = AnsibleProvisioner::default();
        poison_mutex(&prov_poison.generated_inventory);
        let mut config_poison = HashMap::new();
        config_poison.insert(
            "playbook".to_string(),
            pb_path.to_string_lossy().to_string(),
        );
        assert!(prov_poison.prepare(&config_poison).is_ok());
        assert!(prov_poison.provision(&comm).is_ok());
    }

    #[test]
    fn test_ansible_provisioner_empty_cleanup() {
        let prov = AnsibleProvisioner::default();
        assert!(prov.cleanup().is_ok());
    }

    #[test]
    fn test_mock_comm_upload_fail_on_local() {
        let comm = MockComm {
            fail_on_cmd: None,
            fail_upload_path: Some("local_only".to_string()),
        };
        assert!(
            comm.upload(Path::new("local_only.txt"), "dest.txt")
                .is_err()
        );
    }
}
