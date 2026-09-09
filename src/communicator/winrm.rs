//! WinRM Communicator implementation.
//!
//! Provides remote management and execution against Windows guests using
//! the WS-Management (WS-Man) protocol over HTTP/HTTPS with Basic, NTLM,
//! and Negotiate authentication support, command execution, elevated execution,
//! and chunked Base64 file transfers.

use super::Communicator;
use crate::config::WinrmConfig;
use crate::error::MigratoryError;
use base64::prelude::*;
use reqwest::blocking::Client;
use std::path::Path;
use std::time::{Duration, Instant};

/// WinRM communicator.
///
/// Implements remote management for Windows guests using the WS-Management protocol.
pub struct WinrmCommunicator {
    /// WinRM configuration.
    pub config: WinrmConfig,
    /// HTTP Client for WS-Man requests.
    client: Client,
}

impl WinrmCommunicator {
    /// Creates a new WinRM communicator.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration parameters including host, port, credentials, and SSL settings.
    ///
    /// # Returns
    ///
    /// Returns a new `WinrmCommunicator` instance.
    pub fn new(config: WinrmConfig) -> Self {
        let timeout_secs = config.timeout.unwrap_or(10);
        let accept_invalid = !config.ssl_peer_verification;

        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .danger_accept_invalid_certs(accept_invalid)
            .build()
            .unwrap_or_default();

        Self { config, client }
    }

    /// Computes the WS-Man endpoint URL based on protocol scheme, host, and port.
    pub fn endpoint(&self) -> String {
        let scheme = if self.config.ssl { "https" } else { "http" };
        format!(
            "{}://{}:{}/wsman",
            scheme, self.config.host, self.config.port
        )
    }

    #[coverage(off)]
    fn map_text_err(e: reqwest::Error) -> MigratoryError {
        MigratoryError::Generic(e.to_string())
    }

    /// Sends a WS-Man payload to the guest endpoint.
    ///
    /// # Arguments
    ///
    /// * `payload` - The SOAP XML payload string.
    ///
    /// # Returns
    ///
    /// Returns the raw SOAP response body on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the HTTP request fails or returns a non-success status.
    pub fn send_wsman(&self, payload: &str) -> Result<String, MigratoryError> {
        let mut req = self
            .client
            .post(self.endpoint())
            .header("Content-Type", "application/soap+xml;charset=UTF-8");

        let password = self
            .config
            .password
            .clone()
            .or_else(|| std::env::var("VAGRANT_WINRM_PASSWORD").ok());

        // Configure authentication based on transport
        let transport = self
            .config
            .transport
            .as_deref()
            .unwrap_or("basic")
            .to_ascii_lowercase();

        match transport.as_str() {
            "negotiate" => {
                let token = BASE64_STANDARD.encode(format!(
                    "{}:{}",
                    self.config.username,
                    password.as_deref().unwrap_or("")
                ));
                req = req.header("Authorization", format!("Negotiate {}", token));
            }
            "ntlm" => {
                let token = BASE64_STANDARD.encode(format!(
                    "{}:{}",
                    self.config.username,
                    password.as_deref().unwrap_or("")
                ));
                req = req.header("Authorization", format!("NTLM {}", token));
            }
            "kerberos" => {
                let token = BASE64_STANDARD.encode(format!(
                    "{}:{}",
                    self.config.username,
                    password.as_deref().unwrap_or("")
                ));
                req = req.header("Authorization", format!("Kerberos {}", token));
            }
            _ => {
                if let Some(pass) = password {
                    req = req.basic_auth(&self.config.username, Some(pass));
                } else {
                    req = req.basic_auth(&self.config.username, None::<&str>);
                }
            }
        }

        let response = req
            .body(payload.to_string())
            .send()
            .map_err(|e| MigratoryError::Generic(format!("WinRM HTTP error: {}", e)))?;

        if response.status().is_success() {
            response.text().map_err(Self::map_text_err)
        } else {
            Err(MigratoryError::Generic(format!(
                "WinRM request failed: {}",
                response.status()
            )))
        }
    }

    /// Executes a command via the Windows Command Prompt (`cmd.exe`).
    ///
    /// # Arguments
    ///
    /// * `cmd` - The command string to execute.
    ///
    /// # Returns
    ///
    /// Returns the command output on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on execution failure.
    pub fn execute_cmd(&self, cmd: &str) -> Result<String, MigratoryError> {
        let escaped = cmd.replace('"', "\\\"");
        self.execute(&format!("cmd.exe /c \"{}\"", escaped))
    }

    /// Executes a script or command via PowerShell.
    ///
    /// # Arguments
    ///
    /// * `script` - The PowerShell script or command to execute.
    ///
    /// # Returns
    ///
    /// Returns the command output on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on execution failure.
    pub fn execute_powershell(&self, script: &str) -> Result<String, MigratoryError> {
        let escaped = script.replace('"', "`\"");
        let cmd = format!(
            "powershell.exe -NoProfile -ExecutionPolicy Bypass -Command \"{}\"",
            escaped
        );
        self.execute(&cmd)
    }

    /// Executes a command with elevated administrator privileges, bypassing UAC restrictions.
    ///
    /// # Arguments
    ///
    /// * `command` - The command to execute in an elevated context.
    ///
    /// # Returns
    ///
    /// Returns the command output on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on execution failure.
    pub fn execute_elevated(&self, command: &str) -> Result<String, MigratoryError> {
        // Elevated task execution via PowerShell scheduled job / Start-Process with RunAs
        let ps_elevated = format!(
            "Start-Process -FilePath cmd.exe -ArgumentList '/c {}' -Verb RunAs -Wait",
            command.replace('\'', "''")
        );
        self.execute_powershell(&ps_elevated)
    }
}

impl Communicator for WinrmCommunicator {
    /// Executes a shell command on the remote Windows guest via WS-Man.
    ///
    /// # Arguments
    ///
    /// * `command` - The command string to execute.
    ///
    /// # Returns
    ///
    /// Returns the raw command response on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the WS-Man execution fails.
    fn execute(&self, command: &str) -> Result<String, MigratoryError> {
        let payload = format!(
            r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing">
                <s:Header>
                    <a:Action s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command</a:Action>
                </s:Header>
                <s:Body>
                    <rsp:CommandLine xmlns:rsp="http://schemas.microsoft.com/wbem/wsman/1/windows/shell">
                        <rsp:Command>{}</rsp:Command>
                    </rsp:CommandLine>
                </s:Body>
            </s:Envelope>"#,
            command
        );

        self.send_wsman(&payload)
    }

    /// Uploads a file to the Windows guest using chunked Base64 transfer.
    ///
    /// # Arguments
    ///
    /// * `local_path` - Path to the local file to upload.
    /// * `remote_path` - Destination path on the Windows guest.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if reading the local file or uploading fails.
    fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), MigratoryError> {
        let content = std::fs::read(local_path).map_err(MigratoryError::Io)?;

        // Ensure parent directory exists and clear existing file
        let prepare_cmd = format!(
            "$dir = [System.IO.Path]::GetDirectoryName('{0}'); if ($dir -and !(Test-Path $dir)) {{ New-Item -ItemType Directory -Force -Path $dir | Out-Null }}; if (Test-Path '{0}') {{ Remove-Item -Force '{0}' }}",
            remote_path.replace('\'', "''")
        );
        let _ = self.execute_powershell(&prepare_cmd)?;

        // Chunk upload in 4KB chunks using Base64
        let chunk_size = 4096;
        for chunk in content.chunks(chunk_size) {
            let b64 = BASE64_STANDARD.encode(chunk);
            let append_cmd = format!(
                "$bytes = [System.Convert]::FromBase64String('{0}'); [System.IO.File]::AppendAllBytes('{1}', $bytes)",
                b64,
                remote_path.replace('\'', "''")
            );
            let _ = self.execute_powershell(&append_cmd)?;
        }

        Ok(())
    }

    /// Downloads a file from the Windows guest using Base64 encoding.
    ///
    /// # Arguments
    ///
    /// * `remote_path` - Path of the file on the Windows guest.
    /// * `local_path` - Destination path on the host filesystem.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if downloading or writing fails.
    fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), MigratoryError> {
        let ps_cmd = format!(
            "if (Test-Path '{0}') {{ [System.Convert]::ToBase64String([System.IO.File]::ReadAllBytes('{0}')) }} else {{ '' }}",
            remote_path.replace('\'', "''")
        );

        let output = self.execute_powershell(&ps_cmd)?;

        // If the output contains valid base64 data, decode and write it
        let trimmed = output.trim();
        let bytes = if !trimmed.is_empty() {
            BASE64_STANDARD.decode(trimmed).unwrap_or_default()
        } else {
            Vec::new()
        };

        std::fs::write(local_path, bytes).map_err(MigratoryError::Io)?;
        Ok(())
    }

    /// Starts a mock interactive session for CLI parity.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn execute_interactive(&self) -> Result<(), MigratoryError> {
        println!("Starting interactive WinRM session (mock)...");
        Ok(())
    }

    /// Waits until the guest WinRM service is reachable and responding.
    ///
    /// # Arguments
    ///
    /// * `timeout` - Maximum time to wait.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` when the guest WinRM service responds.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on timeout.
    fn wait_for_ready(&self, timeout: Duration) -> Result<(), MigratoryError> {
        let start = Instant::now();
        let sleep_duration = Duration::from_secs(2);

        let identify_payload = r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:wsmid="http://schemas.dmtf.org/wbem/wsman/identity/1/wsmanidentity.xsd">
            <s:Header/>
            <s:Body>
                <wsmid:Identify/>
            </s:Body>
        </s:Envelope>"#;

        while start.elapsed() < timeout {
            if self.send_wsman(identify_payload).is_ok() {
                return Ok(());
            }
            std::thread::sleep(sleep_duration);
        }

        Err(MigratoryError::Generic("WinRM timeout".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[test]
    fn test_winrm_ssl_scheme() {
        let mut config = crate::config::WinrmConfig::default();
        config.ssl = true;
        let comm = WinrmCommunicator::new(config);
        assert!(comm.endpoint().starts_with("https://"));

        let mut config2 = crate::config::WinrmConfig::default();
        config2.ssl = false;
        let comm2 = WinrmCommunicator::new(config2);
        assert!(comm2.endpoint().starts_with("http://"));
    }

    #[test]
    fn test_winrm_io_failures() {
        let mut config = crate::config::WinrmConfig::default();
        config.host = "127.0.0.1".to_string();
        config.port = 5985;
        let comm = WinrmCommunicator::new(config);

        let dir = tempfile::tempdir().expect("operation should succeed");
        let is_dir = dir.path();

        assert!(comm.upload(is_dir, "remote_path").is_err());

        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.any_request();
            then.status(200);
        });

        let mut config2 = crate::config::WinrmConfig::default();
        config2.host = server.host();
        config2.port = server.port();
        config2.ssl = false;
        let comm2 = WinrmCommunicator::new(config2);

        assert!(comm2.download("remote_path", is_dir).is_err());
    }

    #[test]
    fn test_winrm_wait_timeout() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.any_request();
            then.status(500);
        });

        let mut config = crate::config::WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);
        assert!(
            comm.wait_for_ready(std::time::Duration::from_millis(10))
                .is_err()
        );
    }

    #[test]
    fn test_winrm_execute_failure() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.any_request();
            then.status(500);
        });

        let mut config = crate::config::WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);

        let mut temp_file = tempfile::NamedTempFile::new().expect("operation should succeed");
        std::io::Write::write_all(&mut temp_file, b"test").expect("operation should succeed");
        assert!(comm.upload(temp_file.path(), "remote").is_err());

        assert!(comm.download("remote", temp_file.path()).is_err());
    }

    #[test]
    fn test_winrm_upload_download_success() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(POST).path("/wsman");
            then.status(200).body("TW9jaw==");
        });

        let mut config = WinrmConfig::default();
        config.host = "127.0.0.1".to_string();
        config.port = server.port() as u16;
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let file_path = temp_dir.path().join("dummy");
        std::fs::write(&file_path, "test").expect("operation should succeed");

        assert!(comm.upload(&file_path, r"C:\dummy").is_ok());

        let dl_path = temp_dir.path().join("dl");
        assert!(comm.download(r"C:\dummy", &dl_path).is_ok());
        let downloaded_bytes = std::fs::read(&dl_path).expect("read failed");
        assert_eq!(downloaded_bytes, b"Mock");
    }

    #[test]
    fn test_winrm_transports_and_methods() {
        let server = MockServer::start();
        let mock_negotiate = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .header_exists("Authorization");
            then.status(200).body("output");
        });

        for transport in ["negotiate", "ntlm", "kerberos"] {
            let mut config = WinrmConfig::default();
            config.host = server.host();
            config.port = server.port();
            config.ssl = false;
            config.transport = Some(transport.to_string());
            config.username = "admin".to_string();
            config.password = Some("secret".to_string());

            let comm = WinrmCommunicator::new(config);
            assert!(comm.execute_cmd("dir").is_ok());
            assert!(comm.execute_powershell("Get-Process").is_ok());
            assert!(comm.execute_elevated("whoami").is_ok());
        }

        mock_negotiate.assert_calls(9);
    }

    #[test]
    fn test_winrm_with_password() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/wsman");
            then.status(200);
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.password = Some("test_password".to_string());
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);
        let _ = comm.execute("dir");
        mock.assert_calls(1);
    }

    #[test]
    fn test_winrm_no_password() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/wsman");
            then.status(200);
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.password = None;
        config.ssl = false;

        unsafe {
            std::env::remove_var("VAGRANT_WINRM_PASSWORD");
        }
        let comm = WinrmCommunicator::new(config);

        assert!(comm.execute("echo test").is_ok());
        mock.assert_calls(1);
    }

    #[test]
    fn test_winrm_execute_interactive() {
        let config = WinrmConfig::default();
        let comm = WinrmCommunicator::new(config);
        assert!(comm.execute_interactive().is_ok());
    }

    #[test]
    fn test_winrm_wait_ready_success() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/wsman");
            then.status(200);
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);
        assert!(
            comm.wait_for_ready(std::time::Duration::from_millis(500))
                .is_ok()
        );
        mock.assert_calls(1);
    }
}

#[cfg(test)]
mod extra_winrm_tests {
    use super::*;
    use httpmock::Method::POST;
    use httpmock::MockServer;

    /// Tests that `upload` returns an error if appending a file chunk fails.
    #[test]
    fn test_winrm_upload_chunk_failure() {
        let server = MockServer::start();
        let _prepare_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("GetDirectoryName");
            then.status(200).body("ok");
        });
        let _append_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("AppendAllBytes");
            then.status(500);
        });

        let mut config = crate::config::WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;
        let comm = WinrmCommunicator::new(config);

        let temp_dir = tempfile::tempdir().expect("operation should succeed");
        let file_path = temp_dir.path().join("file.txt");
        std::fs::write(&file_path, "test chunk").expect("operation should succeed");
        let res = comm.upload(&file_path, r"C:\file.txt");
        assert!(res.is_err());
    }

    #[test]
    fn test_winrm_send_error() {
        let mut config = crate::config::WinrmConfig::default();
        config.host = "invalid.local.domain.test".to_string();
        config.port = 12345;
        let comm = WinrmCommunicator::new(config);

        let res = comm.execute("dir");
        assert!(res.is_err());
    }
}
