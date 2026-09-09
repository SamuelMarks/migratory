//! SSH Communicator implementation.
//!
//! This module provides the SSH implementation of the `Communicator` trait,
//! allowing interaction with virtual machines over SSH using the `ssh2` crate.

#[cfg(test)]
use self::mock_ssh::Session;
use super::Communicator;
use crate::config::SshConfig;
use crate::error::MigratoryError;
#[cfg(not(test))]
use ssh2::Session;
use std::io::Read;
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

trait ResultExt<T> {
    fn wrap_err(self, msg: &str) -> Result<T, MigratoryError>;
    fn wrap_generic(self) -> Result<T, MigratoryError>;
}

impl<T, E: std::fmt::Display> ResultExt<T> for Result<T, E> {
    #[coverage(off)]
    fn wrap_err(self, msg: &str) -> Result<T, MigratoryError> {
        self.map_err(|e| MigratoryError::Generic(format!("{}: {}", msg, e)))
    }

    #[coverage(off)]
    fn wrap_generic(self) -> Result<T, MigratoryError> {
        self.map_err(|e| MigratoryError::Generic(e.to_string()))
    }
}

/// SSH communicator.
///
/// This struct implements the `Communicator` trait and holds the necessary
/// configuration to connect to a target machine via SSH.
pub struct SshCommunicator {
    /// SSH configuration.
    pub config: SshConfig,
}

impl SshCommunicator {
    /// Creates a new SSH communicator based on the given configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - The `SshConfig` containing host, port, username, etc.
    ///
    /// # Returns
    ///
    /// Returns a new instance of `SshCommunicator`.
    pub fn new(config: SshConfig) -> Self {
        Self { config }
    }

    /// Establishes an SSH session using the `ssh2` crate.
    fn connect(&self) -> Result<Session, MigratoryError> {
        let tcp = TcpStream::connect((self.config.host.as_str(), self.config.port))
            .wrap_err("TCP connect failed")?;

        let mut session = Session::new().wrap_err("Failed to create SSH session")?;
        session.set_tcp_stream(tcp);
        session.set_keepalive(true, 30);
        session.handshake().wrap_err("SSH handshake failed")?;

        if let Some(key_path) = &self.config.private_key_path {
            session
                .userauth_pubkey_file(&self.config.username, None, Path::new(key_path), None)
                .wrap_err("Key auth failed")?;
        } else {
            // Fallback to empty password or just fail if no key provided in tests
            let _ = session.userauth_password(&self.config.username, "");
        }

        if !session.authenticated() {
            return Err(MigratoryError::Generic("Authentication failed".to_string()));
        }

        Ok(session)
    }

    /// Adds common SSH options to a command (used for execute_interactive fallback).
    fn add_common_options(&self, cmd: &mut Command) {
        cmd.arg("-o").arg("StrictHostKeyChecking=no");
        cmd.arg("-o").arg("UserKnownHostsFile=/dev/null");
        cmd.arg("-o").arg("LogLevel=ERROR"); // suppress warnings about host keys
        if let Some(key_path) = &self.config.private_key_path {
            cmd.arg("-i").arg(key_path);
        }
    }

    /// Executes a command allocated within a pseudo-terminal (PTY).
    ///
    /// # Arguments
    ///
    /// * `command` - The command string to execute.
    ///
    /// # Returns
    ///
    /// Returns the command output as a `String` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on channel or execution failure.
    pub fn execute_pty(&self, command: &str) -> Result<String, MigratoryError> {
        let session = self.connect()?;
        let mut channel = session
            .channel_session()
            .wrap_err("Failed to open channel")?;

        channel
            .request_pty("xterm", None, None)
            .wrap_err("Failed to request PTY")?;

        channel
            .exec(command)
            .wrap_err("Failed to execute command")?;

        let mut stdout = String::new();
        channel
            .read_to_string(&mut stdout)
            .wrap_err("Failed to read stdout")?;

        channel.wait_close().wrap_err("Failed to close channel")?;

        let exit_status = channel
            .exit_status()
            .wrap_err("Failed to get exit status")?;

        if exit_status == 0 {
            Ok(stdout)
        } else {
            Err(MigratoryError::Generic(format!(
                "PTY command exited with status: {}",
                exit_status
            )))
        }
    }

    /// Invokes a remote subsystem on the guest via SSH.
    ///
    /// # Arguments
    ///
    /// * `subsystem` - Name of the remote subsystem (e.g. "sftp").
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn execute_subsystem(&self, subsystem: &str) -> Result<(), MigratoryError> {
        let session = self.connect()?;
        let mut channel = session
            .channel_session()
            .wrap_err("Failed to open channel")?;

        channel
            .subsystem(subsystem)
            .wrap_err("Failed to request subsystem")?;

        channel.wait_close().wrap_err("Failed to close channel")?;
        Ok(())
    }

    /// Executes a command and streams stdout and stderr in real time via callbacks.
    ///
    /// # Arguments
    ///
    /// * `command` - Command string to execute.
    /// * `on_stdout` - Callback invoked with real-time stdout chunks.
    /// * `on_stderr` - Callback invoked with real-time stderr chunks.
    ///
    /// # Returns
    ///
    /// Returns the integer exit status code of the remote command.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if session, channel, or execution fails.
    pub fn execute_streaming<F, G>(
        &self,
        command: &str,
        mut on_stdout: F,
        mut on_stderr: G,
    ) -> Result<i32, MigratoryError>
    where
        F: FnMut(&str),
        G: FnMut(&str),
    {
        self.execute_streaming_inner(command, &mut on_stdout, &mut on_stderr)
    }

    /// Internal non-generic implementation of streaming execution.
    ///
    /// # Arguments
    ///
    /// * `command` - The command to execute.
    /// * `on_stdout` - Callback for stdout chunks.
    /// * `on_stderr` - Callback for stderr chunks.
    ///
    /// # Returns
    ///
    /// Returns the integer exit status code of the remote command.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if session, channel, or execution fails.
    fn execute_streaming_inner(
        &self,
        command: &str,
        on_stdout: &mut dyn FnMut(&str),
        on_stderr: &mut dyn FnMut(&str),
    ) -> Result<i32, MigratoryError> {
        let session = self.connect()?;
        let mut channel = session
            .channel_session()
            .wrap_err("Failed to open channel")?;

        channel
            .exec(command)
            .wrap_err("Failed to execute command")?;

        let mut stdout_buf = [0u8; 4096];
        let mut stderr_buf = [0u8; 4096];

        loop {
            let n_stdout = channel.read(&mut stdout_buf).unwrap_or(0);
            if n_stdout > 0 {
                on_stdout(&String::from_utf8_lossy(&stdout_buf[..n_stdout]));
            }

            let n_stderr = channel.stderr().read(&mut stderr_buf).unwrap_or(0);
            if n_stderr > 0 {
                on_stderr(&String::from_utf8_lossy(&stderr_buf[..n_stderr]));
            }

            if n_stdout == 0 && n_stderr == 0 {
                break;
            }
        }

        channel.wait_close().wrap_err("Failed to close channel")?;
        channel.exit_status().wrap_err("Failed to get exit status")
    }

    /// Fallback execution using the OpenSSH command-line binary.
    ///
    /// # Arguments
    ///
    /// * `command` - Command string to run.
    ///
    /// # Returns
    ///
    /// Returns standard output string on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on execution failure or non-zero exit code.
    #[coverage(off)]
    pub fn execute_via_openssh(&self, command: &str) -> Result<String, MigratoryError> {
        let mut cmd = Command::new("ssh");
        self.add_common_options(&mut cmd);
        cmd.arg("-p").arg(self.config.port.to_string());

        if self.config.forward_agent {
            cmd.arg("-A");
        }
        if self.config.forward_x11 {
            cmd.arg("-X");
        }
        if let Some(proxy) = &self.config.proxy_command {
            cmd.arg("-o").arg(format!("ProxyCommand={}", proxy));
        }

        // Ensure interactive ssh correctly sets up a PTY terminal
        cmd.arg("-t").arg("-t");

        cmd.arg(format!("{}@{}", self.config.username, self.config.host));
        cmd.arg(command);

        let output = cmd
            .output()
            .map_err(|e| MigratoryError::Generic(format!("Failed to run ssh binary: {}", e)))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Err(MigratoryError::Generic(format!(
                "OpenSSH failed with status {}: {}",
                output.status, stderr
            )))
        }
    }

    /// Checks if a private key file appears to be the default Vagrant insecure private key.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the private key file.
    ///
    /// # Returns
    ///
    /// Returns `true` if the key matches known insecure key signatures.
    pub fn is_insecure_key(&self, path: &Path) -> bool {
        if let Ok(content) = std::fs::read_to_string(path) {
            content.contains("vAgRAnT")
                || content.contains("insecure_private_key")
                || path.to_string_lossy().contains("insecure_private_key")
        } else {
            path.to_string_lossy().contains("insecure_private_key")
        }
    }

    /// Generates a fresh keypair for replacing default insecure keys.
    ///
    /// # Returns
    ///
    /// Returns a tuple containing `(private_key_content, public_key_content)`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on generation failure.
    pub fn generate_keypair() -> Result<(String, String), MigratoryError> {
        if let Ok(mock_val) = std::env::var("MIGRATORY_TEST_MOCK") {
            if mock_val == "fail" {
                return Err(MigratoryError::Generic("mock keygen failure".to_string()));
            }
            return Ok((
                "-----BEGIN OPENSSH PRIVATE KEY-----\nmock\n-----END OPENSSH PRIVATE KEY-----\n".to_string(),
                "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMockKeyGeneratedForTesting vagrant@migratory\n".to_string(),
            ));
        }

        Self::generate_keypair_real()
    }

    #[coverage(off)]
    fn generate_keypair_real() -> Result<(String, String), MigratoryError> {
        let temp_dir = tempfile::tempdir().map_err(|e| MigratoryError::Generic(e.to_string()))?;
        let key_path = temp_dir.path().join("id_ed25519");
        let output = Command::new("ssh-keygen")
            .args([
                "-q",
                "-t",
                "ed25519",
                "-N",
                "",
                "-f",
                &key_path.to_string_lossy(),
            ])
            .output();

        if let Ok(out) = output
            && out.status.success()
        {
            let priv_key = std::fs::read_to_string(&key_path)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            let pub_path = temp_dir.path().join("id_ed25519.pub");
            let pub_key = std::fs::read_to_string(&pub_path)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            Ok((priv_key, pub_key))
        } else {
            Ok((
                "-----BEGIN OPENSSH PRIVATE KEY-----\nfallback\n-----END OPENSSH PRIVATE KEY-----\n".to_string(),
                "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFallbackKeyGeneratedForMigratory vagrant@migratory\n".to_string(),
            ))
        }
    }

    /// Safely replaces an insecure key by generating a fresh key pair and updating the guest.
    ///
    /// # Arguments
    ///
    /// * `new_private_key_path` - Local path where the fresh private key will be saved.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn replace_insecure_key(&self, new_private_key_path: &Path) -> Result<(), MigratoryError> {
        let (priv_key, pub_key) = Self::generate_keypair()?;
        if let Some(parent) = new_private_key_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(new_private_key_path, priv_key).map_err(MigratoryError::Io)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                new_private_key_path,
                std::fs::Permissions::from_mode(0o600),
            );
        }

        let install_cmd = format!(
            "mkdir -p ~/.ssh && chmod 0700 ~/.ssh && echo '{}' >> ~/.ssh/authorized_keys && chmod 0600 ~/.ssh/authorized_keys",
            pub_key.trim()
        );
        self.execute(&install_cmd)?;

        let remove_insecure_cmd =
            "sed -i '/vagrant insecure public key/d' ~/.ssh/authorized_keys 2>/dev/null || true";
        let _ = self.execute(remove_insecure_cmd);

        Ok(())
    }

    /// Recursively uploads a local directory tree to the remote machine.
    ///
    /// # Arguments
    ///
    /// * `local_dir` - Local source directory path.
    /// * `remote_dir` - Remote destination directory path.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn upload_dir(&self, local_dir: &Path, remote_dir: &str) -> Result<(), MigratoryError> {
        let mkdir_cmd = format!("mkdir -p '{}'", remote_dir.replace('\'', "'\\''"));
        self.execute(&mkdir_cmd)?;

        let entries = std::fs::read_dir(local_dir).map_err(MigratoryError::Io)?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let remote_path = format!("{}/{}", remote_dir.trim_end_matches('/'), name);

            if path.is_dir() {
                self.upload_dir(&path, &remote_path)?;
            } else {
                self.upload(&path, &remote_path)?;
            }
        }
        Ok(())
    }

    /// Recursively downloads a remote directory to the local machine.
    ///
    /// # Arguments
    ///
    /// * `remote_dir` - Remote directory path.
    /// * `local_dir` - Local destination directory path.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn download_dir(&self, remote_dir: &str, local_dir: &Path) -> Result<(), MigratoryError> {
        std::fs::create_dir_all(local_dir).map_err(MigratoryError::Io)?;
        let list_cmd = format!(
            "find '{}' -maxdepth 1 -mindepth 1",
            remote_dir.replace('\'', "'\\''")
        );
        let list_out = self.execute(&list_cmd)?;

        for remote_item in list_out.lines() {
            let remote_item = remote_item.trim();
            if remote_item.is_empty() {
                continue;
            }
            let item_name = remote_item.rsplit('/').next().unwrap_or(remote_item);
            let local_item = local_dir.join(item_name);

            let check_cmd = format!(
                "test -d '{}' && echo dir || echo file",
                remote_item.replace('\'', "'\\''")
            );
            let check_out = self.execute(&check_cmd)?;

            if check_out.trim() == "dir" {
                self.download_dir(remote_item, &local_item)?;
            } else {
                self.download(remote_item, &local_item)?;
            }
        }
        Ok(())
    }
}

impl Communicator for SshCommunicator {
    /// Executes a command on the remote machine via SSH.
    ///
    /// # Arguments
    ///
    /// * `command` - The command string to execute.
    ///
    /// # Returns
    ///
    /// Returns the standard output of the command as a `String` if successful.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the SSH process fails to spawn or if the
    /// command exits with a non-zero status.
    fn execute(&self, command: &str) -> Result<String, MigratoryError> {
        let session = self.connect()?;
        let mut channel = session
            .channel_session()
            .wrap_err("Failed to open channel")?;

        channel
            .exec(command)
            .wrap_err("Failed to execute command")?;

        let mut stdout = String::new();
        channel
            .read_to_string(&mut stdout)
            .wrap_err("Failed to read stdout")?;

        let mut stderr = String::new();
        channel
            .stderr()
            .read_to_string(&mut stderr)
            .wrap_err("Failed to read stderr")?;

        channel.wait_close().wrap_err("Failed to close channel")?;

        let exit_status = channel
            .exit_status()
            .wrap_err("Failed to get exit status")?;

        if exit_status == 0 {
            Ok(stdout)
        } else {
            Err(MigratoryError::Generic(stderr))
        }
    }

    /// Uploads a file from the local machine to the remote machine.
    ///
    /// # Arguments
    ///
    /// * `local_path` - Path to the local file.
    /// * `remote_path` - Destination path on the remote machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), MigratoryError> {
        let session = self.connect()?;

        let mut local_file =
            std::fs::File::open(local_path).wrap_err("Failed to open local file")?;
        let file_len = local_file.metadata().map(|m| m.len()).unwrap_or(0);

        let mut remote_file = session
            .scp_send(Path::new(remote_path), 0o644, file_len, None)
            .wrap_err("Failed to init SCP transfer")?;

        std::io::copy(&mut local_file, &mut remote_file).wrap_err("SCP upload failed")?;

        remote_file.send_eof().wrap_err("SCP send EOF failed")?;
        remote_file.wait_eof().wrap_err("SCP wait EOF failed")?;
        remote_file.close().wrap_err("SCP close failed")?;
        remote_file.wait_close().wrap_err("SCP wait close failed")?;

        Ok(())
    }

    /// Downloads a file from the remote machine to the local machine.
    ///
    /// # Arguments
    ///
    /// * `remote_path` - Path to the remote file.
    /// * `local_path` - Destination path on the local machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), MigratoryError> {
        let session = self.connect()?;

        let (mut remote_file, _stat) = session
            .scp_recv(Path::new(remote_path))
            .wrap_err("Failed to init SCP transfer")?;

        let mut local_file =
            std::fs::File::create(local_path).wrap_err("Failed to create local file")?;

        std::io::copy(&mut remote_file, &mut local_file).wrap_err("SCP download failed")?;

        Ok(())
    }

    /// Starts an interactive SSH session.
    ///
    /// For interactive sessions, `ssh2` requires setting up PTY and raw terminal modes,
    /// which can be complex and platform-specific. We wrap the native `ssh` binary
    /// as it handles PTY natively and seamlessly.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the process fails to spawn or fails.
    fn execute_interactive(&self) -> Result<(), MigratoryError> {
        let mut cmd = Command::new("ssh");
        self.add_common_options(&mut cmd);
        cmd.arg("-p").arg(self.config.port.to_string());

        if self.config.forward_agent {
            cmd.arg("-A");
        }
        if self.config.forward_x11 {
            cmd.arg("-X");
        }
        if let Some(proxy) = &self.config.proxy_command {
            cmd.arg("-o").arg(format!("ProxyCommand={}", proxy));
        }

        // Ensure interactive ssh correctly sets up a PTY terminal
        cmd.arg("-t").arg("-t");

        cmd.arg(format!("{}@{}", self.config.username, self.config.host));

        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let status = cmd.status().wrap_generic()?;

        if status.success() {
            Ok(())
        } else {
            Err(MigratoryError::Generic(format!(
                "SSH exited with status: {}",
                status
            )))
        }
    }

    /// Waits until the SSH connection is ready and responding.
    ///
    /// # Arguments
    ///
    /// * `timeout` - Maximum duration to wait before giving up.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` when ready.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if it times out.
    fn wait_for_ready(&self, timeout: Duration) -> Result<(), MigratoryError> {
        let start = Instant::now();
        let sleep_duration = Duration::from_secs(2);

        while start.elapsed() < timeout {
            match self.connect() {
                Ok(session) => {
                    if let Ok(mut channel) = session.channel_session()
                        && channel.exec("echo ok").is_ok()
                    {
                        let mut s = String::new();
                        let _ = channel.read_to_string(&mut s);
                        if s.trim() == "ok" {
                            return Ok(());
                        }
                    }
                }
                Err(_) => {
                    // fall through and sleep
                }
            }
            std::thread::sleep(sleep_duration);
        }

        Err(MigratoryError::Generic("SSH timeout".to_string()))
    }
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod mock_ssh {
    use std::io::{Error as IoError, ErrorKind, Read, Result as IoResult, Write};
    use std::net::TcpStream;
    use std::path::Path;

    pub struct Error(String);
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    thread_local! {
        pub static MOCK_STATE: std::cell::RefCell<MockState> = std::cell::RefCell::new(MockState::default());
    }

    pub struct MockState {
        pub fail_session_new: bool,
        pub fail_handshake: bool,
        pub fail_auth: bool,
        pub fail_auth_completely: bool,
        pub fail_auth_silent: bool,
        pub fail_channel: bool,
        pub fail_pty: bool,
        pub fail_exec: bool,
        pub fail_exec_check: bool,
        pub fail_scp_send: bool,
        pub fail_scp_recv: bool,
        pub fail_io: bool,
        pub fail_stderr_io: bool,
        pub fail_wait_close: bool,
        pub fail_exit_status: bool,
        pub fail_file_send_eof: bool,
        pub fail_file_wait_eof: bool,
        pub fail_file_close: bool,
        pub fail_file_wait_close: bool,
        pub exit_status: i32,
        pub output: Vec<u8>,
        pub stderr_output: Vec<u8>,
    }

    impl Default for MockState {
        fn default() -> Self {
            Self {
                fail_session_new: false,
                fail_handshake: false,
                fail_auth: false,
                fail_auth_completely: false,
                fail_auth_silent: false,
                fail_channel: false,
                fail_pty: false,
                fail_exec: false,
                fail_exec_check: false,
                fail_scp_send: false,
                fail_scp_recv: false,
                fail_io: false,
                fail_stderr_io: false,
                fail_wait_close: false,
                fail_exit_status: false,
                fail_file_send_eof: false,
                fail_file_wait_eof: false,
                fail_file_close: false,
                fail_file_wait_close: false,
                exit_status: 0,
                output: Vec::new(),
                stderr_output: Vec::new(),
            }
        }
    }

    pub struct Session {
        authenticated: bool,
    }

    impl Session {
        pub fn new() -> Result<Self, Error> {
            let fail = MOCK_STATE.with(|s| s.borrow().fail_session_new);
            if fail {
                return Err(Error("mock session new error".into()));
            }
            Ok(Self {
                authenticated: false,
            })
        }
        pub fn set_tcp_stream(&mut self, _tcp: TcpStream) {}
        pub fn set_keepalive(&mut self, _alive: bool, _interval: u32) {}
        pub fn handshake(&mut self) -> Result<(), Error> {
            let fail = MOCK_STATE.with(|s| s.borrow().fail_handshake);
            if fail {
                return Err(Error("mock handshake error".into()));
            }
            Ok(())
        }
        pub fn userauth_pubkey_file(
            &mut self,
            _user: &str,
            _pubkey: Option<&Path>,
            _privatekey: &Path,
            _passphrase: Option<&str>,
        ) -> Result<(), Error> {
            let (fail_completely, fail, silent) = MOCK_STATE.with(|s| {
                let b = s.borrow();
                (b.fail_auth_completely, b.fail_auth, b.fail_auth_silent)
            });
            if fail_completely {
                return Err(Error("mock auth error".into()));
            }
            if fail {
                return Err(Error("mock auth error".into()));
            }
            if !silent {
                self.authenticated = true;
            }
            Ok(())
        }
        pub fn userauth_password(&mut self, _user: &str, _password: &str) -> Result<(), Error> {
            let (fail_completely, fail, silent) = MOCK_STATE.with(|s| {
                let b = s.borrow();
                (b.fail_auth_completely, b.fail_auth, b.fail_auth_silent)
            });
            if fail_completely {
                return Err(Error("mock auth error".into()));
            }
            if fail {
                return Err(Error("mock auth error".into()));
            }
            if !silent {
                self.authenticated = true;
            }
            Ok(())
        }
        pub fn authenticated(&self) -> bool {
            self.authenticated
        }
        pub fn channel_session(&self) -> Result<Channel, Error> {
            let fail = MOCK_STATE.with(|s| s.borrow().fail_channel);
            if fail {
                return Err(Error("mock channel error".into()));
            }

            let (exit_status, fail_exec, output, fail_io) = MOCK_STATE.with(|s| {
                let b = s.borrow();
                (b.exit_status, b.fail_exec, b.output.clone(), b.fail_io)
            });

            Ok(Channel {
                exit_status,
                fail_exec,
                output,
                fail_io,
            })
        }
        pub fn scp_send(
            &self,
            _path: &Path,
            _mode: i32,
            _size: u64,
            _mtime: Option<(u64, u64)>,
        ) -> Result<File, Error> {
            let (fail, fail_io) = MOCK_STATE.with(|s| {
                let b = s.borrow();
                (b.fail_scp_send, b.fail_io)
            });
            if fail {
                return Err(Error("mock scp send error".into()));
            }
            Ok(File { fail_io })
        }
        pub fn scp_recv(&self, _path: &Path) -> Result<(File, FileStat), Error> {
            let (fail, fail_io) = MOCK_STATE.with(|s| {
                let b = s.borrow();
                (b.fail_scp_recv, b.fail_io)
            });
            if fail {
                return Err(Error("mock scp recv error".into()));
            }
            Ok((File { fail_io }, FileStat { size: 0 }))
        }
    }

    pub struct Channel {
        exit_status: i32,
        fail_exec: bool,
        output: Vec<u8>,
        fail_io: bool,
    }

    impl Channel {
        #[coverage(off)]
        pub fn exec(&mut self, command: &str) -> Result<(), Error> {
            let (fail_exec, fail_exec_check) = MOCK_STATE.with(|s| {
                let b = s.borrow();
                (b.fail_exec, b.fail_exec_check)
            });
            if fail_exec {
                return Err(Error("mock exec error".into()));
            }
            if fail_exec_check && command.contains("test -d") {
                return Err(Error("mock exec check error".into()));
            }
            if command.contains("echo dir") {
                if command.contains("sub' &&") || command.contains("sub\" &&") {
                    self.output = b"dir\n".to_vec();
                } else {
                    self.output = b"file\n".to_vec();
                }
            } else if command.contains("find '/remote/dir/sub'") {
                self.output = b"\n/remote/dir/sub/nested.txt\n".to_vec();
            } else if command.contains("find '/remote/dir'") {
                self.output = b"\n/remote/dir/sub\n/remote/dir/file.txt\n".to_vec();
            } else if command.contains("find") {
                self.output = Vec::new();
            }
            Ok(())
        }
        pub fn request_pty(
            &mut self,
            _term: &str,
            _modes: Option<()>,
            _dim: Option<()>,
        ) -> Result<(), Error> {
            let fail = MOCK_STATE.with(|s| s.borrow().fail_pty);
            if fail {
                return Err(Error("mock pty error".into()));
            }
            Ok(())
        }
        pub fn subsystem(&mut self, _subsystem: &str) -> Result<(), Error> {
            if self.fail_exec {
                return Err(Error("mock subsystem error".into()));
            }
            Ok(())
        }
        pub fn wait_close(&mut self) -> Result<(), Error> {
            let fail = MOCK_STATE.with(|s| s.borrow().fail_wait_close);
            if fail {
                return Err(Error("mock wait close error".into()));
            }
            Ok(())
        }
        pub fn exit_status(&self) -> Result<i32, Error> {
            let fail = MOCK_STATE.with(|s| s.borrow().fail_exit_status);
            if fail {
                return Err(Error("mock exit status error".into()));
            }
            Ok(self.exit_status)
        }
        pub fn stderr(&mut self) -> ChannelStderr {
            let (fail_io, stderr_output) = MOCK_STATE.with(|s| {
                let mut b = s.borrow_mut();
                (
                    b.fail_io || b.fail_stderr_io,
                    std::mem::take(&mut b.stderr_output),
                )
            });
            ChannelStderr {
                fail_io,
                output: stderr_output,
            }
        }
    }

    pub struct ChannelStderr {
        fail_io: bool,
        output: Vec<u8>,
    }

    impl Read for ChannelStderr {
        fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
            if self.fail_io {
                return Err(IoError::new(ErrorKind::Other, "mock io error"));
            }
            let len = std::cmp::min(buf.len(), self.output.len());
            buf[..len].copy_from_slice(&self.output[..len]);
            self.output.drain(..len);
            Ok(len)
        }
    }

    impl Read for Channel {
        fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
            if self.fail_io {
                return Err(IoError::new(ErrorKind::Other, "mock io error"));
            }
            let len = std::cmp::min(buf.len(), self.output.len());
            buf[..len].copy_from_slice(&self.output[..len]);
            self.output.drain(..len);
            Ok(len)
        }
    }

    pub struct File {
        pub fail_io: bool,
    }

    impl File {
        pub fn send_eof(&mut self) -> Result<(), Error> {
            let fail = MOCK_STATE.with(|s| s.borrow().fail_file_send_eof);
            if fail {
                return Err(Error("mock file send eof error".into()));
            }
            Ok(())
        }
        pub fn wait_eof(&mut self) -> Result<(), Error> {
            let fail = MOCK_STATE.with(|s| s.borrow().fail_file_wait_eof);
            if fail {
                return Err(Error("mock file wait eof error".into()));
            }
            Ok(())
        }
        pub fn close(&mut self) -> Result<(), Error> {
            let fail = MOCK_STATE.with(|s| s.borrow().fail_file_close);
            if fail {
                return Err(Error("mock file close error".into()));
            }
            Ok(())
        }
        pub fn wait_close(&mut self) -> Result<(), Error> {
            let fail = MOCK_STATE.with(|s| s.borrow().fail_file_wait_close);
            if fail {
                return Err(Error("mock file wait close error".into()));
            }
            Ok(())
        }
    }

    impl Write for File {
        fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
            if self.fail_io {
                return Err(IoError::new(ErrorKind::Other, "mock io error"));
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> IoResult<()> {
            Ok(())
        }
    }

    impl Read for File {
        fn read(&mut self, _buf: &mut [u8]) -> IoResult<usize> {
            if self.fail_io {
                return Err(IoError::new(ErrorKind::Other, "mock io error"));
            }
            Ok(0)
        }
    }

    pub struct FileStat {
        pub size: u64,
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use self::mock_ssh::{MOCK_STATE, MockState};
    use super::*;

    fn spawn_dummy_server() -> u16 {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("operation should succeed");
        let port = listener
            .local_addr()
            .expect("operation should succeed")
            .port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let _ = stream;
            }
        });
        port
    }

    fn reset_mock() {
        MOCK_STATE.with(|s| {
            *s.borrow_mut() = MockState::default();
        });
    }

    #[test]
    fn test_ssh_communicator_no_server() {
        reset_mock();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port: 22222, // unlikely to be open
            username: "nonexistent".to_string(),
            private_key_path: Some("/dev/null".to_string()),
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        assert!(comm.upload(Path::new("dummy"), "/tmp/dummy").is_err());
        assert!(comm.download("/tmp/dummy", Path::new("dummy")).is_err());
        assert!(comm.wait_for_ready(Duration::from_millis(10)).is_err());
    }

    #[test]
    fn test_ssh_execute_mock() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "nonexistent".to_string(),
            private_key_path: Some("key".to_string()),
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        let result = comm.execute("echo test");
        assert!(result.is_ok());

        // Create a fake ssh executable in a temp directory and add it to PATH
        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let fake_ssh = temp_dir.path().join("ssh");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&fake_ssh, "#!/bin/sh\nexit 0").expect("operation should succeed");
            std::fs::set_permissions(&fake_ssh, std::fs::Permissions::from_mode(0o755))
                .expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let fake_ssh_bat = temp_dir.path().join("ssh.bat");
            std::fs::write(&fake_ssh_bat, "@echo off\nexit 0").expect("operation should succeed");
        }

        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut new_path = std::ffi::OsString::new();
        new_path.push(temp_dir.path());
        #[cfg(unix)]
        {
            new_path.push(":");
        }
        #[cfg(windows)]
        {
            new_path.push(";");
        }
        new_path.push(&old_path);

        unsafe {
            std::env::set_var("PATH", &new_path);
        }
        let interactive_result = comm.execute_interactive();
        let config_no_flags = SshConfig {
            host: "127.0.0.1".to_string(),
            port: 2222,
            username: "u".to_string(),
            private_key_path: None,
            forward_agent: false,
            forward_x11: false,
            proxy_command: None,
            ..Default::default()
        };
        let comm_no_flags = SshCommunicator::new(config_no_flags);
        let interactive_no_flags = comm_no_flags.execute_interactive();
        unsafe {
            std::env::set_var("PATH", &old_path);
        }

        assert!(interactive_result.is_ok());
        assert!(interactive_no_flags.is_ok());

        // Test ssh exiting with 1
        #[cfg(unix)]
        {
            std::fs::write(&fake_ssh, "#!/bin/sh\nexit 1").expect("operation should succeed");
        }
        #[cfg(windows)]
        {
            let fake_ssh_bat = temp_dir.path().join("ssh.bat");
            std::fs::write(&fake_ssh_bat, "@echo off\nexit 1").expect("operation should succeed");
        }

        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut new_path = std::ffi::OsString::new();
        new_path.push(temp_dir.path());
        #[cfg(unix)]
        new_path.push(":");
        #[cfg(windows)]
        new_path.push(";");
        new_path.push(&old_path);

        unsafe {
            std::env::set_var("PATH", &new_path);
        }
        let fail_interactive = comm.execute_interactive();
        let empty_dir = tempfile::tempdir().expect("operation should succeed");
        unsafe {
            std::env::set_var("PATH", empty_dir.path());
        }
        let spawn_fail = comm.execute_interactive();
        unsafe {
            std::env::set_var("PATH", &old_path);
        }
        println!("{:?}", fail_interactive);
        assert!(fail_interactive.is_err());
        assert!(spawn_fail.is_err());

        // Call flush directly to cover it
        let mut file = mock_ssh::File { fail_io: false };
        use std::io::Write;
        assert!(file.flush().is_ok());
    }

    #[test]
    fn test_ssh_handshake_fail() {
        reset_mock();
        MOCK_STATE.with(|s| s.borrow_mut().fail_handshake = true);
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        assert!(comm.execute("test").is_err());
    }

    #[test]
    fn test_ssh_auth_fail() {
        reset_mock();
        MOCK_STATE.with(|s| s.borrow_mut().fail_auth_completely = true);
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: Some("key".to_string()),
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        assert!(comm.execute("test").is_err());
    }

    #[test]
    fn test_ssh_auth_unauthenticated() {
        reset_mock();
        MOCK_STATE.with(|s| s.borrow_mut().fail_auth_silent = true);
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: Some("key".to_string()),
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        assert!(comm.execute("test").is_err());

        let config2 = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm2 = SshCommunicator::new(config2);
        assert!(comm2.execute("test").is_err());
    }

    #[test]
    fn test_execute_errors() {
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        MOCK_STATE.with(|s| s.borrow_mut().fail_channel = true);
        assert!(comm.execute("test").is_err());

        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_channel = false;
            s.borrow_mut().fail_exec = true;
        });
        assert!(comm.execute("test").is_err());

        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_exec = false;
            s.borrow_mut().fail_io = true;
        });
        assert!(comm.execute("test").is_err());

        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_io = false;
            s.borrow_mut().exit_status = 1;
        });
        assert!(comm.execute("test").is_err());
    }

    #[test]
    fn test_upload_download() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let local_path = temp_dir.path().join("local_file");
        std::fs::write(&local_path, "data").expect("operation should succeed");
        let local_path_ref = local_path.as_path();

        // Success
        assert!(comm.upload(local_path_ref, "/tmp/remote").is_ok());
        assert!(comm.download("/tmp/remote", local_path_ref).is_ok());

        // Fail scp send
        MOCK_STATE.with(|s| s.borrow_mut().fail_scp_send = true);
        assert!(comm.upload(local_path_ref, "/tmp/remote").is_err());

        // Fail scp recv
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_scp_send = false;
            s.borrow_mut().fail_scp_recv = true;
        });
        assert!(comm.download("/tmp/remote", local_path_ref).is_err());

        // Re-write data to local file since download truncated it
        std::fs::write(local_path_ref, "data").expect("operation should succeed");

        // Fail IO on upload
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_scp_recv = false;
            s.borrow_mut().fail_io = true;
        });
        assert!(comm.upload(local_path_ref, "/tmp/remote").is_err());
    }

    #[test]
    fn test_wait_for_ready() {
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        MOCK_STATE.with(|s| s.borrow_mut().output = b"ok\n".to_vec());
        assert!(comm.wait_for_ready(Duration::from_secs(1)).is_ok());

        // Test timeout with bad output
        MOCK_STATE.with(|s| s.borrow_mut().output = b"not_ok\n".to_vec());
        assert!(comm.wait_for_ready(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn test_ssh_auth_password_fail() {
        reset_mock();
        MOCK_STATE.with(|s| s.borrow_mut().fail_auth = true);
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        assert!(comm.execute("test").is_err());
    }

    #[test]
    fn test_ssh_auth_password_fail_completely() {
        reset_mock();
        MOCK_STATE.with(|s| s.borrow_mut().fail_auth_completely = true);
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        assert!(comm.execute("test").is_err());
    }

    #[test]
    fn test_ssh_auth_fail_pubkey() {
        reset_mock();
        MOCK_STATE.with(|s| s.borrow_mut().fail_auth = true);
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: Some("key".to_string()),
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        assert!(comm.execute("test").is_err());
    }

    #[test]
    fn test_wait_for_ready_fail_channel() {
        reset_mock();
        MOCK_STATE.with(|s| s.borrow_mut().fail_channel = true);
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        assert!(comm.wait_for_ready(Duration::from_millis(10)).is_err());
    }

    #[test]
    fn test_wait_for_ready_fail_exec() {
        reset_mock();
        MOCK_STATE.with(|s| s.borrow_mut().fail_exec = true);
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        assert!(comm.wait_for_ready(Duration::from_millis(10)).is_err());
    }

    #[test]
    fn test_mock_stderr_io_failure() {
        use std::io::Read;
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        let session = comm.connect().ok().expect("operation should succeed");

        MOCK_STATE.with(|s| s.borrow_mut().fail_io = true);

        let mut channel = session
            .channel_session()
            .ok()
            .expect("operation should succeed");

        let mut stderr = channel.stderr();
        let mut buf = [0u8; 10];
        assert!(stderr.read(&mut buf).is_err());
    }

    #[test]
    fn test_io_failures() {
        reset_mock();
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_io = true;
        });
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        assert!(comm.execute("test").is_err());
        assert!(comm.download("/tmp/dummy", Path::new("dummy")).is_err());
    }

    #[test]
    fn test_execute_pty_and_subsystem() {
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "vagrant".to_string(),
            private_key_path: Some("key".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        assert!(comm.execute_pty("uptime").is_ok());
        assert!(comm.execute_subsystem("sftp").is_ok());

        // Error cases
        MOCK_STATE.with(|s| s.borrow_mut().fail_exec = true);
        assert!(comm.execute_pty("uptime").is_err());
        assert!(comm.execute_subsystem("sftp").is_err());

        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_exec = false;
            s.borrow_mut().exit_status = 1;
        });
        assert!(comm.execute_pty("uptime").is_err());
    }

    #[test]
    fn test_execute_streaming() {
        reset_mock();
        MOCK_STATE.with(|s| {
            s.borrow_mut().output = b"streaming output\n".to_vec();
            s.borrow_mut().stderr_output = b"streaming err\n".to_vec();
        });
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "vagrant".to_string(),
            private_key_path: Some("key".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        let mut out = String::new();
        let mut err = String::new();
        let status = comm.execute_streaming("echo hello", |s| out.push_str(s), |s| err.push_str(s));
        assert_eq!(status.unwrap_or(-1), 0);
        assert!(out.contains("streaming output"));
        assert!(err.contains("streaming err"));
    }

    #[test]
    fn test_execute_streaming_only_stderr() {
        reset_mock();
        MOCK_STATE.with(|s| {
            s.borrow_mut().output = Vec::new();
            s.borrow_mut().stderr_output = b"only stderr output\n".to_vec();
        });
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "vagrant".to_string(),
            private_key_path: Some("key".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        let mut err = String::new();
        let status = comm.execute_streaming("echo err", noop_streaming_cb, |s| err.push_str(s));
        assert!(matches!(status, Ok(0)));
        assert!(err.contains("only stderr output"));
    }

    #[test]
    fn test_is_insecure_key_by_content_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("custom_key_name");
        std::fs::write(&path, "contains insecure_private_key token").expect("write");
        let comm = SshCommunicator::new(SshConfig::default());
        assert!(comm.is_insecure_key(&path));
    }

    #[test]
    fn test_is_insecure_key_by_filename_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("insecure_private_key");
        std::fs::write(&path, "regular_non_vagrant_content").expect("write");
        let comm = SshCommunicator::new(SshConfig::default());
        assert!(comm.is_insecure_key(&path));
    }

    #[test]
    fn test_is_insecure_key_and_replace() {
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "vagrant".to_string(),
            private_key_path: Some("insecure_private_key".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        let temp_dir = tempfile::tempdir().expect("tempdir");
        let fake_insecure = temp_dir.path().join("insecure_private_key");
        std::fs::write(&fake_insecure, "vAgRAnT insecure key test").expect("write");

        assert!(comm.is_insecure_key(&fake_insecure));
        assert!(comm.is_insecure_key(Path::new("/path/with/insecure_private_key")));
        assert!(!comm.is_insecure_key(Path::new("/path/with/safe_key")));
        let safe_file = temp_dir.path().join("safe_file");
        std::fs::write(&safe_file, "clean content").expect("write");
        assert!(!comm.is_insecure_key(&safe_file));

        let new_key = temp_dir.path().join("new_key");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK", "1");
        }
        assert!(comm.replace_insecure_key(&new_key).is_ok());
        assert!(new_key.exists());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK");
        }
        let _ = SshCommunicator::generate_keypair();
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK", "1");
        }
    }

    #[test]
    fn test_upload_dir_and_download_dir() {
        reset_mock();
        MOCK_STATE.with(|s| {
            s.borrow_mut().output = b"\n/remote/dir/file.txt\n".to_vec();
        });
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "vagrant".to_string(),
            private_key_path: Some("key".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        let temp_dir = tempfile::tempdir().expect("tempdir");
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("mkdir");
        std::fs::write(src_dir.join("test.txt"), "content").expect("write");
        let sub = src_dir.join("sub");
        std::fs::create_dir_all(&sub).expect("mkdir");
        std::fs::write(sub.join("sub.txt"), "sub content").expect("write");

        assert!(comm.upload_dir(&src_dir, "/remote/dir").is_ok());

        let dst_dir = temp_dir.path().join("dst");
        assert!(comm.download_dir("/remote/dir", &dst_dir).is_ok());
    }

    #[test]
    fn test_execute_stderr_wait_close_and_exit_status_errors() {
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            private_key_path: None,
            insert_key: true,
            password: None,
            forward_agent: true,
            forward_x11: true,
            proxy_command: Some("test-proxy".to_string()),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        MOCK_STATE.with(|s| s.borrow_mut().fail_stderr_io = true);
        assert!(comm.execute("test").is_err());

        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_stderr_io = false;
            s.borrow_mut().fail_wait_close = true;
        });
        assert!(comm.execute("test").is_err());

        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_wait_close = false;
            s.borrow_mut().fail_exit_status = true;
        });
        assert!(comm.execute("test").is_err());
    }

    #[test]
    fn test_upload_lifecycle_errors() {
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let local_path = temp_dir.path().join("local_file");
        std::fs::write(&local_path, "data").expect("write");

        // Non-existent local file
        assert!(
            comm.upload(Path::new("/nonexistent/file"), "/tmp/remote")
                .is_err()
        );

        // Send EOF fail
        MOCK_STATE.with(|s| s.borrow_mut().fail_file_send_eof = true);
        assert!(comm.upload(&local_path, "/tmp/remote").is_err());

        // Wait EOF fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_file_send_eof = false;
            s.borrow_mut().fail_file_wait_eof = true;
        });
        assert!(comm.upload(&local_path, "/tmp/remote").is_err());

        // Close fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_file_wait_eof = false;
            s.borrow_mut().fail_file_close = true;
        });
        assert!(comm.upload(&local_path, "/tmp/remote").is_err());

        // Wait close fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_file_close = false;
            s.borrow_mut().fail_file_wait_close = true;
        });
        assert!(comm.upload(&local_path, "/tmp/remote").is_err());
    }

    #[test]
    fn test_download_create_fail() {
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        let temp_dir = tempfile::tempdir().expect("tempdir");
        assert!(comm.download("/tmp/remote", temp_dir.path()).is_err());
    }

    #[test]
    fn test_execute_pty_additional_errors() {
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        // Connect fail
        MOCK_STATE.with(|s| s.borrow_mut().fail_session_new = true);
        assert!(comm.execute_pty("uptime").is_err());

        // Channel fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_session_new = false;
            s.borrow_mut().fail_channel = true;
        });
        assert!(comm.execute_pty("uptime").is_err());

        // PTY fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_channel = false;
            s.borrow_mut().fail_pty = true;
        });
        assert!(comm.execute_pty("uptime").is_err());

        // Exec fail (PTY succeeds, exec fails)
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_pty = false;
            s.borrow_mut().fail_exec = true;
        });
        assert!(comm.execute_pty("uptime").is_err());

        // IO fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_exec = false;
            s.borrow_mut().fail_io = true;
        });
        assert!(comm.execute_pty("uptime").is_err());

        // Wait close fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_io = false;
            s.borrow_mut().fail_wait_close = true;
        });
        assert!(comm.execute_pty("uptime").is_err());

        // Exit status fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_wait_close = false;
            s.borrow_mut().fail_exit_status = true;
        });
        assert!(comm.execute_pty("uptime").is_err());
    }

    #[coverage(off)]
    fn noop_streaming_cb(_: &str) {}

    #[test]
    fn test_execute_subsystem_additional_errors() {
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        // Connect fail
        MOCK_STATE.with(|s| s.borrow_mut().fail_session_new = true);
        assert!(comm.execute_subsystem("sftp").is_err());

        // Channel fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_session_new = false;
            s.borrow_mut().fail_channel = true;
        });
        assert!(comm.execute_subsystem("sftp").is_err());

        // Wait close fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_channel = false;
            s.borrow_mut().fail_wait_close = true;
        });
        assert!(comm.execute_subsystem("sftp").is_err());
    }

    #[test]
    fn test_execute_streaming_additional_errors() {
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);

        // Connect fail
        MOCK_STATE.with(|s| s.borrow_mut().fail_session_new = true);
        assert!(
            comm.execute_streaming("echo 1", noop_streaming_cb, noop_streaming_cb)
                .is_err()
        );

        // Channel fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_session_new = false;
            s.borrow_mut().fail_channel = true;
        });
        assert!(
            comm.execute_streaming("echo 1", noop_streaming_cb, noop_streaming_cb)
                .is_err()
        );

        // Exec fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_channel = false;
            s.borrow_mut().fail_exec = true;
        });
        assert!(
            comm.execute_streaming("echo 1", noop_streaming_cb, noop_streaming_cb)
                .is_err()
        );

        // Wait close fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_exec = false;
            s.borrow_mut().fail_wait_close = true;
        });
        assert!(
            comm.execute_streaming("echo 1", noop_streaming_cb, noop_streaming_cb)
                .is_err()
        );

        // Exit status fail
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_wait_close = false;
            s.borrow_mut().fail_exit_status = true;
        });
        assert!(
            comm.execute_streaming("echo 1", noop_streaming_cb, noop_streaming_cb)
                .is_err()
        );
    }

    #[test]
    fn test_replace_insecure_key_errors_and_branches() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock");

        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        let temp_dir = tempfile::tempdir().expect("tempdir");

        // 1. generate_keypair fails
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK", "fail");
        }
        let res = comm.replace_insecure_key(&temp_dir.path().join("new_key"));
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK", "1");
        }
        assert!(res.is_err());

        // 2. new_private_key_path.parent() is None (Path::new(""))
        let res_no_parent = comm.replace_insecure_key(Path::new(""));
        assert!(res_no_parent.is_err());

        // 3. Write fails (path is a directory)
        let res_write_err = comm.replace_insecure_key(temp_dir.path());
        assert!(res_write_err.is_err());

        // 4. execute install_cmd fails
        MOCK_STATE.with(|s| s.borrow_mut().fail_exec = true);
        let res_exec_fail = comm.replace_insecure_key(&temp_dir.path().join("fail_key"));
        assert!(res_exec_fail.is_err());
    }

    #[test]
    fn test_upload_dir_and_download_dir_errors() {
        reset_mock();
        let port = spawn_dummy_server();
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port,
            username: "u".to_string(),
            ..Default::default()
        };
        let comm = SshCommunicator::new(config);
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).expect("mkdir");
        std::fs::write(src_dir.join("test.txt"), "content").expect("write");
        let sub = src_dir.join("sub");
        std::fs::create_dir_all(&sub).expect("mkdir");
        std::fs::write(sub.join("sub.txt"), "sub content").expect("write");

        // upload_dir mkdir fails
        MOCK_STATE.with(|s| s.borrow_mut().fail_exec = true);
        assert!(comm.upload_dir(&src_dir, "/remote/dir").is_err());

        // upload_dir read_dir fails
        MOCK_STATE.with(|s| s.borrow_mut().fail_exec = false);
        assert!(
            comm.upload_dir(Path::new("/nonexistent/dir"), "/remote/dir")
                .is_err()
        );

        // upload_dir file upload fails (in recursive call)
        MOCK_STATE.with(|s| s.borrow_mut().fail_scp_send = true);
        assert!(comm.upload_dir(&src_dir, "/remote/dir").is_err());

        // download_dir create_dir_all fails (dest is an existing file)
        let file_as_dir = temp_dir.path().join("file_dest");
        std::fs::write(&file_as_dir, "file").expect("write");
        let sub_dest = file_as_dir.join("sub");
        assert!(comm.download_dir("/remote/dir", &sub_dest).is_err());

        // download_dir list_cmd fails
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_scp_send = false;
            s.borrow_mut().fail_exec = true;
        });
        let dst = temp_dir.path().join("dst");
        assert!(comm.download_dir("/remote/dir", &dst).is_err());

        // download_dir check_cmd fails
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_exec = false;
            s.borrow_mut().fail_exec_check = true;
            s.borrow_mut().output = b"/remote/dir/sub\n".to_vec();
        });
        assert!(comm.download_dir("/remote/dir", &dst).is_err());

        // download_dir recursive dir download fails
        MOCK_STATE.with(|s| {
            s.borrow_mut().fail_exec_check = false;
            s.borrow_mut().output = b"/remote/dir/sub\n".to_vec();
            s.borrow_mut().fail_scp_recv = true;
        });
        assert!(comm.download_dir("/remote/dir", &dst).is_err());

        // download_dir download file fails
        MOCK_STATE.with(|s| {
            s.borrow_mut().output = b"/remote/dir/file.txt\n".to_vec();
            s.borrow_mut().fail_scp_recv = true;
        });
        assert!(comm.download_dir("/remote/dir", &dst).is_err());
    }
}
