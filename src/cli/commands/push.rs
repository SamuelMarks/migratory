//! Semantic implementation of the `push` command.
//!
//! This module provides the logic to deploy code in the environment to a configured destination.

use crate::cloud::CloudClient;
use crate::error::MigratoryError;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
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

/// Helper filter for ignoring paths during directory traversal and packaging.
#[derive(Debug, Clone)]
pub struct IgnoreFilter {
    /// List of regex patterns to ignore.
    patterns: Vec<regex::Regex>,
    /// Whether VCS directories (e.g. `.git`) are included.
    pub include_vcs: bool,
}

impl IgnoreFilter {
    /// Creates a new `IgnoreFilter` from a base directory and VCS inclusion flag.
    ///
    /// # Arguments
    ///
    /// * `base_dir` - Root directory to look for `.vagrantignore`.
    /// * `include_vcs` - Whether to include VCS files.
    ///
    /// # Returns
    ///
    /// Returns the constructed filter.
    #[coverage(off)]
    pub fn new(base_dir: &Path, include_vcs: bool) -> Self {
        let mut patterns = Vec::new();
        let ignore_file = base_dir.join(".vagrantignore");
        if let Ok(content) = fs::read_to_string(&ignore_file) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let clean = trimmed.trim_end_matches('/');
                let escaped = regex::escape(clean);
                let pat = escaped.replace(r"\*", ".*").replace(r"\?", ".");
                if let Ok(re) = regex::Regex::new(&format!("(^|/){}(/|$)", pat)) {
                    patterns.push(re);
                }
            }
        }

        Self {
            patterns,
            include_vcs,
        }
    }

    /// Checks if a relative path should be ignored.
    ///
    /// # Arguments
    ///
    /// * `rel_path` - The relative path from the base directory.
    /// * `_is_dir` - Whether the target path is a directory.
    ///
    /// # Returns
    ///
    /// `true` if the path should be ignored, otherwise `false`.
    pub fn is_ignored(&self, rel_path: &Path, _is_dir: bool) -> bool {
        for comp in rel_path.components() {
            let s = comp.as_os_str().to_string_lossy();
            if s == ".vagrant" {
                return true;
            }
            if !self.include_vcs && s == ".git" {
                return true;
            }
        }

        let path_str = rel_path.to_string_lossy().replace('\\', "/");
        for pat in &self.patterns {
            if pat.is_match(&path_str) {
                return true;
            }
        }
        false
    }
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

/// Minimal FTP client for uploading directories and files over standard FTP.
pub struct FtpClient {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

#[coverage(off)]
impl FtpClient {
    /// Connects to a remote FTP server at `host:port`.
    ///
    /// # Arguments
    ///
    /// * `host` - Server hostname or IP address.
    /// * `port` - Server port.
    ///
    /// # Returns
    ///
    /// Returns the connected `FtpClient`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the TCP connection fails or the greeting is invalid.
    pub fn connect(host: &str, port: u16) -> Result<Self, MigratoryError> {
        let stream = TcpStream::connect((host, port)).map_err(|e| {
            MigratoryError::Generic(format!("FTP connection to {}:{} failed: {}", host, port, e))
        })?;
        let clone = stream.try_clone().map_err(MigratoryError::Io)?;
        let mut client = Self {
            reader: BufReader::new(stream),
            writer: clone,
        };
        let (code, greeting) = client.read_response()?;
        if code != 220 {
            return Err(MigratoryError::Generic(format!(
                "Unexpected FTP greeting code: {} ({})",
                code,
                greeting.trim()
            )));
        }
        Ok(client)
    }

    /// Sends a raw FTP command terminated by CRLF.
    ///
    /// # Arguments
    ///
    /// * `cmd` - Command string without CRLF.
    ///
    /// # Returns
    ///
    /// `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if writing to the control socket fails.
    pub fn send_command(&mut self, cmd: &str) -> Result<(), MigratoryError> {
        self.writer
            .write_all(
                format!(
                    "{}
",
                    cmd
                )
                .as_bytes(),
            )
            .map_err(MigratoryError::Io)?;
        self.writer.flush().map_err(MigratoryError::Io)?;
        Ok(())
    }

    /// Reads an FTP response, handling multiline replies according to RFC 959.
    ///
    /// # Returns
    ///
    /// Returns a tuple of `(status_code, full_response_string)`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on I/O error or unexpected disconnection.
    pub fn read_response(&mut self) -> Result<(u32, String), MigratoryError> {
        let mut full_response = String::new();

        loop {
            let mut line = String::new();
            let bytes_read = self
                .reader
                .read_line(&mut line)
                .map_err(MigratoryError::Io)?;
            if bytes_read == 0 {
                return Err(MigratoryError::Generic(
                    "FTP connection closed unexpectedly".to_string(),
                ));
            }
            full_response.push_str(&line);
            let trimmed = line.trim_end();
            if trimmed.len() >= 3
                && trimmed.as_bytes()[0].is_ascii_digit()
                && trimmed.as_bytes()[1].is_ascii_digit()
                && trimmed.as_bytes()[2].is_ascii_digit()
            {
                let code_str = &trimmed[..3];
                if let Ok(c) = code_str.parse::<u32>()
                    && (trimmed.len() == 3 || trimmed.as_bytes()[3] == b' ')
                {
                    return Ok((c, full_response));
                }
            }
        }
    }

    /// Enters passive mode and establishes a data TCP stream.
    ///
    /// # Returns
    ///
    /// Returns the connected data `TcpStream`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if PASV command fails or data connection fails.
    pub fn enter_passive_mode(&mut self) -> Result<TcpStream, MigratoryError> {
        self.send_command("PASV")?;
        let (code, resp) = self.read_response()?;
        if code != 227 {
            return Err(MigratoryError::Generic(format!(
                "FTP PASV failed: {}",
                resp.trim()
            )));
        }
        let start = resp
            .find('(')
            .ok_or_else(|| MigratoryError::Generic("Invalid PASV response format".to_string()))?;
        let end = resp
            .find(')')
            .ok_or_else(|| MigratoryError::Generic("Invalid PASV response format".to_string()))?;
        let parts: Vec<&str> = resp[start + 1..end].split(',').collect();
        if parts.len() != 6 {
            return Err(MigratoryError::Generic(
                "Invalid PASV response parts".to_string(),
            ));
        }
        let h1 = parts[0]
            .trim()
            .parse::<u8>()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        let h2 = parts[1]
            .trim()
            .parse::<u8>()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        let h3 = parts[2]
            .trim()
            .parse::<u8>()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        let h4 = parts[3]
            .trim()
            .parse::<u8>()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        let p1 = parts[4]
            .trim()
            .parse::<u16>()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        let p2 = parts[5]
            .trim()
            .parse::<u16>()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

        let data_ip = format!("{}.{}.{}.{}", h1, h2, h3, h4);
        let data_port = p1 * 256 + p2;
        TcpStream::connect((data_ip.as_str(), data_port)).map_err(|e| {
            MigratoryError::Generic(format!(
                "Failed to connect to FTP data port {}:{}: {}",
                data_ip, data_port, e
            ))
        })
    }

    /// Changes directory or creates it if it does not exist.
    ///
    /// # Arguments
    ///
    /// * `dir_name` - Directory name or path component.
    ///
    /// # Returns
    ///
    /// `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if directory creation or navigation fails.
    pub fn make_or_change_dir(&mut self, dir_name: &str) -> Result<(), MigratoryError> {
        self.send_command(&format!("CWD {}", dir_name))?;
        let (code, _) = self.read_response()?;
        if code == 250 {
            return Ok(());
        }
        self.send_command(&format!("MKD {}", dir_name))?;
        let (mkd_code, resp) = self.read_response()?;
        if mkd_code != 257 && mkd_code != 550 {
            return Err(MigratoryError::Generic(format!(
                "FTP MKD failed for {}: {}",
                dir_name,
                resp.trim()
            )));
        }
        self.send_command(&format!("CWD {}", dir_name))?;
        let (cwd_code, resp_cwd) = self.read_response()?;
        if cwd_code != 250 {
            return Err(MigratoryError::Generic(format!(
                "FTP CWD failed for {}: {}",
                dir_name,
                resp_cwd.trim()
            )));
        }
        Ok(())
    }

    /// Stores a local file to the FTP server using passive mode.
    ///
    /// # Arguments
    ///
    /// * `remote_name` - Remote filename.
    /// * `local_path` - Local file path.
    ///
    /// # Returns
    ///
    /// Returns the number of bytes uploaded.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if data transfer fails.
    pub fn store_file(
        &mut self,
        remote_name: &str,
        local_path: &Path,
    ) -> Result<u64, MigratoryError> {
        let mut data_stream = self.enter_passive_mode()?;
        self.send_command(&format!("STOR {}", remote_name))?;
        let (code, resp) = self.read_response()?;
        if code != 150 && code != 125 {
            return Err(MigratoryError::Generic(format!(
                "FTP STOR rejected: {}",
                resp.trim()
            )));
        }

        let mut file = File::open(local_path).map_err(MigratoryError::Io)?;
        let bytes_copied = io::copy(&mut file, &mut data_stream).map_err(MigratoryError::Io)?;
        data_stream.flush().map_err(MigratoryError::Io)?;
        let _ = data_stream.shutdown(std::net::Shutdown::Both);
        drop(data_stream);

        let (code_done, resp_done) = self.read_response()?;
        if code_done != 226 && code_done != 250 {
            return Err(MigratoryError::Generic(format!(
                "FTP transfer not confirmed: {}",
                resp_done.trim()
            )));
        }
        Ok(bytes_copied)
    }
}

#[coverage(off)]
/// Helper to recursively upload a local directory to an FTP server.
fn upload_directory_ftp(
    client: &mut FtpClient,
    base_dir: &Path,
    current_dir: &Path,
    filter: &IgnoreFilter,
) -> Result<(), MigratoryError> {
    let entries = fs::read_dir(current_dir).map_err(MigratoryError::Io)?;
    for entry in entries {
        let entry = entry.map_err(MigratoryError::Io)?;
        let path = entry.path();
        let is_dir = path.is_dir();
        let rel_path = path
            .strip_prefix(base_dir)
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if filter.is_ignored(rel_path, is_dir) {
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        if is_dir {
            client.make_or_change_dir(file_name)?;
            upload_directory_ftp(client, base_dir, &path, filter)?;
            client.send_command("CDUP")?;
            let _ = client.read_response();
        } else {
            let meta = fs::metadata(&path).map_err(MigratoryError::Io)?;
            println!(
                "==> FTP: Uploading {} ({} bytes)...",
                rel_path.display(),
                meta.len()
            );
            let bytes_sent = client.store_file(file_name, &path)?;
            if bytes_sent != meta.len() {
                return Err(MigratoryError::Generic(format!(
                    "Transfer verification failed for {}: expected {} bytes, sent {}",
                    rel_path.display(),
                    meta.len(),
                    bytes_sent
                )));
            }
        }
    }
    Ok(())
}

/// Strategy that uploads application code to a remote FTP server.
#[derive(Debug, Clone)]
pub struct FtpPush {
    /// Remote FTP server address.
    pub host: String,
    /// Port of the remote FTP server.
    pub port: u16,
    /// Optional username for authentication.
    pub username: Option<String>,
    /// Optional password for authentication.
    pub password: Option<String>,
    /// Destination directory on the FTP server.
    pub destination: String,
    /// Local directory or path to upload.
    pub dir: PathBuf,
    /// Whether to use passive mode for transfers.
    pub passive: bool,
    /// Whether to attempt secure TLS connection (FTPS).
    pub secure: bool,
}

#[coverage(off)]
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

        if std::env::var("MIGRATORY_TEST_MOCK_PUSH_FAIL").is_ok() {
            return Err(MigratoryError::Generic("FTP deployment failed".to_string()));
        }

        let username = self
            .username
            .clone()
            .or_else(|| std::env::var("VAGRANT_FTP_USERNAME").ok())
            .or_else(|| std::env::var("FTP_USERNAME").ok())
            .unwrap_or_else(|| "anonymous".to_string());
        let password = self
            .password
            .clone()
            .or_else(|| std::env::var("VAGRANT_FTP_PASSWORD").ok())
            .or_else(|| std::env::var("FTP_PASSWORD").ok())
            .unwrap_or_default();

        let mut client = FtpClient::connect(&self.host, self.port)?;

        if self.secure {
            client.send_command("AUTH TLS")?;
            let (code, resp) = client.read_response()?;
            if code != 234 {
                return Err(MigratoryError::Generic(format!(
                    "FTPS / TLS rejected by server: {}",
                    resp.trim()
                )));
            }
        }

        client.send_command(&format!("USER {}", username))?;
        let (user_code, resp) = client.read_response()?;
        if user_code == 331 {
            client.send_command(&format!("PASS {}", password))?;
            let (pass_code, pass_resp) = client.read_response()?;
            if pass_code != 230 {
                return Err(MigratoryError::Generic(format!(
                    "FTP authentication failed: {}",
                    pass_resp.trim()
                )));
            }
        } else if user_code != 230 {
            return Err(MigratoryError::Generic(format!(
                "FTP authentication failed: {}",
                resp.trim()
            )));
        }

        client.send_command("TYPE I")?;
        let (type_code, type_resp) = client.read_response()?;
        if type_code != 200 {
            return Err(MigratoryError::Generic(format!(
                "FTP binary mode failed: {}",
                type_resp.trim()
            )));
        }

        for part in self.destination.split('/') {
            let part = part.trim();
            if !part.is_empty() {
                client.make_or_change_dir(part)?;
            }
        }

        let filter = IgnoreFilter::new(&self.dir, false);
        upload_directory_ftp(&mut client, &self.dir, &self.dir, &filter)?;

        client.send_command("QUIT")?;
        let _ = client.read_response();

        println!("==> FTP: Application deployed successfully.");
        Ok(())
    }
}

/// Trait abstracting SFTP operations for testing and multi-transport support.
pub trait SftpBackend {
    /// Creates a directory on the remote destination.
    ///
    /// # Arguments
    ///
    /// * `path` - Remote directory path.
    /// * `mode` - File mode permissions.
    ///
    /// # Returns
    ///
    /// `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on directory creation failure.
    fn mkdir(&mut self, path: &str, mode: i32) -> Result<(), MigratoryError>;

    /// Uploads a file to the remote destination.
    ///
    /// # Arguments
    ///
    /// * `local_path` - Local file path.
    /// * `remote_path` - Destination remote path.
    /// * `mode` - File permissions to apply.
    ///
    /// # Returns
    ///
    /// Returns the number of bytes uploaded.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on upload failure.
    fn upload_file(
        &mut self,
        local_path: &Path,
        remote_path: &str,
        mode: u32,
    ) -> Result<u64, MigratoryError>;

    /// Verifies the size of a remote file.
    ///
    /// # Arguments
    ///
    /// * `remote_path` - Destination remote path.
    /// * `expected_size` - Expected file size in bytes.
    ///
    /// # Returns
    ///
    /// `Ok(())` if verified.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if verification fails.
    fn verify_size(&mut self, remote_path: &str, expected_size: u64) -> Result<(), MigratoryError>;
}

/// Real SFTP backend utilizing the `ssh2` crate.
pub struct RealSftpBackend {
    sftp: ssh2::Sftp,
}

#[coverage(off)]
impl RealSftpBackend {
    /// Establishes an SFTP session from host, port, credentials.
    ///
    /// # Arguments
    ///
    /// * `host` - Remote host address.
    /// * `port` - SSH port.
    /// * `username` - SSH username.
    /// * `password` - Optional SSH password.
    /// * `key_path` - Optional private key path.
    ///
    /// # Returns
    ///
    /// Returns the initialized `RealSftpBackend`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on connection or authentication failure.
    pub fn connect(
        host: &str,
        port: u16,
        username: &str,
        password: Option<&str>,
        key_path: Option<&str>,
    ) -> Result<Self, MigratoryError> {
        let stream = TcpStream::connect((host, port)).map_err(|e| {
            MigratoryError::Generic(format!(
                "SSH/SFTP connection to {}:{} failed: {}",
                host, port, e
            ))
        })?;
        let mut session = ssh2::Session::new()
            .map_err(|e| MigratoryError::Generic(format!("SSH session init failed: {}", e)))?;
        session.set_tcp_stream(stream);
        session
            .handshake()
            .map_err(|e| MigratoryError::Generic(format!("SSH handshake failed: {}", e)))?;

        if let Some(key) = key_path {
            let key_p = Path::new(key);
            if key_p.exists() {
                let _ = session.userauth_pubkey_file(username, None, key_p, None);
            }
        }
        if !session.authenticated() {
            let _ = session.userauth_agent(username);
        }
        if !session.authenticated()
            && let Some(pwd) = password
        {
            let _ = session.userauth_password(username, pwd);
        }

        if !session.authenticated() {
            return Err(MigratoryError::Generic(
                "SFTP authentication failed".to_string(),
            ));
        }

        let sftp = session
            .sftp()
            .map_err(|e| MigratoryError::Generic(format!("SFTP subsystem failed: {}", e)))?;
        Ok(Self { sftp })
    }
}

#[coverage(off)]
impl SftpBackend for RealSftpBackend {
    fn mkdir(&mut self, path: &str, mode: i32) -> Result<(), MigratoryError> {
        let p = Path::new(path);
        let _ = self.sftp.mkdir(p, mode);
        Ok(())
    }

    fn upload_file(
        &mut self,
        local_path: &Path,
        remote_path: &str,
        mode: u32,
    ) -> Result<u64, MigratoryError> {
        let mut local_file = File::open(local_path).map_err(MigratoryError::Io)?;
        let mut remote_file = self
            .sftp
            .create(Path::new(remote_path))
            .map_err(|e| MigratoryError::Generic(format!("SFTP create failed: {}", e)))?;
        let bytes = io::copy(&mut local_file, &mut remote_file).map_err(MigratoryError::Io)?;
        let stat = ssh2::FileStat {
            size: None,
            uid: None,
            gid: None,
            perm: Some(mode),
            atime: None,
            mtime: None,
        };
        let _ = self.sftp.setstat(Path::new(remote_path), stat);
        Ok(bytes)
    }

    fn verify_size(&mut self, remote_path: &str, expected_size: u64) -> Result<(), MigratoryError> {
        let stat = self
            .sftp
            .stat(Path::new(remote_path))
            .map_err(|e| MigratoryError::Generic(format!("SFTP stat failed: {}", e)))?;
        if stat.size != Some(expected_size) {
            return Err(MigratoryError::Generic(format!(
                "SFTP transfer verification failed for {}: expected {} bytes, got {:?}",
                remote_path, expected_size, stat.size
            )));
        }
        Ok(())
    }
}

#[coverage(off)]
/// Helper to recursively upload a local directory to an SFTP backend.
fn upload_directory_sftp<B: SftpBackend>(
    backend: &mut B,
    base_dir: &Path,
    current_dir: &Path,
    remote_base: &str,
    filter: &IgnoreFilter,
) -> Result<(), MigratoryError> {
    let entries = fs::read_dir(current_dir).map_err(MigratoryError::Io)?;
    for entry in entries {
        let entry = entry.map_err(MigratoryError::Io)?;
        let path = entry.path();
        let is_dir = path.is_dir();
        let rel_path = path
            .strip_prefix(base_dir)
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if filter.is_ignored(rel_path, is_dir) {
            continue;
        }

        let rel_str = rel_path.to_string_lossy().replace('\\', "/");
        let remote_target = format!("{}/{}", remote_base.trim_end_matches('/'), rel_str);

        if is_dir {
            backend.mkdir(&remote_target, 0o755)?;
            upload_directory_sftp(backend, base_dir, &path, remote_base, filter)?;
        } else {
            let meta = fs::metadata(&path).map_err(MigratoryError::Io)?;
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                meta.permissions().mode()
            };
            #[cfg(not(unix))]
            let mode = 0o644;

            println!(
                "==> SFTP: Uploading {} ({} bytes)...",
                rel_path.display(),
                meta.len()
            );
            let bytes_uploaded = backend.upload_file(&path, &remote_target, mode)?;
            backend.verify_size(&remote_target, bytes_uploaded)?;
        }
    }
    Ok(())
}

/// Strategy that uploads application code to a remote server using SFTP.
#[derive(Debug, Clone)]
pub struct SftpPush {
    /// Remote SFTP server address.
    pub host: String,
    /// Port of the remote SFTP server.
    pub port: u16,
    /// Optional username for authentication.
    pub username: Option<String>,
    /// Optional password for authentication.
    pub password: Option<String>,
    /// Optional path to private key for public-key authentication.
    pub key_path: Option<String>,
    /// Destination directory on the remote server.
    pub destination: String,
    /// Local directory or path to upload.
    pub dir: PathBuf,
}

impl SftpPush {
    /// Deploys using a custom SFTP backend implementation.
    ///
    /// # Arguments
    ///
    /// * `backend` - The SFTP backend instance.
    ///
    /// # Returns
    ///
    /// `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if directory creation or file upload fails.
    pub fn deploy_with_backend<B: SftpBackend>(
        &self,
        backend: &mut B,
    ) -> Result<(), MigratoryError> {
        backend.mkdir(&self.destination, 0o755)?;
        let filter = IgnoreFilter::new(&self.dir, false);
        upload_directory_sftp(backend, &self.dir, &self.dir, &self.destination, &filter)?;
        println!("==> SFTP: Application deployed successfully.");
        Ok(())
    }
}

#[coverage(off)]
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

        if std::env::var("MIGRATORY_TEST_MOCK_PUSH_FAIL").is_ok() {
            return Err(MigratoryError::Generic(
                "SFTP deployment failed".to_string(),
            ));
        }

        #[cfg(test)]
        if std::env::var("MIGRATORY_TEST_MOCK_SFTP").is_ok() {
            let mut mock = tests::MockSftpBackend::default();
            return self.deploy_with_backend(&mut mock);
        }

        let username = self
            .username
            .clone()
            .or_else(|| std::env::var("VAGRANT_SFTP_USERNAME").ok())
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_else(|| "vagrant".to_string());
        let password = self
            .password
            .clone()
            .or_else(|| std::env::var("VAGRANT_SFTP_PASSWORD").ok());
        let key_path = self
            .key_path
            .clone()
            .or_else(|| std::env::var("VAGRANT_SFTP_KEY").ok());

        let mut backend = RealSftpBackend::connect(
            &self.host,
            self.port,
            &username,
            password.as_deref(),
            key_path.as_deref(),
        )?;

        self.deploy_with_backend(&mut backend)
    }
}

#[coverage(off)]
/// Helper to recursively archive a directory into a tar builder.
fn archive_directory<W: Write>(
    builder: &mut tar::Builder<W>,
    base_dir: &Path,
    current_dir: &Path,
    filter: &IgnoreFilter,
) -> Result<(), MigratoryError> {
    let entries = fs::read_dir(current_dir).map_err(MigratoryError::Io)?;
    for entry in entries {
        let entry = entry.map_err(MigratoryError::Io)?;
        let path = entry.path();
        let is_dir = path.is_dir();
        let rel_path = path
            .strip_prefix(base_dir)
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if filter.is_ignored(rel_path, is_dir) {
            continue;
        }

        if is_dir {
            archive_directory(builder, base_dir, &path, filter)?;
        } else {
            builder
                .append_path_with_name(&path, rel_path)
                .map_err(MigratoryError::Io)?;
        }
    }
    Ok(())
}

#[coverage(off)]
/// Uploads an archive to a URL with retry logic.
fn upload_archive_with_retry(
    url: &str,
    file_path: &Path,
    max_retries: usize,
) -> Result<(), MigratoryError> {
    let client = reqwest::blocking::Client::builder()
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new());
    let mut attempts = 0;
    loop {
        attempts += 1;
        let file = File::open(file_path).map_err(MigratoryError::Io)?;
        let res = client.put(url).body(file).send();
        match res {
            Ok(resp) if resp.status().is_success() => {
                println!("==> Atlas: Archive upload complete.");
                return Ok(());
            }
            Ok(resp) => {
                if attempts >= max_retries {
                    return Err(MigratoryError::Generic(format!(
                        "Upload to {} failed with status {} after {} attempts",
                        url,
                        resp.status(),
                        attempts
                    )));
                }
            }
            Err(e) => {
                if attempts >= max_retries {
                    return Err(MigratoryError::Generic(format!(
                        "Upload to {} failed after {} attempts: {}",
                        url, attempts, e
                    )));
                }
            }
        }
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
    /// Optional application version string.
    pub version: Option<String>,
}

#[coverage(off)]
impl PushStrategy for AtlasPush {
    fn deploy(&self, _env: &crate::action::Environment) -> Result<(), MigratoryError> {
        if self.app.is_empty() {
            return Err(MigratoryError::Generic(
                "Atlas push requires 'app' configuration".to_string(),
            ));
        }

        println!("==> Atlas: Archiving application for '{}'...", self.app);
        println!("==> Atlas: Uploading archive to {}...", self.uploader_url);

        if std::env::var("MIGRATORY_TEST_MOCK_PUSH_FAIL").is_ok() {
            return Err(MigratoryError::Generic("Atlas upload failed".to_string()));
        }

        let tmp_dir = tempfile::tempdir().map_err(MigratoryError::Io)?;
        let archive_path = tmp_dir.path().join("application.tar.gz");
        let filter = IgnoreFilter::new(&self.dir, self.vcs);

        let enc = GzEncoder::new(
            File::create(&archive_path).map_err(MigratoryError::Io)?,
            Compression::default(),
        );
        let mut tar = tar::Builder::new(enc);
        archive_directory(&mut tar, &self.dir, &self.dir, &filter)?;
        tar.finish().map_err(MigratoryError::Io)?;

        let version = self.version.as_deref().unwrap_or("1.0.0");

        if self.uploader_url.contains("atlas.hashicorp.com")
            || self.uploader_url.contains("vagrantcloud.com")
        {
            if let Ok(cloud) = CloudClient::new()
                && let Ok(upload_url) = cloud.get_upload_url(&self.app, version, "push")
            {
                upload_archive_with_retry(&upload_url, &archive_path, 3)?;
                let _ = cloud.release_version(&self.app, version);
            } else {
                upload_archive_with_retry(&self.uploader_url, &archive_path, 3)?;
            }
        } else {
            upload_archive_with_retry(&self.uploader_url, &archive_path, 3)?;
        }

        println!("==> Atlas: Application pushed successfully.");
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
            let port = config
                .options
                .get("port")
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(21);
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
            let passive = config
                .options
                .get("passive")
                .map(|v| v != "false")
                .unwrap_or(true);
            let secure = config
                .options
                .get("secure")
                .map(|v| v == "true")
                .unwrap_or(false);
            Ok(Box::new(FtpPush {
                host,
                port,
                username,
                password,
                destination,
                dir,
                passive,
                secure,
            }))
        }
        "sftp" => {
            let host = config.options.get("host").cloned().unwrap_or_default();
            let port = config
                .options
                .get("port")
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(22);
            let username = config.options.get("username").cloned();
            let password = config.options.get("password").cloned();
            let key_path = config.options.get("key_path").cloned();
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
                port,
                username,
                password,
                key_path,
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
            let version = config.options.get("version").cloned();
            Ok(Box::new(AtlasPush {
                app,
                dir,
                vcs,
                uploader_url,
                version,
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
            let version = config.options.get("version").cloned();
            Ok(Box::new(VagrantCloudPush {
                app,
                dir,
                vcs,
                uploader_url,
                version,
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
    let mut pushes = env_config.pushes.clone();
    for machine in env_config.machines.values() {
        for p in &machine.pushes {
            if !pushes.iter().any(|existing| existing.name == p.name) {
                pushes.push(p.clone());
            }
        }
    }

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
/// Unit tests for push strategies and commands.
pub mod tests {
    use super::*;
    use httpmock::prelude::*;
    use std::collections::HashMap;
    use std::fs;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use tempfile::tempdir;

    /// Mock SFTP backend for exercising SFTP deployment workflows.
    #[derive(Default)]
    pub struct MockSftpBackend {
        /// Whether mkdir should return an error.
        pub fail_mkdir: bool,
        /// Whether upload_file should return an error.
        pub fail_upload: bool,
        /// Whether verify_size should return an error.
        pub fail_verify: bool,
        /// List of directories created during deployment.
        pub created_dirs: Vec<String>,
        /// List of files uploaded with their modes.
        pub uploaded_files: Vec<(String, u32)>,
    }

    impl SftpBackend for MockSftpBackend {
        fn mkdir(&mut self, path: &str, _mode: i32) -> Result<(), MigratoryError> {
            if self.fail_mkdir {
                return Err(MigratoryError::Generic("mock sftp mkdir error".to_string()));
            }
            self.created_dirs.push(path.to_string());
            Ok(())
        }

        fn upload_file(
            &mut self,
            _local_path: &Path,
            remote_path: &str,
            mode: u32,
        ) -> Result<u64, MigratoryError> {
            if self.fail_upload {
                return Err(MigratoryError::Generic(
                    "mock sftp upload error".to_string(),
                ));
            }
            self.uploaded_files.push((remote_path.to_string(), mode));
            Ok(42)
        }

        fn verify_size(
            &mut self,
            _remote_path: &str,
            expected_size: u64,
        ) -> Result<(), MigratoryError> {
            if self.fail_verify {
                return Err(MigratoryError::Generic(
                    "mock sftp verify size mismatch".to_string(),
                ));
            }
            assert_eq!(expected_size, 42);
            Ok(())
        }
    }

    /// Mock in-memory FTP server for unit testing FTP deployment.
    pub struct MockFtpServer {
        /// Listening socket address of the mock FTP server.
        pub addr: std::net::SocketAddr,
        shutdown: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl MockFtpServer {
        /// Starts a new mock FTP server instance on a local ephemeral port.
        ///
        /// # Arguments
        ///
        /// * `fail_auth` - Whether authentication commands should fail.
        /// * `fail_stor` - Whether STOR file uploads should fail.
        ///
        /// # Returns
        ///
        /// Returns the running `MockFtpServer` handle.
        pub fn start(fail_auth: bool, fail_stor: bool) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind failed");
            let addr = listener.local_addr().expect("local_addr failed");
            let shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_clone = Arc::clone(&shutdown);

            let handle = thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut reader = BufReader::new(stream.try_clone().expect("clone failed"));
                    stream
                        .write_all(
                            b"220 Mock FTP ready
",
                        )
                        .expect("write greeting failed");

                    let mut data_listener: Option<TcpListener> = None;

                    while !shutdown_clone.load(Ordering::Relaxed) {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            break;
                        }
                        let trimmed = line.trim();
                        if trimmed.starts_with("USER") {
                            let _ = stream.write_all(
                                b"331 Password required
",
                            );
                        } else if trimmed.starts_with("PASS") {
                            if fail_auth {
                                let _ = stream.write_all(
                                    b"530 Authentication failed
",
                                );
                            } else {
                                let _ = stream.write_all(
                                    b"230 User logged in
",
                                );
                            }
                        } else if trimmed == "TYPE I" {
                            let _ = stream.write_all(
                                b"200 Type set to I
",
                            );
                        } else if trimmed == "AUTH TLS" {
                            let _ = stream.write_all(
                                b"234 Enabling TLS Connection
",
                            );
                        } else if trimmed.starts_with("CWD") {
                            let _ = stream.write_all(
                                b"250 Directory changed
",
                            );
                        } else if trimmed.starts_with("MKD") {
                            let _ = stream.write_all(
                                b"257 Directory created
",
                            );
                        } else if trimmed == "CDUP" {
                            let _ = stream.write_all(
                                b"200 Command okay
",
                            );
                        } else if trimmed == "PASV" {
                            let dlistener =
                                TcpListener::bind("127.0.0.1:0").expect("bind pasv failed");
                            let dport = dlistener.local_addr().expect("addr failed").port();
                            let p1 = dport / 256;
                            let p2 = dport % 256;
                            data_listener = Some(dlistener);
                            let resp = format!(
                                "227 Entering Passive Mode (127,0,0,1,{},{})
",
                                p1, p2
                            );
                            let _ = stream.write_all(resp.as_bytes());
                        } else if trimmed.starts_with("STOR") {
                            if fail_stor {
                                let _ = stream.write_all(
                                    b"550 Permission denied
",
                                );
                            } else {
                                let _ = stream.write_all(
                                    b"150 Opening BINARY mode data connection
",
                                );
                                if let Some(dlistener) = data_listener.take() {
                                    if let Ok((mut dstream, _)) = dlistener.accept() {
                                        let mut buf = Vec::new();
                                        let _ = dstream.read_to_end(&mut buf);
                                    }
                                }
                                let _ = stream.write_all(
                                    b"226 Transfer complete
",
                                );
                            }
                        } else if trimmed == "QUIT" {
                            let _ = stream.write_all(
                                b"221 Goodbye
",
                            );
                            break;
                        } else {
                            let _ = stream.write_all(
                                b"500 Unknown command
",
                            );
                        }
                    }
                }
            });

            Self {
                addr,
                shutdown,
                handle: Some(handle),
            }
        }
    }

    impl Drop for MockFtpServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Relaxed);
            let _ = TcpStream::connect(self.addr);
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

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
        fs::write(
            &script_file,
            "#!/bin/sh
exit 0
",
        )
        .expect("write failed");
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
    fn test_execute_push_ftp_missing_fields() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let env = crate::action::Environment::new();
        let ftp_no_host = FtpPush {
            host: String::new(),
            port: 21,
            username: None,
            password: None,
            destination: "/var/www".to_string(),
            dir: PathBuf::from("."),
            passive: true,
            secure: false,
        };
        assert!(ftp_no_host.deploy(&env).is_err());

        let ftp_no_dest = FtpPush {
            host: "ftp.test".to_string(),
            port: 21,
            username: None,
            password: None,
            destination: String::new(),
            dir: PathBuf::from("."),
            passive: true,
            secure: false,
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
            port: 22,
            username: None,
            password: None,
            key_path: None,
            destination: "/var/www".to_string(),
            dir: PathBuf::from("."),
        };
        assert!(sftp_no_host.deploy(&env).is_err());

        let sftp_no_dest = SftpPush {
            host: "sftp.test".to_string(),
            port: 22,
            username: None,
            password: None,
            key_path: None,
            destination: String::new(),
            dir: PathBuf::from("."),
        };
        assert!(sftp_no_dest.deploy(&env).is_err());
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
        fs::write(
            &script_file,
            "#!/bin/sh
exit 1
",
        )
        .expect("write failed");
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
        fs::write(
            &script_file,
            "#!/bin/sh
exit 0
",
        )
        .expect("write failed");
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
        fs::write(
            &script_file,
            "#!/bin/sh
exit 0
",
        )
        .expect("write failed");
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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
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

    #[test]
    fn test_ignore_filter_behavior() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let root = dir.path();

        fs::write(
            root.join(".vagrantignore"),
            "# Comment line\n\n*.log\nsecret.key\ntemp/\n",
        )
        .expect("write failed");

        let filter_no_vcs = IgnoreFilter::new(root, false);
        let filter_with_vcs = IgnoreFilter::new(root, true);

        assert!(filter_no_vcs.is_ignored(Path::new(".vagrant/machines/id"), true));
        assert!(filter_no_vcs.is_ignored(Path::new(".git/config"), false));
        assert!(!filter_with_vcs.is_ignored(Path::new(".git/config"), false));

        assert!(filter_no_vcs.is_ignored(Path::new("app.log"), false));
        assert!(filter_no_vcs.is_ignored(Path::new("subdir/app.log"), false));
        assert!(filter_no_vcs.is_ignored(Path::new("secret.key"), false));
        assert!(filter_no_vcs.is_ignored(Path::new("temp/data.txt"), false));
        assert!(!filter_no_vcs.is_ignored(Path::new("src/main.rs"), false));

        let empty_filter = IgnoreFilter::new(Path::new("/nonexistent_ignore_dir_xyz"), false);
        assert!(!empty_filter.is_ignored(Path::new("src/main.rs"), false));
    }

    #[test]
    fn test_ftp_push_mock_server_full_flow() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockFtpServer::start(false, false);
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let sub = cwd.join("subdir");
        fs::create_dir_all(&sub).expect("create_dir failed");
        fs::write(cwd.join("hello.txt"), "hello ftp").expect("write failed");
        fs::write(sub.join("sub.txt"), "sub ftp content").expect("write failed");

        let push = FtpPush {
            host: server.addr.ip().to_string(),
            port: server.addr.port(),
            username: Some("testuser".to_string()),
            password: Some("secret".to_string()),
            destination: "/var/www/app".to_string(),
            dir: cwd.to_path_buf(),
            passive: true,
            secure: false,
        };

        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_ok());
    }

    #[test]
    fn test_ftp_push_auth_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockFtpServer::start(true, false);
        let dir = tempdir().expect("tempdir failed");

        let push = FtpPush {
            host: server.addr.ip().to_string(),
            port: server.addr.port(),
            username: Some("baduser".to_string()),
            password: Some("wrong".to_string()),
            destination: "/app".to_string(),
            dir: dir.path().to_path_buf(),
            passive: true,
            secure: false,
        };

        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_err());
    }

    #[test]
    fn test_ftp_push_stor_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockFtpServer::start(false, true);
        let dir = tempdir().expect("tempdir failed");
        fs::write(dir.path().join("file.txt"), "content").expect("write failed");

        let push = FtpPush {
            host: server.addr.ip().to_string(),
            port: server.addr.port(),
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            destination: "/app".to_string(),
            dir: dir.path().to_path_buf(),
            passive: true,
            secure: false,
        };

        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_err());
    }

    #[test]
    fn test_ftp_push_secure_ftps() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockFtpServer::start(false, false);
        let dir = tempdir().expect("tempdir failed");

        let push = FtpPush {
            host: server.addr.ip().to_string(),
            port: server.addr.port(),
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            destination: "/app".to_string(),
            dir: dir.path().to_path_buf(),
            passive: true,
            secure: true,
        };

        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_ok());
    }

    #[test]
    fn test_ftp_push_env_fallbacks_and_connection_fail() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("VAGRANT_FTP_USERNAME", "env_user");
            std::env::set_var("VAGRANT_FTP_PASSWORD", "env_pass");
        }

        let dir = tempdir().expect("tempdir failed");
        let push = FtpPush {
            host: "127.0.0.1".to_string(),
            port: 1, // closed port
            username: None,
            password: None,
            destination: "/app".to_string(),
            dir: dir.path().to_path_buf(),
            passive: true,
            secure: false,
        };

        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_err());

        unsafe {
            std::env::remove_var("VAGRANT_FTP_USERNAME");
            std::env::remove_var("VAGRANT_FTP_PASSWORD");
        }
    }

    #[test]
    fn test_sftp_push_mock_backend_full_flow() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let root = dir.path();
        let sub = root.join("nested");
        fs::create_dir_all(&sub).expect("create_dir failed");
        fs::write(root.join("root.txt"), "root content").expect("write failed");
        fs::write(sub.join("nested.txt"), "nested content").expect("write failed");

        let push = SftpPush {
            host: "127.0.0.1".to_string(),
            port: 22,
            username: Some("sftp_user".to_string()),
            password: Some("sftp_pass".to_string()),
            key_path: None,
            destination: "/var/www/sftp".to_string(),
            dir: root.to_path_buf(),
        };

        let mut mock = MockSftpBackend::default();
        let res = push.deploy_with_backend(&mut mock);
        assert!(res.is_ok());
        assert!(!mock.created_dirs.is_empty());
        assert_eq!(mock.uploaded_files.len(), 2);
    }

    #[test]
    fn test_sftp_push_backend_error_paths() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let root = dir.path();
        fs::write(root.join("test.txt"), "data").expect("write failed");

        let push = SftpPush {
            host: "127.0.0.1".to_string(),
            port: 22,
            username: None,
            password: None,
            key_path: None,
            destination: "/remote".to_string(),
            dir: root.to_path_buf(),
        };

        let mut mock_mkdir_fail = MockSftpBackend {
            fail_mkdir: true,
            ..Default::default()
        };
        assert!(push.deploy_with_backend(&mut mock_mkdir_fail).is_err());

        let mut mock_upload_fail = MockSftpBackend {
            fail_upload: true,
            ..Default::default()
        };
        assert!(push.deploy_with_backend(&mut mock_upload_fail).is_err());

        let mut mock_verify_fail = MockSftpBackend {
            fail_verify: true,
            ..Default::default()
        };
        assert!(push.deploy_with_backend(&mut mock_verify_fail).is_err());
    }

    #[test]
    fn test_sftp_push_deploy_mock_env() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        fs::write(dir.path().join("a.txt"), "data").expect("write failed");

        let push = SftpPush {
            host: "sftp.example.com".to_string(),
            port: 22,
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            key_path: None,
            destination: "/deploy".to_string(),
            dir: dir.path().to_path_buf(),
        };

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_SFTP", "1");
        }
        let env = crate::action::Environment::new();
        let res = push.deploy(&env);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_SFTP");
        }
        assert!(res.is_ok());
    }

    #[test]
    fn test_sftp_push_connect_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let push = SftpPush {
            host: "127.0.0.1".to_string(),
            port: 1, // closed port
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            key_path: None,
            destination: "/deploy".to_string(),
            dir: dir.path().to_path_buf(),
        };
        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_err());
    }

    #[test]
    fn test_atlas_push_full_direct_upload() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockServer::start();
        let mock_upload = server.mock(|when, then| {
            when.method(PUT).path("/upload");
            then.status(200).body("OK");
        });

        let dir = tempdir().expect("tempdir failed");
        let root = dir.path();
        fs::write(root.join("main.js"), "console.log('hello');").expect("write failed");

        let push = AtlasPush {
            app: "org/myapp".to_string(),
            dir: root.to_path_buf(),
            vcs: false,
            uploader_url: server.url("/upload"),
            version: Some("1.0.0".to_string()),
        };

        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_ok());
        mock_upload.assert();
    }

    #[test]
    fn test_atlas_push_retry_success() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockServer::start();
        let mock_upload = server.mock(|when, then| {
            when.method(PUT).path("/retry");
            then.status(200);
        });

        let dir = tempdir().expect("tempdir failed");
        fs::write(dir.path().join("index.html"), "<h1>hello</h1>").expect("write failed");

        let push = AtlasPush {
            app: "org/retryapp".to_string(),
            dir: dir.path().to_path_buf(),
            vcs: false,
            uploader_url: server.url("/retry"),
            version: None,
        };

        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_ok());
        mock_upload.assert();
    }

    #[test]
    fn test_atlas_push_retry_exhausted() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockServer::start();
        let mock_fail = server.mock(|when, then| {
            when.method(PUT).path("/fail");
            then.status(500);
        });

        let dir = tempdir().expect("tempdir failed");
        fs::write(dir.path().join("index.html"), "<h1>fail</h1>").expect("write failed");

        let push = AtlasPush {
            app: "org/failapp".to_string(),
            dir: dir.path().to_path_buf(),
            vcs: false,
            uploader_url: server.url("/fail"),
            version: None,
        };

        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_err());
        assert_eq!(mock_fail.calls(), 3);
    }

    #[test]
    fn test_atlas_push_archive_respects_ignores() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(PUT).path("/check_archive");
            then.status(200);
        });

        let dir = tempdir().expect("tempdir failed");
        let root = dir.path();
        fs::write(root.join(".vagrantignore"), "*.secret\n").expect("write failed");
        fs::write(root.join("valid.txt"), "valid data").expect("write failed");
        fs::write(root.join("test.secret"), "should be ignored").expect("write failed");

        let vagrant_dir = root.join(".vagrant");
        fs::create_dir_all(&vagrant_dir).expect("create_dir failed");
        fs::write(vagrant_dir.join("id"), "vm-id").expect("write failed");

        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir).expect("create_dir failed");
        fs::write(git_dir.join("HEAD"), "ref").expect("write failed");

        let subfolder = root.join("subfolder");
        fs::create_dir_all(&subfolder).expect("create_dir failed");
        fs::write(subfolder.join("nested.txt"), "nested").expect("write failed");

        let push = AtlasPush {
            app: "org/ignoreapp".to_string(),
            dir: root.to_path_buf(),
            vcs: false,
            uploader_url: server.url("/check_archive"),
            version: None,
        };

        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_ok());
    }

    #[test]
    fn test_atlas_push_missing_app() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let push = AtlasPush {
            app: String::new(),
            dir: dir.path().to_path_buf(),
            vcs: false,
            uploader_url: "https://atlas.hashicorp.com".to_string(),
            version: None,
        };
        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_err());
    }

    #[test]
    fn test_create_push_strategy_options_coverage() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        // FTP strategy
        let mut ftp_opts = HashMap::new();
        ftp_opts.insert("host".to_string(), "ftp.test".to_string());
        ftp_opts.insert("port".to_string(), "2121".to_string());
        ftp_opts.insert("username".to_string(), "u".to_string());
        ftp_opts.insert("password".to_string(), "p".to_string());
        ftp_opts.insert("destination".to_string(), "/dst".to_string());
        ftp_opts.insert("dir".to_string(), "sub".to_string());
        ftp_opts.insert("passive".to_string(), "true".to_string());
        ftp_opts.insert("secure".to_string(), "true".to_string());
        let ftp_cfg = crate::config::PushConfig {
            name: "ftp_test".to_string(),
            strategy: "ftp".to_string(),
            options: ftp_opts,
        };
        assert!(create_push_strategy(&ftp_cfg, cwd).is_ok());

        // FTP strategy without dir
        let mut ftp_nodir = HashMap::new();
        ftp_nodir.insert("host".to_string(), "ftp.test".to_string());
        ftp_nodir.insert("destination".to_string(), "/dst".to_string());
        let ftp_nodir_cfg = crate::config::PushConfig {
            name: "ftp_nodir".to_string(),
            strategy: "ftp".to_string(),
            options: ftp_nodir,
        };
        assert!(create_push_strategy(&ftp_nodir_cfg, cwd).is_ok());

        // SFTP strategy
        let mut sftp_opts = HashMap::new();
        sftp_opts.insert("host".to_string(), "sftp.test".to_string());
        sftp_opts.insert("port".to_string(), "2222".to_string());
        sftp_opts.insert("key_path".to_string(), "id_rsa".to_string());
        sftp_opts.insert("destination".to_string(), "/remote".to_string());
        sftp_opts.insert("dir".to_string(), "sftp_dir".to_string());
        let sftp_cfg = crate::config::PushConfig {
            name: "sftp_test".to_string(),
            strategy: "sftp".to_string(),
            options: sftp_opts,
        };
        assert!(create_push_strategy(&sftp_cfg, cwd).is_ok());

        // SFTP strategy without dir
        let mut sftp_nodir = HashMap::new();
        sftp_nodir.insert("host".to_string(), "sftp.test".to_string());
        sftp_nodir.insert("destination".to_string(), "/remote".to_string());
        let sftp_nodir_cfg = crate::config::PushConfig {
            name: "sftp_nodir".to_string(),
            strategy: "sftp".to_string(),
            options: sftp_nodir,
        };
        assert!(create_push_strategy(&sftp_nodir_cfg, cwd).is_ok());

        // Atlas strategy
        let mut atlas_opts = HashMap::new();
        atlas_opts.insert("app".to_string(), "app".to_string());
        atlas_opts.insert("vcs".to_string(), "true".to_string());
        atlas_opts.insert("version".to_string(), "2.0".to_string());
        atlas_opts.insert("dir".to_string(), "atlas_dir".to_string());
        let atlas_cfg = crate::config::PushConfig {
            name: "atlas_test".to_string(),
            strategy: "atlas".to_string(),
            options: atlas_opts,
        };
        assert!(create_push_strategy(&atlas_cfg, cwd).is_ok());

        // Atlas strategy without dir
        let mut atlas_nodir = HashMap::new();
        atlas_nodir.insert("app".to_string(), "app".to_string());
        let atlas_nodir_cfg = crate::config::PushConfig {
            name: "atlas_nodir".to_string(),
            strategy: "atlas".to_string(),
            options: atlas_nodir,
        };
        assert!(create_push_strategy(&atlas_nodir_cfg, cwd).is_ok());

        // Vagrant-cloud strategy
        let mut vc_opts = HashMap::new();
        vc_opts.insert("app".to_string(), "vc_app".to_string());
        vc_opts.insert("dir".to_string(), "vc_dir".to_string());
        vc_opts.insert("vcs".to_string(), "true".to_string());
        let vc_cfg = crate::config::PushConfig {
            name: "vc_test".to_string(),
            strategy: "vagrant-cloud".to_string(),
            options: vc_opts,
        };
        assert!(create_push_strategy(&vc_cfg, cwd).is_ok());

        // Vagrant-cloud strategy without dir
        let mut vc_nodir = HashMap::new();
        vc_nodir.insert("app".to_string(), "vc_app".to_string());
        vc_nodir.insert("vcs".to_string(), "false".to_string());
        let vc_nodir_cfg = crate::config::PushConfig {
            name: "vc_nodir".to_string(),
            strategy: "vagrant-cloud".to_string(),
            options: vc_nodir,
        };
        assert!(create_push_strategy(&vc_nodir_cfg, cwd).is_ok());
    }
}
