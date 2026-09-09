//! Salt provisioner implementation.

use super::Provisioner;
use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::collections::HashMap;

/// Salt provisioner.
pub struct SaltProvisioner {
    run_highstate: bool,
    install_master: bool,
    no_minion: bool,
    masterless: bool,
    install_type: Option<String>,
    minion_config: Option<String>,
    formula_path: Option<String>,
    state_files_path: Option<String>,
    pillar_path: Option<String>,
}

impl SaltProvisioner {
    /// Creates a new salt provisioner.
    pub fn new() -> Self {
        Self {
            run_highstate: true,
            install_master: false,
            no_minion: false,
            masterless: false,
            install_type: None,
            minion_config: None,
            formula_path: None,
            state_files_path: None,
            pillar_path: None,
        }
    }
}

impl Default for SaltProvisioner {
    fn default() -> Self {
        Self::new()
    }
}

impl Provisioner for SaltProvisioner {
    fn name(&self) -> &str {
        "salt"
    }

    fn prepare(&mut self, config: &HashMap<String, String>) -> Result<(), MigratoryError> {
        if let Some(v) = config.get("run_highstate") {
            self.run_highstate = v.to_lowercase() == "true" || v == "1";
        }
        if let Some(v) = config.get("install_master") {
            self.install_master = v.to_lowercase() == "true" || v == "1";
        }
        if let Some(v) = config.get("no_minion") {
            self.no_minion = v.to_lowercase() == "true" || v == "1";
        }
        if let Some(v) = config.get("masterless") {
            self.masterless = v.to_lowercase() == "true" || v == "1";
        } else if self.no_minion {
            self.masterless = true;
        }
        self.install_type = config.get("install_type").cloned();
        self.minion_config = config.get("minion_config").cloned();
        self.formula_path = config.get("formula_path").cloned();
        self.state_files_path = config.get("state_files_path").cloned();
        self.pillar_path = config.get("pillar_path").cloned();
        Ok(())
    }

    fn provision(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let install_type = self.install_type.as_deref().unwrap_or("stable");

        // Ensure salt bootstrap script is run
        let mut install_cmd = format!(
            "curl -L https://bootstrap.saltstack.com -o install_salt.sh && sudo sh install_salt.sh -P {}",
            install_type
        );
        if self.install_master {
            install_cmd.push_str(" -M");
        }
        if self.no_minion {
            install_cmd.push_str(" -N");
        }
        comm.execute(&install_cmd)?;

        // Upload formulas if configured
        if let Some(fp) = &self.formula_path {
            let p = std::path::Path::new(fp);
            if p.exists() {
                comm.execute("sudo mkdir -p /srv/formulas")?;
                comm.upload(p, "/srv/formulas")?;
            }
        }

        // Upload state files if configured
        if let Some(sp) = &self.state_files_path {
            let p = std::path::Path::new(sp);
            if p.exists() {
                comm.execute("sudo mkdir -p /srv/salt")?;
                comm.upload(p, "/srv/salt")?;
            }
        }

        // Upload pillar files if configured
        if let Some(pp) = &self.pillar_path {
            let p = std::path::Path::new(pp);
            if p.exists() {
                comm.execute("sudo mkdir -p /srv/pillar")?;
                comm.upload(p, "/srv/pillar")?;
            }
        }

        // Upload custom minion config if specified
        if let Some(cfg_path) = &self.minion_config {
            let local_path = std::path::Path::new(cfg_path);
            if local_path.exists() {
                comm.upload(local_path, "/tmp/minion")?;
                comm.execute(
                    "sudo mv /tmp/minion /etc/salt/minion && sudo systemctl restart salt-minion",
                )?;
            } else {
                return Err(MigratoryError::NotFound(format!(
                    "minion_config not found: {}",
                    cfg_path
                )));
            }
        }

        if self.run_highstate {
            if self.masterless {
                comm.execute("sudo salt-call --local state.highstate")?;
            } else {
                comm.execute("sudo salt-call state.highstate")?;
            }
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
    use std::io::Write;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    struct MockComm;

    impl Communicator for MockComm {
        #[coverage(off)]
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

    struct FailingComm {
        fail_upload: bool,
        fail_execute: bool,
        fail_cmd_substring: Option<&'static str>,
    }

    impl Communicator for FailingComm {
        #[coverage(off)]
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            if self.fail_execute {
                return Err(MigratoryError::Generic("execute failed".to_string()));
            }
            if let Some(needle) = self.fail_cmd_substring {
                if command.contains(needle) {
                    return Err(MigratoryError::Generic("execute failed".to_string()));
                }
            }
            Ok("".to_string())
        }
        #[coverage(off)]
        fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
            if self.fail_upload {
                Err(MigratoryError::Generic("upload failed".to_string()))
            } else {
                Ok(())
            }
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
    fn test_salt_provisioner_success() {
        let mut prov = SaltProvisioner::default();
        let mut config = HashMap::new();
        config.insert("run_highstate".to_string(), "true".to_string());
        config.insert("install_master".to_string(), "true".to_string());
        config.insert("no_minion".to_string(), "false".to_string());
        config.insert("install_type".to_string(), "stable".to_string());

        assert_eq!(prov.name(), "salt");
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
        assert!(prov.cleanup().is_ok());
    }

    #[test]
    fn test_salt_provisioner_flags() {
        let mut prov = SaltProvisioner::default();
        let mut config = HashMap::new();
        config.insert("run_highstate".to_string(), "1".to_string());
        config.insert("install_master".to_string(), "1".to_string());
        config.insert("no_minion".to_string(), "1".to_string());

        assert!(prov.prepare(&config).is_ok());
        assert!(prov.run_highstate);
        assert!(prov.install_master);
        assert!(prov.no_minion);
        assert!(prov.masterless);
    }

    #[test]
    fn test_salt_provisioner_nonexistent_paths() {
        let mut prov = SaltProvisioner::default();
        let mut config = HashMap::new();
        config.insert(
            "formula_path".to_string(),
            "/nonexistent/formula_path".to_string(),
        );
        config.insert(
            "state_files_path".to_string(),
            "/nonexistent/state_files_path".to_string(),
        );
        config.insert(
            "pillar_path".to_string(),
            "/nonexistent/pillar_path".to_string(),
        );
        config.insert("run_highstate".to_string(), "false".to_string());

        assert!(prov.prepare(&config).is_ok());
        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_salt_provisioner_masterless_and_sync() {
        let dir = tempdir().expect("tempdir failed");
        let formulas_dir = dir.path().join("formulas");
        let states_dir = dir.path().join("states");
        let pillar_dir = dir.path().join("pillar");

        std::fs::create_dir(&formulas_dir).expect("create_dir failed");
        std::fs::create_dir(&states_dir).expect("create_dir failed");
        std::fs::create_dir(&pillar_dir).expect("create_dir failed");

        let mut prov = SaltProvisioner::default();
        let mut config = HashMap::new();
        config.insert("masterless".to_string(), "true".to_string());
        config.insert(
            "formula_path".to_string(),
            formulas_dir.to_string_lossy().to_string(),
        );
        config.insert(
            "state_files_path".to_string(),
            states_dir.to_string_lossy().to_string(),
        );
        config.insert(
            "pillar_path".to_string(),
            pillar_dir.to_string_lossy().to_string(),
        );
        config.insert("run_highstate".to_string(), "true".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_salt_provisioner_minion_config() {
        let mut prov = SaltProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir().expect("tempdir failed");
        let cfg_path = dir.path().join("minion");
        let mut file = std::fs::File::create(&cfg_path).expect("create file failed");
        writeln!(file, "master: localhost").expect("write failed");

        config.insert(
            "minion_config".to_string(),
            cfg_path.to_string_lossy().to_string(),
        );
        config.insert("no_minion".to_string(), "true".to_string());
        config.insert("run_highstate".to_string(), "false".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_salt_provisioner_minion_config_missing() {
        let mut prov = SaltProvisioner::default();
        let mut config = HashMap::new();

        config.insert(
            "minion_config".to_string(),
            "/nonexistent/minion_cfg".to_string(),
        );

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_err());
    }

    #[test]
    fn test_salt_provisioner_failures() {
        let dir = tempdir().expect("tempdir failed");
        let formulas_dir = dir.path().join("formulas");
        let states_dir = dir.path().join("states");
        let pillar_dir = dir.path().join("pillar");
        let cfg_path = dir.path().join("minion");

        std::fs::create_dir(&formulas_dir).expect("create_dir failed");
        std::fs::create_dir(&states_dir).expect("create_dir failed");
        std::fs::create_dir(&pillar_dir).expect("create_dir failed");
        std::fs::write(&cfg_path, "master: local").expect("write failed");

        let comm_exec_fail = FailingComm {
            fail_upload: false,
            fail_execute: true,
            fail_cmd_substring: None,
        };
        let comm_upload_fail = FailingComm {
            fail_upload: true,
            fail_execute: false,
            fail_cmd_substring: None,
        };

        // 1. install_cmd execute failure
        let prov1 = SaltProvisioner::default();
        assert!(prov1.provision(&comm_exec_fail).is_err());

        // 2. formula mkdir and upload failure
        let mut prov2 = SaltProvisioner::default();
        let mut config2 = HashMap::new();
        config2.insert(
            "formula_path".to_string(),
            formulas_dir.to_string_lossy().to_string(),
        );
        assert!(prov2.prepare(&config2).is_ok());
        let comm_mkdir_formulas_fail = FailingComm {
            fail_upload: false,
            fail_execute: false,
            fail_cmd_substring: Some("/srv/formulas"),
        };
        assert!(prov2.provision(&comm_mkdir_formulas_fail).is_err());
        assert!(prov2.provision(&comm_upload_fail).is_err());

        // 3. state_files_path mkdir and upload failure
        let mut prov3 = SaltProvisioner::default();
        let mut config3 = HashMap::new();
        config3.insert(
            "state_files_path".to_string(),
            states_dir.to_string_lossy().to_string(),
        );
        assert!(prov3.prepare(&config3).is_ok());
        let comm_mkdir_salt_fail = FailingComm {
            fail_upload: false,
            fail_execute: false,
            fail_cmd_substring: Some("/srv/salt"),
        };
        assert!(prov3.provision(&comm_mkdir_salt_fail).is_err());
        assert!(prov3.provision(&comm_upload_fail).is_err());

        // 4. pillar_path mkdir and upload failure
        let mut prov4 = SaltProvisioner::default();
        let mut config4 = HashMap::new();
        config4.insert(
            "pillar_path".to_string(),
            pillar_dir.to_string_lossy().to_string(),
        );
        assert!(prov4.prepare(&config4).is_ok());
        let comm_mkdir_pillar_fail = FailingComm {
            fail_upload: false,
            fail_execute: false,
            fail_cmd_substring: Some("/srv/pillar"),
        };
        assert!(prov4.provision(&comm_mkdir_pillar_fail).is_err());
        assert!(prov4.provision(&comm_upload_fail).is_err());

        // 5. minion_config upload and mv failure
        let mut prov5 = SaltProvisioner::default();
        let mut config5 = HashMap::new();
        config5.insert(
            "minion_config".to_string(),
            cfg_path.to_string_lossy().to_string(),
        );
        assert!(prov5.prepare(&config5).is_ok());
        let comm_mv_minion_fail = FailingComm {
            fail_upload: false,
            fail_execute: false,
            fail_cmd_substring: Some("salt-minion"),
        };
        assert!(prov5.provision(&comm_mv_minion_fail).is_err());
        assert!(prov5.provision(&comm_upload_fail).is_err());

        // 6. highstate masterless failure
        let mut prov6 = SaltProvisioner::default();
        let mut config6 = HashMap::new();
        config6.insert("run_highstate".to_string(), "true".to_string());
        config6.insert("masterless".to_string(), "true".to_string());
        assert!(prov6.prepare(&config6).is_ok());
        let comm_highstate_masterless_fail = FailingComm {
            fail_upload: false,
            fail_execute: false,
            fail_cmd_substring: Some("--local state.highstate"),
        };
        assert!(prov6.provision(&comm_highstate_masterless_fail).is_err());

        // 7. highstate non-masterless failure
        let mut prov7 = SaltProvisioner::default();
        let mut config7 = HashMap::new();
        config7.insert("run_highstate".to_string(), "true".to_string());
        config7.insert("masterless".to_string(), "false".to_string());
        assert!(prov7.prepare(&config7).is_ok());
        let comm_highstate_fail = FailingComm {
            fail_upload: false,
            fail_execute: false,
            fail_cmd_substring: Some("state.highstate"),
        };
        assert!(prov7.provision(&comm_highstate_fail).is_err());
    }
}
