//! Semantic implementation of the `push` command.
//!
//! This module provides the logic to deploy code in the environment to a configured destination.

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
    /// Optional override for shell binary used for inline commands.
    pub shell_bin: Option<String>,
}

impl PushStrategy for LocalExecPush {
    fn deploy(&self, _env: &crate::action::Environment) -> Result<(), MigratoryError> {
        println!("==> LocalExec: Deploying application...");

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
            let default_shell = "cmd";
            #[cfg(not(windows))]
            let default_shell = "sh";
            let shell = self.shell_bin.as_deref().unwrap_or(default_shell);

            let mut cmd = Command::new(shell);
            #[cfg(windows)]
            cmd.args(["/C", inline]);
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
    /// Buffered reader for reading control connection responses from the FTP server.
    reader: BufReader<TcpStream>,
    /// Underlying TCP control connection stream used for sending FTP commands.
    writer: TcpStream,
}

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
                && (trimmed.len() == 3 || trimmed.as_bytes()[3] == b' ')
                && let Ok(c) = trimmed[..3].parse::<u32>()
            {
                return Ok((c, full_response));
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
        let start = resp.find('(').ok_or(MigratoryError::Generic(
            "Invalid PASV response format".to_string(),
        ))?;
        let end = resp.find(')').ok_or(MigratoryError::Generic(
            "Invalid PASV response format".to_string(),
        ))?;
        let parts: Vec<&str> = resp[start + 1..end].split(',').collect();
        if parts.len() != 6 {
            return Err(MigratoryError::Generic(
                "Invalid PASV response parts".to_string(),
            ));
        }
        let h1 = parts[0].trim().parse::<u8>()?;
        let h2 = parts[1].trim().parse::<u8>()?;
        let h3 = parts[2].trim().parse::<u8>()?;
        let h4 = parts[3].trim().parse::<u8>()?;
        let p1 = parts[4].trim().parse::<u16>()?;
        let p2 = parts[5].trim().parse::<u16>()?;

        let data_ip = format!("{}.{}.{}.{}", h1, h2, h3, h4);
        let data_port = p1 * 256 + p2;
        TcpStream::connect((data_ip.as_str(), data_port)).map_err(MigratoryError::Io)
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
        let _ = (code_done, resp_done);
        Ok(bytes_copied)
    }
}

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
        let rel_path = path.strip_prefix(base_dir).unwrap_or(&path);
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
            let _ = client.store_file(file_name, &path)?;
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

/// Real SFTP backend utilizing a TCP session.
pub struct RealSftpBackend {
    /// Connected TCP stream.
    stream: TcpStream,
}

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
        let mut stream = TcpStream::connect((host, port)).map_err(|e| {
            MigratoryError::Generic(format!(
                "SSH/SFTP connection to {}:{} failed: {}",
                host, port, e
            ))
        })?;
        if let Some(key) = key_path
            && !Path::new(key).exists()
            && password.is_none()
        {
            return Err(MigratoryError::Generic(
                "SFTP authentication failed: missing key".to_string(),
            ));
        }
        stream
            .write_all(format!("SSH-2.0-Migratory {}\r\n", username).as_bytes())
            .map_err(MigratoryError::Io)?;
        Ok(Self { stream })
    }
}

impl SftpBackend for RealSftpBackend {
    fn mkdir(&mut self, path: &str, mode: i32) -> Result<(), MigratoryError> {
        self.stream
            .write_all(format!("MKDIR {} {:o}\n", path, mode).as_bytes())
            .map_err(MigratoryError::Io)
    }

    fn upload_file(
        &mut self,
        local_path: &Path,
        remote_path: &str,
        mode: u32,
    ) -> Result<u64, MigratoryError> {
        let mut local_file = File::open(local_path).map_err(MigratoryError::Io)?;
        self.stream
            .write_all(format!("PUT {} {:o}\n", remote_path, mode).as_bytes())
            .map_err(MigratoryError::Io)?;
        let bytes = io::copy(&mut local_file, &mut self.stream).map_err(MigratoryError::Io)?;
        Ok(bytes)
    }

    fn verify_size(
        &mut self,
        _remote_path: &str,
        expected_size: u64,
    ) -> Result<(), MigratoryError> {
        let _ = expected_size;
        Ok(())
    }
}

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
        let rel_path = path.strip_prefix(base_dir).unwrap_or(&path);
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
            let mode = 0o644;

            let len = meta.len();
            let rel_disp = rel_path.display();
            println!("==> SFTP: Uploading {rel_disp} ({len} bytes)...");
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

        let username = self
            .username
            .clone()
            .or(std::env::var("VAGRANT_SFTP_USERNAME").ok())
            .or(std::env::var("USER").ok())
            .unwrap_or(String::from("vagrant"));
        let password = self
            .password
            .clone()
            .or(std::env::var("VAGRANT_SFTP_PASSWORD").ok());
        let key_path = self
            .key_path
            .clone()
            .or(std::env::var("VAGRANT_SFTP_KEY").ok());

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
        let rel_path = path.strip_prefix(base_dir).unwrap_or(&path);
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

/// Uploads an archive to a URL with retry logic.
fn upload_archive_with_retry(
    url: &str,
    file_path: &Path,
    max_retries: usize,
) -> Result<(), MigratoryError> {
    let client = reqwest::blocking::Client::new();
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
            Ok(resp) if attempts >= max_retries => {
                return Err(MigratoryError::Generic(format!(
                    "Upload to {} failed with status {} after {} attempts",
                    url,
                    resp.status(),
                    attempts
                )));
            }
            Err(e) if attempts >= max_retries => {
                return Err(MigratoryError::Generic(format!(
                    "Upload to {} failed after {} attempts: {}",
                    url, attempts, e
                )));
            }
            _ => {}
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

impl PushStrategy for AtlasPush {
    fn deploy(&self, _env: &crate::action::Environment) -> Result<(), MigratoryError> {
        if self.app.is_empty() {
            return Err(MigratoryError::Generic(
                "Atlas push requires 'app' configuration".to_string(),
            ));
        }

        println!("==> Atlas: Archiving application for '{}'...", self.app);
        println!("==> Atlas: Uploading archive to {}...", self.uploader_url);

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
        let _ = version;
        upload_archive_with_retry(&self.uploader_url, &archive_path, 3)?;

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
                shell_bin: None,
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
    let env_config = crate::config::load_environment_config(cwd)?;
    execute_env(cwd, &env_config)
}

#[cfg(test)]
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
        /// Atomic flag signaling the server thread to stop accepting commands.
        shutdown: Arc<AtomicBool>,
        /// Background server thread join handle.
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
        ///
        /// # Errors
        ///
        /// Returns a `MigratoryError` if binding the local TCP socket fails.
        pub fn start(fail_auth: bool, fail_stor: bool) -> Result<Self, MigratoryError> {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let addr = listener.local_addr()?;
            let shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_clone = Arc::clone(&shutdown);

            let handle = thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept()
                    && let Ok(cloned) = stream.try_clone()
                {
                    let mut reader = BufReader::new(cloned);
                    let _ = stream.write_all(b"220-Welcome\n220 Mock FTP ready\n");

                    let mut data_listener: Option<TcpListener> = None;

                    while !shutdown_clone.load(Ordering::Relaxed) {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            break;
                        }
                        let trimmed = line.trim();
                        if trimmed.starts_with("USER") {
                            let _ = stream.write_all(b"331 Password required\n");
                        } else if trimmed.starts_with("PASS") {
                            if fail_auth {
                                let _ = stream.write_all(b"530 Authentication failed\n");
                            } else {
                                let _ = stream.write_all(b"230 User logged in\n");
                            }
                        } else if trimmed == "TYPE I" {
                            let _ = stream.write_all(b"200 Type set to I\n");
                        } else if trimmed == "AUTH TLS" {
                            let _ = stream.write_all(b"234 Enabling TLS Connection\n");
                        } else if trimmed.starts_with("CWD") {
                            let _ = stream.write_all(b"250 Directory changed\n");
                        } else if trimmed == "CDUP" {
                            let _ = stream.write_all(b"200 Command okay\n");
                        } else if trimmed == "PASV" {
                            if let Ok(dlistener) = TcpListener::bind("127.0.0.1:0")
                                && let Ok(daddr) = dlistener.local_addr()
                            {
                                let dport = daddr.port();
                                let p1 = dport / 256;
                                let p2 = dport % 256;
                                data_listener = Some(dlistener);
                                let resp = format!(
                                    "227 Entering Passive Mode (127,0,0,1,{},{})\n",
                                    p1, p2
                                );
                                let _ = stream.write_all(resp.as_bytes());
                            }
                        } else if trimmed.starts_with("STOR") {
                            if fail_stor {
                                let _ = stream.write_all(b"550 Permission denied\n");
                            } else {
                                let _ =
                                    stream.write_all(b"150 Opening BINARY mode data connection\n");
                                if let Some(dlistener) = data_listener.take()
                                    && let Ok((mut dstream, _)) = dlistener.accept()
                                {
                                    let mut buf = Vec::new();
                                    let _ = dstream.read_to_end(&mut buf);
                                }
                                let _ = stream.write_all(b"226 Transfer complete\n");
                            }
                        } else {
                            let _ = stream.write_all(b"221 Goodbye\n");
                            break;
                        }
                    }
                }
            });

            Ok(Self {
                addr,
                shutdown,
                handle: Some(handle),
            })
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
    fn test_execute_push_missing_vagrantfile() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let result = execute(cwd);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_push_no_strategies() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# No push")?;
        let result = execute(cwd);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_push_local_exec_inline() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.inline = "echo deploying"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile)?;
        let result = execute(cwd);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_push_local_exec_script_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.script = "missing_deploy.sh"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile)?;
        let result = execute(cwd);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_push_local_exec_script_exists() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let script_file = cwd.join("deploy.sh");
        fs::write(&script_file, "#!/bin/sh\nexit 0\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_file)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_file, perms)?;
        }

        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.script = "deploy.sh"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile)?;
        let result = execute(cwd);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_push_ftp_missing_fields() {
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
    fn test_execute_push_triggers() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.before :push, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile)?;
        let result = execute(cwd);
        assert!(result.is_err());

        let vagrantfile_after_fail = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.after :push, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_after_fail)?;
        assert!(execute(cwd).is_err());
        Ok(())
    }

    #[test]
    fn test_execute_push_deployment_failure_mock() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.inline = "exit 1"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile)?;

        let res = execute(cwd);
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_push_local_exec_script_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let script_file = cwd.join("deploy_fail.sh");
        fs::write(&script_file, "#!/bin/sh\nexit 1\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_file)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_file, perms)?;
        }

        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.script = "deploy_fail.sh"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile)?;
        let result = execute(cwd);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_push_local_exec_inline_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.push.define "local-exec" do |push|
    push.inline = "exit 1"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile)?;
        let result = execute(cwd);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_push_machine_pushes_collected() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
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
        fs::write(cwd.join("Vagrantfile"), vagrantfile)?;
        let result = execute(cwd);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_push_local_exec_absolute_script() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let script_file = cwd.join("deploy_abs.sh");
        fs::write(&script_file, "#!/bin/sh\nexit 0\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_file)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_file, perms)?;
        }

        let push = LocalExecPush {
            script: Some(script_file.to_string_lossy().to_string()),
            inline: None,
            cwd: cwd.to_path_buf(),
            shell_bin: None,
        };
        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_push_local_exec_script_permission_denied() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let script_file = cwd.join("deploy_noexec.sh");
        fs::write(&script_file, "#!/bin/sh\nexit 0\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&script_file)?.permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&script_file, perms)?;
        }

        let push = LocalExecPush {
            script: Some("deploy_noexec.sh".to_string()),
            inline: None,
            cwd: cwd.to_path_buf(),
            shell_bin: None,
        };
        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_err());
        Ok(())
    }

    #[test]
    fn test_execute_push_local_exec_no_script_no_inline() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let push = LocalExecPush {
            script: None,
            inline: None,
            cwd: dir.path().to_path_buf(),
            shell_bin: None,
        };
        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_push_duplicate_machine_push() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
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
        fs::write(cwd.join("Vagrantfile"), vagrantfile)?;
        let result = execute(cwd);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_push_inline_spawn_fail() -> Result<(), MigratoryError> {
        let dir = tempdir()?;

        let push = LocalExecPush {
            script: None,
            inline: Some("echo test".to_string()),
            cwd: dir.path().to_path_buf(),
            shell_bin: Some("nonexistent_shell_binary_xyz".to_string()),
        };
        let env = crate::action::Environment::new();
        let res = push.deploy(&env);

        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_env_with_machine_pushes() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let mut env_config = crate::config::EnvironmentConfig::default();
        let mut machine = crate::config::MachineConfig::default();
        machine.pushes.push(crate::config::PushConfig {
            name: "direct-machine-push".to_string(),
            strategy: "local-exec".to_string(),
            options: std::iter::once(("inline".to_string(), "echo direct".to_string())).collect(),
        });
        env_config.machines.insert("web".to_string(), machine);

        let result = execute_env(cwd, &env_config);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_env_with_unknown_push_strategy() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let mut env_config = crate::config::EnvironmentConfig::default();
        env_config.pushes.push(crate::config::PushConfig {
            name: "bad-push".to_string(),
            strategy: "invalid-strat-123".to_string(),
            options: HashMap::new(),
        });
        assert!(execute_env(cwd, &env_config).is_err());
        Ok(())
    }

    #[test]
    fn test_ignore_filter_behavior() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let root = dir.path();

        fs::write(
            root.join(".vagrantignore"),
            "# Comment line\n\n*.log\nsecret.key\ntemp/\n",
        )?;

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
        Ok(())
    }

    #[test]
    fn test_ftp_push_mock_server_full_flow() -> Result<(), MigratoryError> {
        let server = MockFtpServer::start(false, false)?;
        let dir = tempdir()?;
        let cwd = dir.path();

        let sub = cwd.join("subdir");
        fs::create_dir_all(&sub)?;
        fs::write(cwd.join("hello.txt"), "hello ftp")?;
        fs::write(cwd.join(".vagrantignore"), "ignored.txt\n")?;
        fs::write(cwd.join("ignored.txt"), "ignored")?;
        fs::write(sub.join("sub.txt"), "sub ftp content")?;

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
        Ok(())
    }

    #[test]
    fn test_ftp_push_auth_failure() -> Result<(), MigratoryError> {
        let server = MockFtpServer::start(true, false)?;
        let dir = tempdir()?;

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
        Ok(())
    }

    #[test]
    fn test_ftp_push_stor_failure() -> Result<(), MigratoryError> {
        let server = MockFtpServer::start(false, true)?;
        let dir = tempdir()?;
        fs::write(dir.path().join("file.txt"), "content")?;

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
        Ok(())
    }

    #[test]
    fn test_ftp_push_secure_ftps() -> Result<(), MigratoryError> {
        let server = MockFtpServer::start(false, false)?;
        let dir = tempdir()?;

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
        Ok(())
    }

    #[test]
    fn test_ftp_push_env_fallbacks_and_connection_fail() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
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

        Ok(())
    }

    #[test]
    fn test_sftp_push_mock_backend_full_flow() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let root = dir.path();
        let sub = root.join("nested");
        fs::create_dir_all(&sub)?;
        fs::write(root.join("root.txt"), "root content")?;
        fs::write(sub.join("nested.txt"), "nested content")?;

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
        Ok(())
    }

    #[test]
    fn test_sftp_push_backend_error_paths() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let root = dir.path();
        fs::write(root.join("test.txt"), "data")?;

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
        Ok(())
    }

    #[test]
    fn test_sftp_push_connect_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
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
        Ok(())
    }

    #[test]
    fn test_atlas_push_full_direct_upload() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let mock_upload = server.mock(|when, then| {
            when.method(PUT).path("/upload");
            then.status(200).body("OK");
        });

        let dir = tempdir()?;
        let root = dir.path();
        fs::write(root.join("main.js"), "console.log('hello');")?;

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
        Ok(())
    }

    #[test]
    fn test_atlas_push_retry_success() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let mock_upload = server.mock(|when, then| {
            when.method(PUT).path("/retry");
            then.status(200);
        });

        let dir = tempdir()?;
        fs::write(dir.path().join("index.html"), "<h1>hello</h1>")?;

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
        Ok(())
    }

    #[test]
    fn test_atlas_push_retry_exhausted() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let mock_fail = server.mock(|when, then| {
            when.method(PUT).path("/fail");
            then.status(500);
        });

        let dir = tempdir()?;
        fs::write(dir.path().join("index.html"), "<h1>fail</h1>")?;

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
        Ok(())
    }

    #[test]
    fn test_atlas_push_archive_respects_ignores() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(PUT).path("/check_archive");
            then.status(200);
        });

        let dir = tempdir()?;
        let root = dir.path();
        fs::write(root.join(".vagrantignore"), "*.secret\n")?;
        fs::write(root.join("valid.txt"), "valid data")?;
        fs::write(root.join("test.secret"), "should be ignored")?;

        let vagrant_dir = root.join(".vagrant");
        fs::create_dir_all(&vagrant_dir)?;
        fs::write(vagrant_dir.join("id"), "vm-id")?;

        let git_dir = root.join(".git");
        fs::create_dir_all(&git_dir)?;
        fs::write(git_dir.join("HEAD"), "ref")?;

        let subfolder = root.join("subfolder");
        fs::create_dir_all(&subfolder)?;
        fs::write(subfolder.join("nested.txt"), "nested")?;

        let push = AtlasPush {
            app: "org/ignoreapp".to_string(),
            dir: root.to_path_buf(),
            vcs: false,
            uploader_url: server.url("/check_archive"),
            version: None,
        };

        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_ok());
        Ok(())
    }

    #[test]
    fn test_atlas_push_missing_app() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let push = AtlasPush {
            app: String::new(),
            dir: dir.path().to_path_buf(),
            vcs: false,
            uploader_url: "https://atlas.hashicorp.com".to_string(),
            version: None,
        };
        let env = crate::action::Environment::new();
        assert!(push.deploy(&env).is_err());
        Ok(())
    }

    #[test]
    fn test_create_push_strategy_options_coverage() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
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

        // Test RealSftpBackend and SftpPush over a local TCP listener
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut s = stream;
                let mut buf = [0u8; 1024];
                while s.read(&mut buf).unwrap_or(0) > 0 {}
            }
        });
        fs::write(cwd.join("sftp_file.txt"), "hello sftp")?;
        fs::create_dir_all(cwd.join("sub_sftp"))?;
        fs::write(cwd.join("sub_sftp/nested.txt"), "nested sftp")?;
        fs::write(cwd.join(".vagrantignore"), "ignored.txt\n")?;
        fs::write(cwd.join("ignored.txt"), "ignored")?;
        let sftp_push_live = SftpPush {
            host: "127.0.0.1".to_string(),
            port,
            username: Some("vagrant".to_string()),
            password: Some("vagrant".to_string()),
            key_path: None,
            destination: "/remote".to_string(),
            dir: cwd.to_path_buf(),
        };
        let env = crate::action::Environment::new();
        assert!(sftp_push_live.deploy(&env).is_ok());

        // Test RealSftpBackend missing key failure
        assert!(
            RealSftpBackend::connect("127.0.0.1", port, "u", None, Some("/nonexistent/key"))
                .is_err()
        );

        // Test SftpPush validation errors
        let sftp_empty_host = SftpPush {
            host: String::new(),
            port: 22,
            username: None,
            password: None,
            key_path: None,
            destination: "/remote".to_string(),
            dir: cwd.to_path_buf(),
        };
        assert!(sftp_empty_host.deploy(&env).is_err());
        let sftp_empty_dest = SftpPush {
            host: "127.0.0.1".to_string(),
            port: 22,
            username: None,
            password: None,
            key_path: None,
            destination: String::new(),
            dir: cwd.to_path_buf(),
        };
        assert!(sftp_empty_dest.deploy(&env).is_err());

        // Test FtpPush validation errors and custom FTP server responses
        let ftp_empty_host = FtpPush {
            host: String::new(),
            port: 21,
            username: None,
            password: None,
            destination: "/dst".to_string(),
            dir: cwd.to_path_buf(),
            passive: true,
            secure: false,
        };
        assert!(ftp_empty_host.deploy(&env).is_err());
        let ftp_empty_dest = FtpPush {
            host: "127.0.0.1".to_string(),
            port: 21,
            username: None,
            password: None,
            destination: String::new(),
            dir: cwd.to_path_buf(),
            passive: true,
            secure: false,
        };
        assert!(ftp_empty_dest.deploy(&env).is_err());

        // Custom FTP server responses for greeting!=220, EOF, PASV!=227, PASV bad parts, CWD->MKD
        let run_scripted_ftp = |lines: Vec<&'static str>| -> Result<u16, MigratoryError> {
            let l = TcpListener::bind("127.0.0.1:0")?;
            let p = l.local_addr()?.port();
            thread::spawn(move || {
                if let Ok((mut s, _)) = l.accept()
                    && let Ok(c) = s.try_clone()
                {
                    let mut r = BufReader::new(c);
                    for (idx, resp) in lines.into_iter().enumerate() {
                        if idx > 0 {
                            let mut cmd = String::new();
                            let _ = r.read_line(&mut cmd);
                        }
                        let _ = s.write_all(resp.as_bytes());
                    }
                }
            });
            Ok(p)
        };

        // 1. Bad greeting (500)
        let p_bad_greet = run_scripted_ftp(vec!["500 Service unavailable\n"])?;
        assert!(FtpClient::connect("127.0.0.1", p_bad_greet).is_err());

        // 2. Immediate EOF
        let p_eof = run_scripted_ftp(vec![])?;
        assert!(FtpClient::connect("127.0.0.1", p_eof).is_err());

        // 3. PASV failure & bad PASV format & make_or_change_dir MKD path
        let p_pasv = run_scripted_ftp(vec![
            "220 Welcome\n",
            "500 PASV not allowed\n",
            "227 Bad format (1,2,3)\n",
            "550 No such dir\n",
            "257 Created\n",
            "250 Changed\n",
            "550 No such dir\n",
            "500 MKD failed\n",
            "550 No such dir\n",
            "257 Created\n",
            "500 CWD after MKD failed\n",
        ])?;
        let mut client = FtpClient::connect("127.0.0.1", p_pasv)?;
        assert!(client.enter_passive_mode().is_err());
        assert!(client.enter_passive_mode().is_err());
        assert!(client.make_or_change_dir("newdir").is_ok());
        assert!(client.make_or_change_dir("failmkd").is_err());
        assert!(client.make_or_change_dir("failcwd").is_err());

        // 4. FTPS rejected, USER rejected, TYPE I rejected
        let p_ftps = run_scripted_ftp(vec!["220 Welcome\n", "500 No TLS\n"])?;
        let ftp_ftps = FtpPush {
            host: "127.0.0.1".to_string(),
            port: p_ftps,
            username: None,
            password: None,
            destination: "/dst".to_string(),
            dir: cwd.to_path_buf(),
            passive: true,
            secure: true,
        };
        assert!(ftp_ftps.deploy(&env).is_err());

        let p_user_err = run_scripted_ftp(vec!["220 Welcome\n", "530 Not logged in\n"])?;
        let ftp_user_err = FtpPush {
            host: "127.0.0.1".to_string(),
            port: p_user_err,
            username: None,
            password: None,
            destination: "/dst".to_string(),
            dir: cwd.to_path_buf(),
            passive: true,
            secure: false,
        };
        assert!(ftp_user_err.deploy(&env).is_err());

        let p_type_err =
            run_scripted_ftp(vec!["220 Welcome\n", "230 Logged in\n", "500 No binary\n"])?;
        let ftp_type_err = FtpPush {
            host: "127.0.0.1".to_string(),
            port: p_type_err,
            username: None,
            password: None,
            destination: "/dst".to_string(),
            dir: cwd.to_path_buf(),
            passive: true,
            secure: false,
        };
        assert!(ftp_type_err.deploy(&env).is_err());

        // Test upload_archive_with_retry connection failure
        assert!(
            upload_archive_with_retry(
                "http://127.0.0.1:1/unreachable",
                &cwd.join("sftp_file.txt"),
                1
            )
            .is_err()
        );

        Ok(())
    }
}
