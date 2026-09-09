//! Chef provisioner implementation.

use super::Provisioner;
use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

/// Chef provisioner.
pub struct ChefProvisioner {
    mode: Option<String>,
    run_list: Option<String>,
    install: bool,
    json: String,
    cookbooks_path: Option<String>,
    roles_path: Option<String>,
    data_bags_path: Option<String>,
    environments_path: Option<String>,
}

impl ChefProvisioner {
    /// Creates a new chef provisioner.
    pub fn new() -> Self {
        Self {
            mode: None,
            run_list: None,
            install: true,
            json: "{}".to_string(),
            cookbooks_path: None,
            roles_path: None,
            data_bags_path: None,
            environments_path: None,
        }
    }
}

impl Default for ChefProvisioner {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to write content to a local temporary file and upload it to the guest.
///
/// # Arguments
///
/// * `comm` - The communicator used for upload.
/// * `file_name` - Local temporary file name.
/// * `content` - Content to write into the file.
/// * `remote_path` - Destination path on the guest.
///
/// # Errors
///
/// Returns a `MigratoryError` if local file creation or remote upload fails.
#[coverage(off)]
fn write_and_upload_temp(
    comm: &dyn Communicator,
    file_name: &str,
    content: &str,
    remote_path: &str,
) -> Result<(), MigratoryError> {
    let local_path = std::env::temp_dir().join(file_name);
    let mut file = File::create(&local_path).map_err(MigratoryError::Io)?;
    write!(file, "{}", content).map_err(MigratoryError::Io)?;
    comm.upload(&local_path, remote_path)?;
    let _ = std::fs::remove_file(&local_path);
    Ok(())
}

impl Provisioner for ChefProvisioner {
    fn name(&self) -> &str {
        "chef"
    }

    fn prepare(&mut self, config: &HashMap<String, String>) -> Result<(), MigratoryError> {
        self.mode = config
            .get("mode")
            .cloned()
            .or_else(|| Some("solo".to_string()));
        self.run_list = config.get("run_list").cloned();

        if let Some(install_str) = config.get("install") {
            self.install = install_str.to_lowercase() == "true" || install_str == "1";
        }

        if let Some(json_str) = config.get("json") {
            self.json = json_str.clone();
        }

        self.cookbooks_path = config.get("cookbooks_path").cloned();
        self.roles_path = config.get("roles_path").cloned();
        self.data_bags_path = config.get("data_bags_path").cloned();
        self.environments_path = config.get("environments_path").cloned();

        Ok(())
    }

    fn provision(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let mode = self.mode.as_deref().unwrap_or("solo");
        let run_list = match &self.run_list {
            Some(rl) => rl,
            None => {
                return Err(MigratoryError::Validation(
                    "Chef provisioner requires a 'run_list'".to_string(),
                ));
            }
        };

        if self.install {
            comm.execute("curl -sL https://omnitruck.chef.io/install.sh | sudo bash")?;
        }

        // Upload custom artifacts if configured
        let mut solo_rb_lines = Vec::new();

        if let Some(cb) = &self.cookbooks_path {
            let p = std::path::Path::new(cb);
            if p.exists() {
                comm.execute("mkdir -p /tmp/chef/cookbooks")?;
                comm.upload(p, "/tmp/chef/cookbooks")?;
                solo_rb_lines.push("cookbook_path ['/tmp/chef/cookbooks']".to_string());
            }
        }
        if let Some(rp) = &self.roles_path {
            let p = std::path::Path::new(rp);
            if p.exists() {
                comm.execute("mkdir -p /tmp/chef/roles")?;
                comm.upload(p, "/tmp/chef/roles")?;
                solo_rb_lines.push("role_path '/tmp/chef/roles'".to_string());
            }
        }
        if let Some(db) = &self.data_bags_path {
            let p = std::path::Path::new(db);
            if p.exists() {
                comm.execute("mkdir -p /tmp/chef/data_bags")?;
                comm.upload(p, "/tmp/chef/data_bags")?;
                solo_rb_lines.push("data_bag_path '/tmp/chef/data_bags'".to_string());
            }
        }
        if let Some(ep) = &self.environments_path {
            let p = std::path::Path::new(ep);
            if p.exists() {
                comm.execute("mkdir -p /tmp/chef/environments")?;
                comm.upload(p, "/tmp/chef/environments")?;
                solo_rb_lines.push("environment_path '/tmp/chef/environments'".to_string());
            }
        }

        if !solo_rb_lines.is_empty() {
            let solo_content = solo_rb_lines.join("\n");
            write_and_upload_temp(
                comm,
                &format!("migratory_chef_solo_{}.rb", std::process::id()),
                &solo_content,
                "/tmp/solo.rb",
            )?;
        }

        let remote_json_path = "/tmp/node.json";
        write_and_upload_temp(
            comm,
            &format!("migratory_chef_node_{}.json", std::process::id()),
            &self.json,
            remote_json_path,
        )?;

        let config_arg = if !solo_rb_lines.is_empty() {
            "-c /tmp/solo.rb"
        } else {
            ""
        };

        let command = if mode == "client" {
            format!(
                "sudo chef-client {} -j {} -o '{}'",
                config_arg,
                remote_json_path,
                run_list.replace('\'', "'\\''")
            )
        } else if mode == "zero" {
            format!(
                "sudo chef-client -z {} -j {} -o '{}'",
                config_arg,
                remote_json_path,
                run_list.replace('\'', "'\\''")
            )
        } else {
            format!(
                "sudo chef-solo {} -j {} -o '{}'",
                config_arg,
                remote_json_path,
                run_list.replace('\'', "'\\''")
            )
        };

        comm.execute(&command)?;
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
        fail_upload_substring: Option<&'static str>,
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
        fn upload(&self, _local_path: &Path, remote_path: &str) -> Result<(), MigratoryError> {
            if self.fail_upload {
                return Err(MigratoryError::Generic("upload failed".to_string()));
            }
            if let Some(needle) = self.fail_upload_substring {
                if remote_path.contains(needle) {
                    return Err(MigratoryError::Generic("upload failed".to_string()));
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
    fn test_chef_provisioner_solo() {
        let mut prov = ChefProvisioner::default();
        let mut config = HashMap::new();
        config.insert("run_list".to_string(), "recipe[apt]".to_string());

        assert_eq!(prov.name(), "chef");
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
        assert!(prov.cleanup().is_ok());
    }

    #[test]
    fn test_chef_provisioner_client() {
        let mut prov = ChefProvisioner::default();
        let mut config = HashMap::new();
        config.insert("mode".to_string(), "client".to_string());
        config.insert("run_list".to_string(), "recipe[apt]".to_string());
        config.insert("install".to_string(), "false".to_string());
        config.insert("json".to_string(), "{\"foo\":\"bar\"}".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_chef_provisioner_missing_run_list() {
        let mut prov = ChefProvisioner::default();
        let config = HashMap::new();

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_err());
    }

    #[test]
    fn test_chef_provisioner_install_one() {
        let mut prov = ChefProvisioner::default();
        let mut config = HashMap::new();
        config.insert("run_list".to_string(), "recipe[apt]".to_string());
        config.insert("install".to_string(), "1".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_chef_provisioner_nonexistent_paths() {
        let mut prov = ChefProvisioner::default();
        let mut config = HashMap::new();
        config.insert("run_list".to_string(), "recipe[starter]".to_string());
        config.insert(
            "cookbooks_path".to_string(),
            "/nonexistent/cookbooks".to_string(),
        );
        config.insert("roles_path".to_string(), "/nonexistent/roles".to_string());
        config.insert(
            "data_bags_path".to_string(),
            "/nonexistent/data_bags".to_string(),
        );
        config.insert(
            "environments_path".to_string(),
            "/nonexistent/environments".to_string(),
        );

        assert!(prov.prepare(&config).is_ok());
        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_chef_provisioner_zero_and_sync() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let cb_dir = dir.path().join("cookbooks");
        let roles_dir = dir.path().join("roles");
        let db_dir = dir.path().join("data_bags");
        let env_dir = dir.path().join("environments");

        std::fs::create_dir(&cb_dir).expect("create_dir failed");
        std::fs::create_dir(&roles_dir).expect("create_dir failed");
        std::fs::create_dir(&db_dir).expect("create_dir failed");
        std::fs::create_dir(&env_dir).expect("create_dir failed");

        let mut prov = ChefProvisioner::default();
        let mut config = HashMap::new();
        config.insert("mode".to_string(), "zero".to_string());
        config.insert("run_list".to_string(), "recipe[starter]".to_string());
        config.insert(
            "cookbooks_path".to_string(),
            cb_dir.to_string_lossy().to_string(),
        );
        config.insert(
            "roles_path".to_string(),
            roles_dir.to_string_lossy().to_string(),
        );
        config.insert(
            "data_bags_path".to_string(),
            db_dir.to_string_lossy().to_string(),
        );
        config.insert(
            "environments_path".to_string(),
            env_dir.to_string_lossy().to_string(),
        );

        assert!(prov.prepare(&config).is_ok());
        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_chef_provisioner_unprepared() {
        // If provision is called without prepare, mode is None, run_list is None.
        let mut prov = ChefProvisioner::default();
        prov.run_list = Some("recipe[apt]".to_string());

        let comm = MockComm;
        // mode is None here, so it falls back to "solo" in provision()
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_chef_provisioner_failures() {
        let dir = tempfile::tempdir().expect("tempdir failed");
        let cb_dir = dir.path().join("cookbooks");
        let roles_dir = dir.path().join("roles");
        let db_dir = dir.path().join("data_bags");
        let env_dir = dir.path().join("environments");

        std::fs::create_dir(&cb_dir).expect("create_dir failed");
        std::fs::create_dir(&roles_dir).expect("create_dir failed");
        std::fs::create_dir(&db_dir).expect("create_dir failed");
        std::fs::create_dir(&env_dir).expect("create_dir failed");

        let mut prov = ChefProvisioner::default();
        let mut config = HashMap::new();
        config.insert("run_list".to_string(), "recipe[apt]".to_string());
        config.insert("install".to_string(), "true".to_string());
        config.insert(
            "cookbooks_path".to_string(),
            cb_dir.to_string_lossy().to_string(),
        );
        config.insert(
            "roles_path".to_string(),
            roles_dir.to_string_lossy().to_string(),
        );
        config.insert(
            "data_bags_path".to_string(),
            db_dir.to_string_lossy().to_string(),
        );
        config.insert(
            "environments_path".to_string(),
            env_dir.to_string_lossy().to_string(),
        );
        assert!(prov.prepare(&config).is_ok());

        let comm_upload_fail = FailingComm {
            fail_upload: true,
            fail_upload_substring: None,
            fail_execute: false,
            fail_cmd_substring: None,
        };
        let comm_install_fail = FailingComm {
            fail_upload: false,
            fail_upload_substring: None,
            fail_execute: false,
            fail_cmd_substring: Some("omnitruck"),
        };
        let comm_mkdir_cookbooks_fail = FailingComm {
            fail_upload: false,
            fail_upload_substring: None,
            fail_execute: false,
            fail_cmd_substring: Some("mkdir -p /tmp/chef/cookbooks"),
        };
        let comm_mkdir_roles_fail = FailingComm {
            fail_upload: false,
            fail_upload_substring: None,
            fail_execute: false,
            fail_cmd_substring: Some("mkdir -p /tmp/chef/roles"),
        };
        let comm_upload_roles_fail = FailingComm {
            fail_upload: false,
            fail_upload_substring: Some("/tmp/chef/roles"),
            fail_execute: false,
            fail_cmd_substring: None,
        };
        let comm_mkdir_data_bags_fail = FailingComm {
            fail_upload: false,
            fail_upload_substring: None,
            fail_execute: false,
            fail_cmd_substring: Some("mkdir -p /tmp/chef/data_bags"),
        };
        let comm_upload_db_fail = FailingComm {
            fail_upload: false,
            fail_upload_substring: Some("/tmp/chef/data_bags"),
            fail_execute: false,
            fail_cmd_substring: None,
        };
        let comm_mkdir_environments_fail = FailingComm {
            fail_upload: false,
            fail_upload_substring: None,
            fail_execute: false,
            fail_cmd_substring: Some("mkdir -p /tmp/chef/environments"),
        };
        let comm_upload_env_fail = FailingComm {
            fail_upload: false,
            fail_upload_substring: Some("/tmp/chef/environments"),
            fail_execute: false,
            fail_cmd_substring: None,
        };
        let comm_upload_solo_fail = FailingComm {
            fail_upload: false,
            fail_upload_substring: Some("/tmp/solo.rb"),
            fail_execute: false,
            fail_cmd_substring: None,
        };
        let comm_upload_node_fail = FailingComm {
            fail_upload: false,
            fail_upload_substring: Some("/tmp/node.json"),
            fail_execute: false,
            fail_cmd_substring: None,
        };
        let comm_final_exec_fail = FailingComm {
            fail_upload: false,
            fail_upload_substring: None,
            fail_execute: false,
            fail_cmd_substring: Some("sudo chef-solo"),
        };

        assert!(prov.provision(&comm_install_fail).is_err());
        assert!(prov.provision(&comm_mkdir_cookbooks_fail).is_err());
        assert!(prov.provision(&comm_upload_fail).is_err());
        assert!(prov.provision(&comm_upload_solo_fail).is_err());

        // Test roles mkdir and upload fail without cookbooks
        let mut prov_roles = ChefProvisioner::default();
        let mut conf_roles = HashMap::new();
        conf_roles.insert("run_list".to_string(), "recipe[apt]".to_string());
        conf_roles.insert("install".to_string(), "false".to_string());
        conf_roles.insert(
            "roles_path".to_string(),
            roles_dir.to_string_lossy().to_string(),
        );
        assert!(prov_roles.prepare(&conf_roles).is_ok());
        assert!(prov_roles.provision(&comm_mkdir_roles_fail).is_err());
        assert!(prov_roles.provision(&comm_upload_roles_fail).is_err());

        // Test data_bags mkdir and upload fail
        let mut prov_db = ChefProvisioner::default();
        let mut conf_db = HashMap::new();
        conf_db.insert("run_list".to_string(), "recipe[apt]".to_string());
        conf_db.insert("install".to_string(), "false".to_string());
        conf_db.insert(
            "data_bags_path".to_string(),
            db_dir.to_string_lossy().to_string(),
        );
        assert!(prov_db.prepare(&conf_db).is_ok());
        assert!(prov_db.provision(&comm_mkdir_data_bags_fail).is_err());
        assert!(prov_db.provision(&comm_upload_db_fail).is_err());

        // Test environments mkdir and upload fail
        let mut prov_env = ChefProvisioner::default();
        let mut conf_env = HashMap::new();
        conf_env.insert("run_list".to_string(), "recipe[apt]".to_string());
        conf_env.insert("install".to_string(), "false".to_string());
        conf_env.insert(
            "environments_path".to_string(),
            env_dir.to_string_lossy().to_string(),
        );
        assert!(prov_env.prepare(&conf_env).is_ok());
        assert!(prov_env.provision(&comm_mkdir_environments_fail).is_err());
        assert!(prov_env.provision(&comm_upload_env_fail).is_err());

        // Test node.json upload fail and final command fail without custom artifacts
        let mut prov_simple = ChefProvisioner::default();
        let mut conf_simple = HashMap::new();
        conf_simple.insert("run_list".to_string(), "recipe[apt]".to_string());
        conf_simple.insert("install".to_string(), "false".to_string());
        assert!(prov_simple.prepare(&conf_simple).is_ok());
        assert!(prov_simple.provision(&comm_upload_node_fail).is_err());
        assert!(prov_simple.provision(&comm_final_exec_fail).is_err());
    }
}
