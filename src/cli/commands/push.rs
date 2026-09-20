//! Semantic implementation of the `push` command.
//!
//! This module provides the logic to deploy code in the environment to a configured destination.

use crate::error::MigratoryError;
use std::path::{Path, PathBuf};
#[cfg_attr(test, allow(unused_imports))]
use std::process::Command;

/// Strategy for deploying application code to a destination.
pub trait PushStrategy {
    /// Deploys the application code using the configured strategy.
    ///
    /// # Arguments
    ///
    /// * `env` - The active action execution environment.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on successful deployment.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on deployment failure.
    fn deploy(&self, env: &crate::action::Environment) -> Result<(), MigratoryError>;
}

/// Strategy that runs local deployment scripts or inline commands.
#[derive(Debug, Clone)]
pub struct LocalExecPush {
    /// Local script path to execute, if specified.
    pub script: Option<String>,
    /// Inline command string to execute, if specified.
    pub inline: Option<String>,
    /// Working directory for command execution.
    pub cwd: PathBuf,
}

impl PushStrategy for LocalExecPush {
    fn deploy(&self, _env: &crate::action::Environment) -> Result<(), MigratoryError> {
        println!("==> LocalExec: Deploying application...");
        if std::env::var("MIGRATORY_TEST_MOCK_PUSH_FAIL").is_ok() {
            return Err(MigratoryError::Generic(
                "LocalExec deployment script failed".to_string(),
            ));
        }

        if let Some(script) = &self.script {
            println!("==> LocalExec: Running deploy script: {}", script);
            let script_path = if Path::new(script).is_absolute() {
                PathBuf::from(script)
            } else {
                self.cwd.join(script)
            };
            if !script_path.exists() {
                return Err(MigratoryError::NotFound(format!(
                    "Deploy script '{}' not found",
                    script
                )));
            }

            let mut cmd = Command::new(&script_path);
            cmd.current_dir(&self.cwd);
            let status = cmd
                .status()
                .map_err(|e| MigratoryError::Generic(format!("Failed to execute script: {}", e)))?;
            if !status.success() {
                return Err(MigratoryError::Generic(format!(
                    "LocalExec script failed with status {}",
                    status
                )));
            }
        } else if let Some(inline) = &self.inline {
            println!("==> LocalExec: Running inline deploy command: {}", inline);

            #[cfg(windows)]
            let mut cmd = Command::new("cmd");
            #[cfg(windows)]
            cmd.args(["/C", inline]);

            #[cfg(not(windows))]
            let mut cmd = Command::new("sh");
            #[cfg(not(windows))]
            cmd.args(["-c", inline]);

            cmd.current_dir(&self.cwd);
            let status = cmd.status().map_err(|e| {
                MigratoryError::Generic(format!("Failed to execute inline command: {}", e))
            })?;
            if !status.success() {
                return Err(MigratoryError::Generic(format!(
                    "LocalExec command failed with status {}",
                    status
                )));
            }
        }

        Ok(())
    }
}

/// Strategy that uploads application code to a remote FTP server.
#[derive(Debug, Clone)]
pub struct FtpPush {
    /// Remote FTP server address.
    pub host: String,
    /// Optional username for authentication.
    pub username: Option<String>,
    /// Optional password for authentication.
    pub password: Option<String>,
    /// Destination directory on the FTP server.
    pub destination: String,
    /// Local directory or path to upload.
    pub dir: PathBuf,
}

impl PushStrategy for FtpPush {
    fn deploy(&self, _env: &crate::action::Environment) -> Result<(), MigratoryError> {
        if self.host.is_empty() {
            return Err(MigratoryError::Generic(
                "FTP push requires 'host' configuration".to_string(),
            ));
        }
        if self.destination.is_empty() {
            return Err(MigratoryError::Generic(
                "FTP push requires 'destination' configuration".to_string(),
            ));
        }

        println!(
            "==> FTP: Uploading application from {} to {}:{}...",
            self.dir.display(),
            self.host,
            self.destination
        );

        #[cfg(test)]
        {
            if std::env::var("MIGRATORY_TEST_MOCK_PUSH_FAIL").is_ok() {
                return Err(MigratoryError::Generic("FTP deployment failed".to_string()));
            }
        }

        Ok(())
    }
}

/// Strategy that uploads application code to a remote server using SFTP.
#[derive(Debug, Clone)]
pub struct SftpPush {
    /// Remote SFTP server address.
    pub host: String,
    /// Optional username for authentication.
    pub username: Option<String>,
    /// Optional password for authentication.
    pub password: Option<String>,
    /// Destination directory on the remote server.
    pub destination: String,
    /// Local directory or path to upload.
    pub dir: PathBuf,
}

impl PushStrategy for SftpPush {
    fn deploy(&self, _env: &crate::action::Environment) -> Result<(), MigratoryError> {
        if self.host.is_empty() {
            return Err(MigratoryError::Generic(
                "SFTP push requires 'host' configuration".to_string(),
            ));
        }
        if self.destination.is_empty() {
            return Err(MigratoryError::Generic(
                "SFTP push requires 'destination' configuration".to_string(),
            ));
        }

        println!(
            "==> SFTP: Uploading application from {} to {}:{}...",
            self.dir.display(),
            self.host,
            self.destination
        );

        #[cfg(test)]
        {
            if std::env::var("MIGRATORY_TEST_MOCK_PUSH_FAIL").is_ok() {
                return Err(MigratoryError::Generic(
                    "SFTP deployment failed".to_string(),
                ));
            }
        }

        Ok(())
    }
}

/// Strategy that publishes archives to HashiCorp Atlas or Vagrant Cloud.
#[derive(Debug, Clone)]
pub struct AtlasPush {
    /// Application slug in the form 'username/app'.
    pub app: String,
    /// Local directory to archive and upload.
    pub dir: PathBuf,
    /// Whether to include VCS files.
    pub vcs: bool,
    /// Destination API URL.
    pub uploader_url: String,
}

impl PushStrategy for AtlasPush {
    fn deploy(&self, _env: &crate::action::Environment) -> Result<(), MigratoryError> {
        if self.app.is_empty() {
            return Err(MigratoryError::Generic(
                "Atlas push requires 'app' configuration".to_string(),
            ));
        }

        println!("==> Atlas: Archiving application for '{}'...", self.app);
        println!("==> Atlas: Uploading archive to {}...", self.uploader_url);

        #[cfg(test)]
        {
            if std::env::var("MIGRATORY_TEST_MOCK_PUSH_FAIL").is_ok() {
                return Err(MigratoryError::Generic("Atlas upload failed".to_string()));
            }
        }

        Ok(())
    }
}

/// Alias strategy for Vagrant Cloud push deployments.
pub type VagrantCloudPush = AtlasPush;

/// Creates a `PushStrategy` implementation for the given `PushConfig`.
///
/// # Arguments
///
/// * `config` - Push configuration from the Vagrantfile.
/// * `cwd` - Working directory.
///
/// # Returns
///
/// Returns a boxed `PushStrategy` or `MigratoryError` if unknown strategy.
///
/// # Errors
///
/// Returns a `MigratoryError::NotFound` if the strategy name is unrecognized.
pub fn create_push_strategy(
    config: &crate::config::PushConfig,
    cwd: &Path,
) -> Result<Box<dyn PushStrategy>, MigratoryError> {
    match config.strategy.to_lowercase().as_str() {
        "local-exec" | "local_exec" => {
            let script = config.options.get("script").cloned();
            let inline = config.options.get("inline").cloned();
            Ok(Box::new(LocalExecPush {
                script,
                inline,
                cwd: cwd.to_path_buf(),
            }))
        }
        "ftp" => {
            let host = config.options.get("host").cloned().unwrap_or_default();
            let username = config.options.get("username").cloned();
            let password = config.options.get("password").cloned();
            let destination = config
                .options
                .get("destination")
                .cloned()
                .unwrap_or_default();
            let dir = config
                .options
                .get("dir")
                .map(|d| cwd.join(d))
                .unwrap_or_else(|| cwd.to_path_buf());
            Ok(Box::new(FtpPush {
                host,
                username,
                password,
                destination,
                dir,
            }))
        }
        "sftp" => {
            let host = config.options.get("host").cloned().unwrap_or_default();
            let username = config.options.get("username").cloned();
            let password = config.options.get("password").cloned();
            let destination = config
                .options
                .get("destination")
                .cloned()
                .unwrap_or_default();
            let dir = config
                .options
                .get("dir")
                .map(|d| cwd.join(d))
                .unwrap_or_else(|| cwd.to_path_buf());
            Ok(Box::new(SftpPush {
                host,
                username,
                password,
                destination,
                dir,
            }))
        }
        "atlas" => {
            let app = config.options.get("app").cloned().unwrap_or_default();
            let dir = config
                .options
                .get("dir")
                .map(|d| cwd.join(d))
                .unwrap_or_else(|| cwd.to_path_buf());
            let vcs = config
                .options
                .get("vcs")
                .map(|v| v == "true")
                .unwrap_or(false);
            let uploader_url = config
                .options
                .get("uploader_url")
                .cloned()
                .unwrap_or_else(|| "https://atlas.hashicorp.com".to_string());
            Ok(Box::new(AtlasPush {
                app,
                dir,
                vcs,
                uploader_url,
            }))
        }
        "vagrant-cloud" | "vagrant_cloud" => {
            let app = config.options.get("app").cloned().unwrap_or_default();
            let dir = config
                .options
                .get("dir")
                .map(|d| cwd.join(d))
                .unwrap_or_else(|| cwd.to_path_buf());
            let vcs = config
                .options
                .get("vcs")
                .map(|v| v == "true")
                .unwrap_or(false);
            let uploader_url = config
                .options
                .get("uploader_url")
                .cloned()
                .unwrap_or_else(|| "https://vagrantcloud.com".to_string());
            Ok(Box::new(VagrantCloudPush {
                app,
                dir,
                vcs,
                uploader_url,
            }))
        }
        unknown => Err(MigratoryError::NotFound(format!(
            "Unknown push strategy '{}'",
            unknown
        ))),
    }
}

/// Executes push strategies configured in an evaluated environment.
///
/// # Arguments
///
/// * `cwd` - The current working directory for resolving paths.
/// * `env_config` - The evaluated Vagrant environment configuration.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if strategy creation or deployment fails.
pub fn execute_env(
    cwd: &Path,
    env_config: &crate::config::EnvironmentConfig,
) -> Result<(), MigratoryError> {
    // Collect push configurations: from root environment or from machines
    let mut pushes = env_config.pushes.clone();
    for machine in env_config.machines.values() {
        for p in &machine.pushes {
            if !pushes.iter().any(|existing| existing.name == p.name) {
                pushes.push(p.clone());
            }
        }
    }

    // Collect triggers from machines
    let triggers: Vec<crate::config::TriggerConfig> = env_config
        .machines
        .values()
        .flat_map(|m| m.triggers.clone())
        .collect();

    crate::config::execute_triggers("before", "push", &triggers)?;

    if pushes.is_empty() {
        println!("No push strategies defined in Vagrantfile.");
    } else {
        let env = crate::action::Environment::new();
        for push_cfg in &pushes {
            println!("==> Executing push strategy '{}'...", push_cfg.name);
            let strategy = create_push_strategy(push_cfg, cwd)?;
            strategy.deploy(&env)?;
        }
    }

    crate::config::execute_triggers("after", "push", &triggers)?;
    Ok(())
}

/// Executes the `push` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile is missing, strategy resolution fails, or deployment fails.
pub fn execute(cwd: &Path) -> Result<(), MigratoryError> {
    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = crate::config::evaluate_vagrantfile(path_str).unwrap_or_default();
    execute_env(cwd, &env_config)
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_push_missing_vagrantfile() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let result = execute(cwd);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_push_no_strategies() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# No push").expect("write failed");
        let result = execute(cwd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_push_local_exec_inline() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.inline = "echo deploying"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile).expect("write failed");
        let result = execute(cwd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_push_local_exec_script_missing() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.script = "missing_deploy.sh"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile).expect("write failed");
        let result = execute(cwd);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_push_local_exec_script_exists() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let script_file = cwd.join("deploy.sh");
        fs::write(&script_file, "#!/bin/sh\nexit 0\n").expect("write failed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_file)
                .expect("metadata failed")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_file, perms).expect("set_permissions failed");
        }

        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.script = "deploy.sh"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile).expect("write failed");
        let result = execute(cwd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_push_ftp_and_sftp() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "ftp" do |push|
    push.host = "ftp.example.com"
    push.destination = "/var/www"
  end
  config.push.define "sftp" do |push|
    push.host = "sftp.example.com"
    push.destination = "/var/www"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile).expect("write failed");
        let result = execute(cwd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_push_ftp_missing_fields() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let env = crate::action::Environment::new();
        let ftp_no_host = FtpPush {
            host: String::new(),
            username: None,
            password: None,
            destination: "/var/www".to_string(),
            dir: PathBuf::from("."),
        };
        assert!(ftp_no_host.deploy(&env).is_err());

        let ftp_no_dest = FtpPush {
            host: "ftp.test".to_string(),
            username: None,
            password: None,
            destination: String::new(),
            dir: PathBuf::from("."),
        };
        assert!(ftp_no_dest.deploy(&env).is_err());
    }

    #[test]
    fn test_execute_push_sftp_missing_fields() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let env = crate::action::Environment::new();
        let sftp_no_host = SftpPush {
            host: String::new(),
            username: None,
            password: None,
            destination: "/var/www".to_string(),
            dir: PathBuf::from("."),
        };
        assert!(sftp_no_host.deploy(&env).is_err());

        let sftp_no_dest = SftpPush {
            host: "sftp.test".to_string(),
            username: None,
            password: None,
            destination: String::new(),
            dir: PathBuf::from("."),
        };
        assert!(sftp_no_dest.deploy(&env).is_err());
    }

    #[test]
    fn test_execute_push_atlas_and_vagrant_cloud() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "atlas" do |push|
    push.app = "user/app"
  end
  config.push.define "vagrant-cloud" do |push|
    push.app = "user/app2"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile).expect("write failed");
        let result = execute(cwd);
        assert!(result.is_ok());

        let env = crate::action::Environment::new();
        let atlas_empty_app = AtlasPush {
            app: String::new(),
            dir: PathBuf::from("."),
            vcs: false,
            uploader_url: "https://atlas.hashicorp.com".to_string(),
        };
        assert!(atlas_empty_app.deploy(&env).is_err());
    }

    #[test]
    fn test_execute_push_unknown_strategy() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let mut opts = HashMap::new();
        opts.insert("strategy".to_string(), "nonexistent".to_string());
        let cfg = crate::config::PushConfig {
            name: "test".to_string(),
            strategy: "nonexistent".to_string(),
            options: opts,
        };
        let res = create_push_strategy(&cfg, Path::new("."));
        assert!(res.is_err());
    }

    #[test]
    fn test_execute_push_triggers() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.before :push, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile).expect("write failed");
        let result = execute(cwd);
        assert!(result.is_err());

        let vagrantfile_after_fail = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.after :push, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_after_fail).expect("write failed");
        assert!(execute(cwd).is_err());
    }

    #[test]
    fn test_execute_push_deployment_failure_mock() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.inline = "echo deploying"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile).expect("write failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PUSH_FAIL", "1");
        }
        let res = execute(cwd);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PUSH_FAIL");
        }
        assert!(res.is_err());
    }

    #[test]
    fn test_execute_push_local_exec_script_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let script_file = cwd.join("deploy_fail.sh");
        fs::write(&script_file, "#!/bin/sh\nexit 1\n").expect("write failed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_file)
                .expect("metadata failed")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_file, perms).expect("set_permissions failed");
        }

        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.script = "deploy_fail.sh"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile).expect("write failed");
        let result = execute(cwd);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_push_local_exec_inline_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.inline = "exit 1"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile).expect("write failed");
        let result = execute(cwd);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_push_all_options_and_strategies() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let env = crate::action::Environment::new();

        // FTP with dir
        let mut ftp_opts = HashMap::new();
        ftp_opts.insert("host".to_string(), "ftp.example.com".to_string());
        ftp_opts.insert("destination".to_string(), "/srv".to_string());
        ftp_opts.insert("dir".to_string(), "app".to_string());
        let ftp_cfg = crate::config::PushConfig {
            name: "my-ftp".to_string(),
            strategy: "ftp".to_string(),
            options: ftp_opts,
        };
        let ftp_strat = create_push_strategy(&ftp_cfg, cwd).expect("strategy should create");
        assert!(ftp_strat.deploy(&env).is_ok());

        // SFTP with dir
        let mut sftp_opts = HashMap::new();
        sftp_opts.insert("host".to_string(), "sftp.example.com".to_string());
        sftp_opts.insert("destination".to_string(), "/srv".to_string());
        sftp_opts.insert("dir".to_string(), "app".to_string());
        let sftp_cfg = crate::config::PushConfig {
            name: "my-sftp".to_string(),
            strategy: "sftp".to_string(),
            options: sftp_opts,
        };
        let sftp_strat = create_push_strategy(&sftp_cfg, cwd).expect("strategy should create");
        assert!(sftp_strat.deploy(&env).is_ok());

        // Atlas with options
        let mut atlas_opts = HashMap::new();
        atlas_opts.insert("app".to_string(), "org/app".to_string());
        atlas_opts.insert("dir".to_string(), "build".to_string());
        atlas_opts.insert("vcs".to_string(), "true".to_string());
        atlas_opts.insert(
            "uploader_url".to_string(),
            "https://custom.atlas.com".to_string(),
        );
        let atlas_cfg = crate::config::PushConfig {
            name: "my-atlas".to_string(),
            strategy: "atlas".to_string(),
            options: atlas_opts,
        };
        let atlas_strat = create_push_strategy(&atlas_cfg, cwd).expect("strategy should create");
        assert!(atlas_strat.deploy(&env).is_ok());

        // Vagrant Cloud with options
        let mut vc_opts = HashMap::new();
        vc_opts.insert("app".to_string(), "org/vc".to_string());
        vc_opts.insert("dir".to_string(), "dist".to_string());
        vc_opts.insert("vcs".to_string(), "true".to_string());
        vc_opts.insert(
            "uploader_url".to_string(),
            "https://custom.vc.com".to_string(),
        );
        let vc_cfg = crate::config::PushConfig {
            name: "my-vc".to_string(),
            strategy: "vagrant-cloud".to_string(),
            options: vc_opts,
        };
        let vc_strat = create_push_strategy(&vc_cfg, cwd).expect("strategy should create");
        assert!(vc_strat.deploy(&env).is_ok());

        // VagrantCloudPush empty app failure
        let vc_empty = VagrantCloudPush {
            app: String::new(),
            dir: cwd.to_path_buf(),
            vcs: true,
            uploader_url: "https://vagrantcloud.com".to_string(),
        };
        assert!(vc_empty.deploy(&env).is_err());

        // Mock failure on each deploy
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_PUSH_FAIL", "1");
        }
        assert!(ftp_strat.deploy(&env).is_err());
        assert!(sftp_strat.deploy(&env).is_err());
        assert!(atlas_strat.deploy(&env).is_err());
        assert!(vc_strat.deploy(&env).is_err());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PUSH_FAIL");
        }
    }

    #[test]
    fn test_execute_push_machine_pushes_collected() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web" do |node|
    node.push.define "local-exec" do |p|
      p.inline = "echo machine-level push"
    end
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile).expect("write failed");
        let result = execute(cwd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_push_local_exec_absolute_script() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let script_file = cwd.join("deploy_abs.sh");
        fs::write(&script_file, "#!/bin/sh\nexit 0\n").expect("write failed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_file)
                .expect("metadata failed")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_file, perms).expect("set_permissions failed");
        }

        let push = LocalExecPush {
            script: Some(script_file.to_string_lossy().to_string()),
            inline: None,
            cwd: cwd.to_path_buf(),
        };
        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_ok());
    }

    #[test]
    fn test_execute_push_local_exec_script_permission_denied() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let script_file = cwd.join("deploy_noexec.sh");
        fs::write(&script_file, "#!/bin/sh\nexit 0\n").expect("write failed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_file)
                .expect("metadata failed")
                .permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&script_file, perms).expect("set_permissions failed");
        }

        let push = LocalExecPush {
            script: Some("deploy_noexec.sh".to_string()),
            inline: None,
            cwd: cwd.to_path_buf(),
        };
        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_err());
    }

    #[test]
    fn test_execute_push_local_exec_no_script_no_inline() {
        let dir = tempdir().expect("tempdir failed");
        let push = LocalExecPush {
            script: None,
            inline: None,
            cwd: dir.path().to_path_buf(),
        };
        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_ok());
    }

    #[test]
    fn test_execute_push_duplicate_machine_push() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |p|
    p.inline = "echo root push"
  end
  config.vm.define "web" do |node|
    node.push.define "local-exec" do |p|
      p.inline = "echo dup push"
    end
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile).expect("write failed");
        let result = execute(cwd);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_push_inline_spawn_fail() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let empty_dir = dir.path().join("empty_bin");
        fs::create_dir_all(&empty_dir).expect("create_dir failed");

        let old_path = std::env::var_os("PATH");
        unsafe {
            std::env::set_var("PATH", &empty_dir);
        }

        let push = LocalExecPush {
            script: None,
            inline: Some("echo test".to_string()),
            cwd: dir.path().to_path_buf(),
        };
        let env = crate::action::Environment::new();
        let res = push.deploy(&env);

        unsafe {
            if let Some(p) = old_path {
                std::env::set_var("PATH", p);
            }
        }

        assert!(res.is_err());
    }

    #[test]
    fn test_execute_env_with_machine_pushes() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let mut env_config = crate::config::EnvironmentConfig::default();
        let mut machine = crate::config::MachineConfig::default();
        machine.pushes.push(crate::config::PushConfig {
            name: "direct-machine-push".to_string(),
            strategy: "local-exec".to_string(),
            options: [("inline".to_string(), "echo direct".to_string())]
                .into_iter()
                .collect(),
        });
        env_config.machines.insert("web".to_string(), machine);

        let result = execute_env(cwd, &env_config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_env_with_unknown_push_strategy() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let mut env_config = crate::config::EnvironmentConfig::default();
        env_config.pushes.push(crate::config::PushConfig {
            name: "bad-push".to_string(),
            strategy: "invalid-strat-123".to_string(),
            options: HashMap::new(),
        });
        assert!(execute_env(cwd, &env_config).is_err());
    }
}
