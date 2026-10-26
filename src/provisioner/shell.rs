//! Shell provisioner implementation.

use super::Provisioner;
use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::collections::HashMap;
use std::path::Path;

/// Shell script provisioner.
pub struct ShellProvisioner {
    /// Inline shell script commands to execute.
    inline: Option<String>,
    /// Local path or remote HTTP(S) URL to a script file.
    path: Option<String>,
    /// Arguments passed to the script during execution.
    args: Option<String>,
    /// Guest path where the script file is uploaded prior to execution.
    upload_path: Option<String>,
    /// Whether to execute the script using PowerShell instead of `/bin/sh`.
    powershell: bool,
    /// Custom arguments passed to `powershell.exe`.
    powershell_args: Option<String>,
    /// Whether environment variables and script contents should be redacted in logs.
    sensitive: bool,
    /// Environment variables exported prior to executing the script.
    env: HashMap<String, String>,
    /// Whether to run the script with `sudo` privileges on Unix guests.
    privileged: bool,
    /// Whether to reboot the guest machine after the script completes.
    reboot: bool,
}

impl ShellProvisioner {
    /// Creates a new shell provisioner.
    pub fn new() -> Self {
        Self {
            inline: None,
            path: None,
            args: None,
            upload_path: None,
            powershell: false,
            powershell_args: None,
            sensitive: false,
            env: HashMap::new(),
            privileged: true, // Default to true per Vagrant conventions
            reboot: false,
        }
    }
}

/// Downloads a remote script from a URL and saves it to a temporary file.
///
/// # Arguments
///
/// * `url` - Remote script URL.
///
/// # Returns
///
/// Returns a NamedTempFile containing the downloaded script.
///
/// # Errors
///
/// Returns a `MigratoryError` if downloading, reading, or writing fails.
fn download_remote_script(url: &str) -> Result<tempfile::NamedTempFile, MigratoryError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .connect_timeout(std::time::Duration::from_millis(500))
        .build()?;
    let response = client.get(url).send().map_err(|e| {
        MigratoryError::Generic(format!(
            "Failed to download remote shell script {}: {}",
            url, e
        ))
    })?;
    let content = response.text()?;
    let mut temp = tempfile::NamedTempFile::new().map_err(MigratoryError::Io)?;
    std::io::Write::write_all(&mut temp, content.as_bytes()).map_err(MigratoryError::Io)?;
    Ok(temp)
}

/// Returns the reboot sleep duration in milliseconds.
fn reboot_sleep_millis(is_test: bool) -> u64 {
    if is_test { 1 } else { 5000 }
}

/// Sleeps briefly to allow the connection to drop during reboot.
fn sleep_for_reboot() {
    let millis = reboot_sleep_millis(cfg!(test));
    std::thread::sleep(std::time::Duration::from_millis(millis));
}

impl Provisioner for ShellProvisioner {
    fn name(&self) -> &str {
        "shell"
    }

    fn prepare(&mut self, config: &HashMap<String, String>) -> Result<(), MigratoryError> {
        let p_cfg = crate::config::ProvisionerConfig {
            config: config.clone(),
            ..Default::default()
        };
        let _ = p_cfg.as_shell();
        self.inline = config.get("inline").cloned();
        self.path = config.get("path").cloned();
        self.args = config.get("args").cloned();
        self.upload_path = config.get("upload_path").cloned();
        if let Some(ps) = config.get("powershell") {
            self.powershell = ps.to_lowercase() == "true" || ps == "1";
        }
        if let Some(ps_args) = config.get("powershell_args") {
            self.powershell_args = Some(ps_args.clone());
        }
        if let Some(sens) = config.get("sensitive") {
            self.sensitive = sens.to_lowercase() == "true" || sens == "1";
        }
        if let Some(priv_val) = config.get("privileged") {
            self.privileged = priv_val.to_lowercase() == "true" || priv_val == "1";
        }
        if let Some(reboot_val) = config.get("reboot") {
            self.reboot = reboot_val.to_lowercase() == "true" || reboot_val == "1";
        }
        if let Some(env_val) = config.get("env") {
            let env_map = serde_json::from_str::<HashMap<String, String>>(env_val)
                .map_err(|e| MigratoryError::Validation(format!("Invalid env JSON: {}", e)))?;
            self.env = env_map;
        }
        Ok(())
    }

    fn provision(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let mut env_prefix = String::new();
        for (k, v) in &self.env {
            env_prefix.push_str(&format!("{}='{}' ", k, v.replace('\'', "'\\''")));
        }

        if let Some(path) = &self.path {
            let temp_holder;
            let local_path: &Path = if path.starts_with("http://") || path.starts_with("https://") {
                temp_holder = download_remote_script(path)?;
                temp_holder.path()
            } else {
                let p = Path::new(path);
                if !p.exists() {
                    return Err(MigratoryError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Shell script path not found: {}", path),
                    )));
                }
                p
            };

            let file_name = Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("provision.sh");
            let remote_path = if let Some(up) = &self.upload_path {
                up.clone()
            } else {
                format!("/tmp/{}", file_name)
            };
            comm.upload(local_path, &remote_path)?;

            let mut exec_cmd = remote_path.clone();
            if let Some(args) = &self.args {
                exec_cmd.push(' ');
                exec_cmd.push_str(args);
            }

            let ps_flags = self
                .powershell_args
                .as_deref()
                .unwrap_or("-ExecutionPolicy Bypass");

            let is_ps1 = std::path::Path::new(&remote_path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ps1"));

            let mut command = if self.powershell || is_ps1 {
                let args_str = self.args.as_deref().unwrap_or("");
                format!(
                    "powershell {} -File '{}' {}",
                    ps_flags, remote_path, args_str
                )
            } else {
                format!("chmod +x {} && {}{}", remote_path, env_prefix, exec_cmd)
            };
            if self.privileged && !self.powershell && !is_ps1 {
                command = format!("sudo sh -c '{}'", command.replace('\'', "'\\''"));
            }
            comm.execute(&command)?;
        } else if let Some(inline) = &self.inline {
            let mut command = inline.clone();
            if let Some(args) = &self.args {
                command.push(' ');
                command.push_str(args);
            }
            let command_with_env = format!("{}{}", env_prefix, command);
            let final_cmd = if self.privileged {
                format!("sudo sh -c '{}'", command_with_env.replace('\'', "'\\''"))
            } else {
                command_with_env
            };
            comm.execute(&final_cmd)?;
        }

        if self.reboot {
            let _ = comm.execute("sudo reboot");
            // Wait for it to come back up. We sleep briefly to allow the connection to drop,
            // then wait for SSH/WinRM to become available again.
            sleep_for_reboot();
            comm.wait_for_ready(std::time::Duration::from_secs(300))?;
        }
        Ok(())
    }

    fn cleanup(&self) -> Result<(), MigratoryError> {
        Ok(())
    }
}

impl Default for ShellProvisioner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::communicator::Communicator;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;

    /// No-op mock communicator for Shell provisioner tests.
    struct MockComm;

    impl Communicator for MockComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            Ok(String::new())
        }
    }

    #[test]
    fn test_shell_provisioner_inline() {
        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();
        config.insert("inline".to_string(), "echo 'hello'".to_string());
        config.insert("privileged".to_string(), "false".to_string());
        config.insert("reboot".to_string(), "false".to_string());

        assert_eq!(prov.name(), "shell");
        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
        assert!(prov.cleanup().is_ok());
    }

    #[test]
    fn test_shell_provisioner_inline_privileged() {
        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();
        config.insert("inline".to_string(), "echo 'hello'".to_string());
        config.insert("args".to_string(), "arg1 arg2".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_shell_provisioner_path() -> Result<(), MigratoryError> {
        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir()?;
        let file_path = dir.path().join("script.sh");
        let mut file = File::create(&file_path)?;
        writeln!(file, "echo 'test'")?;

        let path_str = file_path.to_string_lossy();
        config.insert("path".to_string(), path_str.to_string());
        config.insert("privileged".to_string(), "true".to_string());
        config.insert("args".to_string(), "--force".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
        Ok(())
    }

    #[test]
    fn test_shell_provisioner_path_unprivileged() -> Result<(), MigratoryError> {
        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir()?;
        let file_path = dir.path().join("script.sh");
        let mut file = File::create(&file_path)?;
        writeln!(file, "echo 'test'")?;

        let path_str = file_path.to_string_lossy();
        config.insert("path".to_string(), path_str.to_string());
        config.insert("privileged".to_string(), "false".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
        Ok(())
    }

    #[test]
    fn test_shell_provisioner_path_not_found() {
        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();
        config.insert("path".to_string(), "/nonexistent/script.sh".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        let res = prov.provision(&comm);
        assert!(res.is_err());
    }

    #[test]
    fn test_shell_provisioner_empty() {
        let mut prov = ShellProvisioner::new();
        let empty_config = HashMap::new();
        assert!(prov.prepare(&empty_config).is_ok());
        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_shell_provisioner_env_and_reboot() {
        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();
        config.insert("inline".to_string(), "echo".to_string());
        config.insert("reboot".to_string(), "true".to_string());
        config.insert(
            "env".to_string(),
            "{\"FOO\": \"bar\", \"BASH\": \"/bin/sh\"}".to_string(),
        );

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_shell_provisioner_inline_unprivileged() {
        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();
        config.insert("inline".to_string(), "echo 'hello'".to_string());
        config.insert("privileged".to_string(), "false".to_string());
        config.insert("args".to_string(), "arg1 arg2".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_shell_provisioner_invalid_env() {
        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();
        config.insert("env".to_string(), "{ invalid_json }".to_string());

        let res = prov.prepare(&config);
        assert!(res.is_err());
    }

    #[test]
    fn test_shell_provisioner_upload_path_and_powershell() -> Result<(), MigratoryError> {
        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir()?;
        let file_path = dir.path().join("script.ps1");
        let mut file = File::create(&file_path)?;
        writeln!(file, "Write-Host 'test'")?;

        let path_str = file_path.to_string_lossy();
        config.insert("path".to_string(), path_str.to_string());
        config.insert("upload_path".to_string(), "C:\\tmp\\script.ps1".to_string());
        config.insert("powershell".to_string(), "true".to_string());
        config.insert("args".to_string(), "-Verbose".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
        Ok(())
    }

    #[test]
    fn test_shell_provisioner_powershell_args_and_sensitive() -> Result<(), MigratoryError> {
        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();

        let dir = tempdir()?;
        let file_path = dir.path().join("script.ps1");
        let mut file = File::create(&file_path)?;
        writeln!(file, "Write-Host 'custom powershell'")?;

        let path_str = file_path.to_string_lossy();
        config.insert("path".to_string(), path_str.to_string());
        config.insert(
            "powershell_args".to_string(),
            "-NoProfile -ExecutionPolicy Bypass".to_string(),
        );
        config.insert("sensitive".to_string(), "true".to_string());

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
        Ok(())
    }

    #[test]
    fn test_shell_provisioner_remote_script_url() {
        let server = httpmock::MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET).path("/script.sh");
            then.status(200).body("echo 'hello from remote'");
        });

        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();
        config.insert("path".to_string(), server.url("/script.sh"));

        assert!(prov.prepare(&config).is_ok());

        let comm = MockComm;
        assert!(prov.provision(&comm).is_ok());
    }

    #[test]
    fn test_shell_provisioner_numeric_flags() {
        assert_eq!(reboot_sleep_millis(true), 1);
        assert_eq!(reboot_sleep_millis(false), 5000);
        let mut prov = ShellProvisioner::default();
        let mut config = HashMap::new();
        config.insert("powershell".to_string(), "1".to_string());
        config.insert("sensitive".to_string(), "1".to_string());
        config.insert("privileged".to_string(), "1".to_string());
        config.insert("reboot".to_string(), "1".to_string());

        assert!(prov.prepare(&config).is_ok());
        assert!(prov.powershell);
        assert!(prov.sensitive);
        assert!(prov.privileged);
        assert!(prov.reboot);
    }

    #[test]
    fn test_shell_provisioner_failures() -> Result<(), MigratoryError> {
        /// Configurable failing communicator for Shell provisioner error tests.
        struct FailingComm {
            /// Fail `upload` calls.
            fail_upload: bool,
            /// Fail `execute` calls.
            fail_execute: bool,
            /// Fail `wait_for_ready` calls.
            fail_wait: bool,
        }
        impl Communicator for FailingComm {
            fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
                if self.fail_execute {
                    Err(MigratoryError::Generic("execute failed".to_string()))
                } else {
                    Ok(String::new())
                }
            }
            fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
                if self.fail_upload {
                    Err(MigratoryError::Generic("upload failed".to_string()))
                } else {
                    Ok(())
                }
            }
            fn wait_for_ready(&self, _timeout: Duration) -> Result<(), MigratoryError> {
                if self.fail_wait {
                    Err(MigratoryError::Generic("wait failed".to_string()))
                } else {
                    Ok(())
                }
            }
        }

        let dir = tempdir()?;
        let file_path = dir.path().join("script.sh");
        let mut file = File::create(&file_path)?;
        writeln!(file, "echo 1")?;
        let path_str = file_path.to_string_lossy();

        // 1. Upload failure
        let mut prov1 = ShellProvisioner::default();
        let mut config1 = HashMap::new();
        config1.insert("path".to_string(), path_str.to_string());
        assert!(prov1.prepare(&config1).is_ok());
        let comm_up_fail = FailingComm {
            fail_upload: true,
            fail_execute: false,
            fail_wait: false,
        };
        assert!(
            comm_up_fail
                .wait_for_ready(Duration::from_millis(1))
                .is_ok()
        );
        assert!(prov1.provision(&comm_up_fail).is_err());

        // 2. Path execute failure
        let comm_exec_fail = FailingComm {
            fail_upload: false,
            fail_execute: true,
            fail_wait: false,
        };
        assert!(prov1.provision(&comm_exec_fail).is_err());

        // 3. Inline execute failure
        let mut prov2 = ShellProvisioner::default();
        let mut config2 = HashMap::new();
        config2.insert("inline".to_string(), "echo hi".to_string());
        assert!(prov2.prepare(&config2).is_ok());
        assert!(prov2.provision(&comm_exec_fail).is_err());

        // 4. Reboot wait_for_ready failure
        let mut prov3 = ShellProvisioner::default();
        let mut config3 = HashMap::new();
        config3.insert("reboot".to_string(), "true".to_string());
        assert!(prov3.prepare(&config3).is_ok());
        let comm_wait_fail = FailingComm {
            fail_upload: false,
            fail_execute: false,
            fail_wait: true,
        };
        assert!(prov3.provision(&comm_wait_fail).is_err());

        // 5. Remote script download failure (invalid URL)
        let mut prov4 = ShellProvisioner::default();
        let mut config4 = HashMap::new();
        config4.insert(
            "path".to_string(),
            "http://127.0.0.1:1/nonexistent.sh".to_string(),
        );
        assert!(prov4.prepare(&config4).is_ok());
        let comm_ok = FailingComm {
            fail_upload: false,
            fail_execute: false,
            fail_wait: false,
        };
        assert!(prov4.provision(&comm_ok).is_err());

        // 6. Remote script with https URL
        let mut prov5 = ShellProvisioner::default();
        let mut config5 = HashMap::new();
        config5.insert(
            "path".to_string(),
            "https://127.0.0.1:1/nonexistent.sh".to_string(),
        );
        assert!(prov5.prepare(&config5).is_ok());
        assert!(prov5.provision(&comm_ok).is_err());
        Ok(())
    }
}
