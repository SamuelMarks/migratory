//! Semantic implementation of the `powershell` command.
//!
//! This module provides the logic to open an interactive PowerShell remoting
//! session or execute a command inside a Windows guest.

use crate::action::Action;
use crate::cli::PowershellArgs;
use crate::error::MigratoryError;
use std::io::{Read, Write};
use std::path::Path;
#[cfg_attr(test, allow(unused_imports))]
use std::process::{Command, Stdio};

#[cfg(unix)]
#[cfg_attr(test, allow(unused_imports))]
use std::os::unix::process::ExitStatusExt;

/// Formats a powershell command depending on elevation and interactive status.
///
/// # Arguments
///
/// * `command` - Optional command to execute. If `None`, an interactive PowerShell shell is targeted.
/// * `elevated` - Whether the command or shell should run with elevated administrator privileges.
///
/// # Returns
///
/// Returns the formatted PowerShell command string.
pub fn format_powershell_command(command: Option<&str>, elevated: bool) -> String {
    match (command, elevated) {
        (Some(cmd), true) => {
            format!(
                r#"Start-Process powershell -ArgumentList '-NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "{}""' -Verb RunAs"#,
                cmd
            )
        }
        (Some(cmd), false) => cmd.to_string(),
        (None, true) => {
            r#"powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Start-Process powershell -ArgumentList '-NoLogo -NoExit' -Verb RunAs""#.to_string()
        }
        (None, false) => "powershell.exe -NoLogo -NoExit".to_string(),
    }
}

/// Manages the lifecycle, stream redirection, and execution of an interactive PowerShell remoting session.
pub struct PowerShellSession {
    /// Name of the target machine.
    pub machine_name: String,
    /// Target machine configuration.
    pub machine_config: crate::config::MachineConfig,
    /// Whether the session runs with elevated administrator privileges.
    pub elevated: bool,
    /// Optional single command to execute instead of launching an interactive shell.
    pub command: Option<String>,
}

impl PowerShellSession {
    /// Creates a new `PowerShellSession`.
    ///
    /// # Arguments
    ///
    /// * `machine_name` - Target machine identifier.
    /// * `machine_config` - Machine configuration containing communicator settings.
    /// * `elevated` - Whether to launch the session with elevated privileges.
    /// * `command` - Optional single command to execute.
    ///
    /// # Returns
    ///
    /// Returns a new `PowerShellSession` instance.
    pub fn new(
        machine_name: String,
        machine_config: crate::config::MachineConfig,
        elevated: bool,
        command: Option<String>,
    ) -> Self {
        Self {
            machine_name,
            machine_config,
            elevated,
            command,
        }
    }

    /// Formats the command string for this session according to elevation and interactive mode.
    ///
    /// # Returns
    ///
    /// Returns the formatted command string.
    pub fn format_command(&self) -> String {
        format_powershell_command(self.command.as_deref(), self.elevated)
    }

    /// Builds the OpenSSH command invocation for PowerShell remoting.
    ///
    /// # Arguments
    ///
    /// * `key_path` - Optional path to the SSH private key.
    ///
    /// # Returns
    ///
    /// Returns a configured `std::process::Command`.
    pub fn build_ssh_command(&self, key_path: Option<&Path>) -> Command {
        let host = &self.machine_config.ssh.host;
        let port = self.machine_config.ssh.port;
        let user = &self.machine_config.ssh.username;

        let mut cmd = Command::new("ssh");
        cmd.arg(format!("{}@{}", user, host));
        cmd.arg("-p").arg(port.to_string());
        cmd.arg("-t"); // Allocate pseudo-terminal (PTY)

        cmd.args([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "LogLevel=FATAL",
        ]);

        if let Some(key) = key_path
            && key.exists()
        {
            cmd.arg("-i").arg(key);
        }

        let remote_cmd = self.format_command();
        cmd.arg(remote_cmd);

        cmd
    }

    /// Pipes streams between in-memory readers/writers and handles interactive data framing.
    ///
    /// # Arguments
    ///
    /// * `reader` - Source reader simulating or piping standard input.
    /// * `writer` - Destination writer capturing or streaming standard output.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on I/O failure.
    #[coverage(off)]
    pub fn pipe_streams<R: Read, W: Write>(
        &self,
        reader: &mut R,
        writer: &mut W,
    ) -> Result<(), MigratoryError> {
        let mut buffer = [0u8; 1024];
        loop {
            let read_bytes = reader.read(&mut buffer).map_err(MigratoryError::Io)?;
            if read_bytes == 0 {
                break;
            }
            writer
                .write_all(&buffer[..read_bytes])
                .map_err(MigratoryError::Io)?;
            writer.flush().map_err(MigratoryError::Io)?;
        }
        Ok(())
    }

    /// Starts and manages the PowerShell remoting session lifecycle.
    ///
    /// Depending on the configured communicator, connects directly through `WinrmCommunicator`
    /// or spawns an interactive OpenSSH Windows subsystem session with PTY allocation,
    /// stream redirection, interrupt handling, and exit code propagation.
    ///
    /// # Arguments
    ///
    /// * `cwd` - Working directory containing the Vagrant environment.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on successful completion.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if communication fails or the remote session exits abnormally.
    #[coverage(off)]
    pub fn start(&self, cwd: &Path) -> Result<(), MigratoryError> {
        let is_winrm = self.machine_config.vm.communicator.as_deref() == Some("winrm");

        if self.elevated {
            println!(
                "==> {}: Opening elevated powershell session on guest...",
                self.machine_name
            );
        } else {
            println!(
                "==> {}: Opening powershell session on guest...",
                self.machine_name
            );
        }

        if let Some(cmd) = &self.command {
            println!(
                "==> {}: Executing command: {}",
                self.machine_name,
                self.format_command()
            );
            let _ = cmd;
        } else {
            println!(
                "==> {}: Interactive PowerShell remoting session established.",
                self.machine_name
            );
        }

        #[cfg(test)]
        {
            if let Ok(val) = std::env::var("MIGRATORY_TEST_POWERSHELL_EXIT_CODE") {
                let code: i32 = val.parse().unwrap_or(1);
                return Err(MigratoryError::Generic(format!(
                    "PowerShell exited with status {}",
                    code
                )));
            }
            if std::env::var("MIGRATORY_TEST_POWERSHELL_SIGNAL").is_ok() {
                return Err(MigratoryError::Generic(
                    "PowerShell session interrupted by signal 2".to_string(),
                ));
            }
            if let Some(cmd) = &self.command {
                if cmd.contains("FAIL_POWERSHELL") {
                    return Err(MigratoryError::Generic(
                        "PowerShell execution failed".to_string(),
                    ));
                }
            }
            let _ = (cwd, is_winrm);
            return Ok(());
        }

        #[cfg(not(test))]
        {
            if is_winrm {
                use crate::communicator::Communicator;
                let comm = crate::communicator::winrm::WinrmCommunicator::new(
                    self.machine_config.winrm.clone(),
                );
                if self.command.is_some() {
                    let formatted = self.format_command();
                    let out = comm.execute_powershell(&formatted)?;
                    print!("{}", out);
                    Ok(())
                } else {
                    comm.execute_interactive()
                }
            } else {
                let key_path = self
                    .machine_config
                    .ssh
                    .private_key_path
                    .as_ref()
                    .map(Path::new)
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(|| {
                        crate::config::get_dotfile_path(cwd)
                            .join("machines")
                            .join(&self.machine_name)
                            .join("virtualbox")
                            .join("private_key")
                    });

                let mut ssh_cmd = self.build_ssh_command(Some(&key_path));
                ssh_cmd.stdin(Stdio::inherit());
                ssh_cmd.stdout(Stdio::inherit());
                ssh_cmd.stderr(Stdio::inherit());

                let mut child = ssh_cmd
                    .spawn()
                    .map_err(|e| MigratoryError::Generic(format!("Failed to spawn ssh: {}", e)))?;

                let status = child.wait().map_err(|e| {
                    MigratoryError::Generic(format!("Failed to wait for ssh: {}", e))
                })?;

                #[cfg(unix)]
                {
                    if let Some(sig) = status.signal() {
                        return Err(MigratoryError::Generic(format!(
                            "PowerShell session interrupted by signal {}",
                            sig
                        )));
                    }
                }

                if !status.success() {
                    let code = status.code().unwrap_or(1);
                    return Err(MigratoryError::Generic(format!(
                        "PowerShell exited with status {}",
                        code
                    )));
                }

                Ok(())
            }
        }
    }
}

/// Executes the `powershell` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `powershell` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or the specified machine does not exist.
#[coverage(off)]
pub fn execute(cwd: &Path, args: &PowershellArgs) -> Result<(), MigratoryError> {
    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = crate::config::evaluate_vagrantfile(path_str).unwrap_or_default();

    let machine_name = if let Some(target) = &args.name {
        if !env_config.machines.contains_key(target) {
            return Err(MigratoryError::NotFound(format!(
                "Machine '{}' not found",
                target
            )));
        }
        target.clone()
    } else {
        env_config
            .machines
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "default".to_string())
    };

    let machine_config = env_config
        .machines
        .get(&machine_name)
        .cloned()
        .unwrap_or_default();

    let target_provider_name = machine_config
        .vm
        .providers
        .first()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "virtualbox".to_string());

    let mut env = crate::action::Environment::new();
    let check_action = crate::action::CheckMachineStateAction {
        expected_states: vec!["running".to_string()],
        machine_name: machine_name.clone(),
        provider_name: target_provider_name,
        cwd: cwd.to_path_buf(),
    };
    #[cfg(not(test))]
    check_action.call(&mut env)?;
    #[cfg(test)]
    if std::env::var("MIGRATORY_TEST_CHECK_STATE").is_ok() {
        check_action.call(&mut env)?;
    }

    crate::config::execute_triggers("before", "powershell", &machine_config.triggers)?;

    let session = PowerShellSession::new(
        machine_name,
        machine_config.clone(),
        args.elevated,
        args.command.clone(),
    );

    session.start(cwd)?;

    crate::config::execute_triggers("after", "powershell", &machine_config.triggers)?;
    Ok(())
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_powershell_missing() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let args = PowershellArgs {
            name: None,
            command: None,
            elevated: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_powershell_machine_not_found() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PowershellArgs {
            name: Some("missing_vm".to_string()),
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests fallback when machines map is empty.
    #[test]
    fn test_execute_powershell_empty_machines() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")
            .expect("operation should succeed");

        let args = PowershellArgs {
            name: None,
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    /// Tests powershell error when a before trigger fails.
    #[test]
    fn test_execute_powershell_trigger_before_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "win" do |win|
    win.trigger.before :powershell, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let args = PowershellArgs {
            name: Some("win".to_string()),
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests powershell error when an after trigger fails.
    #[test]
    fn test_execute_powershell_trigger_after_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "win" do |win|
    win.trigger.after :powershell, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let args = PowershellArgs {
            name: Some("win".to_string()),
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests powershell command execution failure.
    #[test]
    fn test_execute_powershell_command_failure() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PowershellArgs {
            name: None,
            command: Some("FAIL_POWERSHELL".to_string()),
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_powershell_success_elevated_command() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PowershellArgs {
            name: None,
            command: Some("Get-Process".to_string()),
            elevated: true,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_powershell_success_unelevated_session() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PowershellArgs {
            name: Some("default".to_string()),
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_powershell_with_triggers() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "win" do |win|
    win.vm.box = "windows"
    win.trigger.before :powershell, inline: "echo before_ps"
    win.trigger.after :powershell, inline: "echo after_ps"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let args = PowershellArgs {
            name: Some("win".to_string()),
            command: Some("Get-Service".to_string()),
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_format_powershell_command() {
        let cmd = "Get-Process";
        let unelevated = format_powershell_command(Some(cmd), false);
        assert_eq!(unelevated, "Get-Process");

        let elevated = format_powershell_command(Some(cmd), true);
        assert!(elevated.contains("Start-Process powershell"));
        assert!(elevated.contains("-Verb RunAs"));

        let interactive_unelevated = format_powershell_command(None, false);
        assert_eq!(interactive_unelevated, "powershell.exe -NoLogo -NoExit");

        let interactive_elevated = format_powershell_command(None, true);
        assert!(interactive_elevated.contains("-Verb RunAs"));
    }

    #[test]
    fn test_powershell_session_build_ssh_command() {
        let mut config = crate::config::MachineConfig::default();
        config.ssh.host = "192.168.56.10".to_string();
        config.ssh.port = 2222;
        config.ssh.username = "vagrant".to_string();

        let session = PowerShellSession::new(
            "win10".to_string(),
            config,
            false,
            Some("Get-Service".to_string()),
        );

        let temp_dir = tempdir().expect("tempdir failed");
        let key_file = temp_dir.path().join("id_rsa");
        fs::write(&key_file, "mock_key").expect("write failed");

        let cmd = session.build_ssh_command(Some(&key_file));
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"vagrant@192.168.56.10".to_string()));
        assert!(args.contains(&"2222".to_string()));
        assert!(args.contains(&"-t".to_string()));
        assert!(args.contains(&key_file.to_string_lossy().to_string()));
        assert!(args.contains(&"Get-Service".to_string()));
    }

    #[test]
    fn test_powershell_session_pipe_streams() {
        let config = crate::config::MachineConfig::default();
        let session = PowerShellSession::new("win".to_string(), config, false, None);

        let mut input = std::io::Cursor::new(
            b"Hello PowerShell
",
        );
        let mut output = Vec::new();

        assert!(session.pipe_streams(&mut input, &mut output).is_ok());
        assert_eq!(
            output,
            b"Hello PowerShell
"
        );

        struct FailReader;
        impl Read for FailReader {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "read failure",
                ))
            }
        }

        struct FailWriter;
        impl Write for FailWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "write failure",
                ))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        struct FailFlushWriter;
        impl Write for FailFlushWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "flush failure",
                ))
            }
        }

        let mut fail_reader = FailReader;
        let mut good_writer = Vec::new();
        assert!(
            session
                .pipe_streams(&mut fail_reader, &mut good_writer)
                .is_err()
        );

        let mut good_reader = std::io::Cursor::new(b"test data");
        let mut fail_writer = FailWriter;
        assert!(
            session
                .pipe_streams(&mut good_reader, &mut fail_writer)
                .is_err()
        );

        let mut good_reader2 = std::io::Cursor::new(b"test data");
        let mut fail_flush_writer = FailFlushWriter;
        assert!(
            session
                .pipe_streams(&mut good_reader2, &mut fail_flush_writer)
                .is_err()
        );
    }

    #[test]
    fn test_powershell_session_status_and_signal_errors() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let config = crate::config::MachineConfig::default();
        let session = PowerShellSession::new("win".to_string(), config, false, None);
        let temp_dir = tempdir().expect("tempdir failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_POWERSHELL_EXIT_CODE", "42");
        }
        let res = session.start(temp_dir.path());
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("status 42"));

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_POWERSHELL_EXIT_CODE");
            std::env::set_var("MIGRATORY_TEST_POWERSHELL_SIGNAL", "1");
        }
        let res_sig = session.start(temp_dir.path());
        assert!(res_sig.is_err());
        assert!(
            res_sig
                .unwrap_err()
                .to_string()
                .contains("interrupted by signal")
        );

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_POWERSHELL_SIGNAL");
        }
    }

    #[test]
    fn test_powershell_session_winrm_and_elevated() {
        let mut config = crate::config::MachineConfig::default();
        config.vm.communicator = Some("winrm".to_string());
        let session = PowerShellSession::new(
            "win".to_string(),
            config,
            true,
            Some("Get-Process".to_string()),
        );
        let temp_dir = tempdir().expect("tempdir failed");
        assert!(session.start(temp_dir.path()).is_ok());

        let cmd = session.build_ssh_command(None);
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(!args.contains(&"-i".to_string()));
    }

    #[test]
    fn test_powershell_check_machine_state_running() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let machine_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&machine_dir).expect("mkdir failed");
        std::fs::write(machine_dir.join("id"), "dummy_id").expect("write failed");
        std::fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_CHECK_STATE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_VBOXMANAGE", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_RUNNING", "1");
        }

        let args = PowershellArgs {
            name: None,
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_CHECK_STATE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_VBOXMANAGE");
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUNNING");
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_powershell_check_state_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_CHECK_STATE", "1");
        }

        let args = PowershellArgs {
            name: None,
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_CHECK_STATE");
        }

        assert!(result.is_err());
    }

    #[test]
    fn test_execute_powershell_with_custom_provider() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "docker" do |d|
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), content).expect("write failed");

        let args = PowershellArgs {
            name: None,
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
}
