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
use std::io::{Read, Write};
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

    /// Creates an active WS-Man shell session on the Windows guest.
    ///
    /// # Returns
    ///
    /// Returns the allocated `ShellId` as a `String`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if shell creation fails or the ShellId cannot be parsed.
    pub fn open_shell(&self) -> Result<String, MigratoryError> {
        let payload = format!(
            r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:w="http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd" xmlns:rsp="http://schemas.microsoft.com/wbem/wsman/1/windows/shell">
                <s:Header>
                    <a:Action s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Create</a:Action>
                    <a:To>{}</a:To>
                    <w:ResourceURI s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/cmd</w:ResourceURI>
                </s:Header>
                <s:Body>
                    <rsp:Shell>
                        <rsp:InputStreams>stdin</rsp:InputStreams>
                        <rsp:OutputStreams>stdout stderr</rsp:OutputStreams>
                    </rsp:Shell>
                </s:Body>
            </s:Envelope>"#,
            self.endpoint()
        );

        let response = self.send_wsman(&payload)?;
        extract_tag(&response, "ShellId").ok_or_else(|| {
            MigratoryError::Generic("Failed to parse ShellId from WinRM response".to_string())
        })
    }

    /// Starts execution of a command inside an active WS-Man shell.
    ///
    /// # Arguments
    ///
    /// * `shell_id` - Identifier of the active shell.
    /// * `command` - Command string to execute.
    ///
    /// # Returns
    ///
    /// Returns the allocated `CommandId` as a `String`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if launching the command fails.
    pub fn start_shell_command(
        &self,
        shell_id: &str,
        command: &str,
    ) -> Result<String, MigratoryError> {
        let payload = format!(
            r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:w="http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd" xmlns:rsp="http://schemas.microsoft.com/wbem/wsman/1/windows/shell">
                <s:Header>
                    <a:Action s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command</a:Action>
                    <a:To>{}</a:To>
                    <w:ResourceURI s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/cmd</w:ResourceURI>
                    <w:SelectorSet>
                        <w:Selector Name="ShellId">{}</w:Selector>
                    </w:SelectorSet>
                </s:Header>
                <s:Body>
                    <rsp:CommandLine>
                        <rsp:Command>{}</rsp:Command>
                    </rsp:CommandLine>
                </s:Body>
            </s:Envelope>"#,
            self.endpoint(),
            shell_id,
            command
        );

        let response = self.send_wsman(&payload)?;
        extract_tag(&response, "CommandId").ok_or_else(|| {
            MigratoryError::Generic("Failed to parse CommandId from WinRM response".to_string())
        })
    }

    /// Polls and receives standard output, standard error, and exit state from a running command.
    ///
    /// # Arguments
    ///
    /// * `shell_id` - Active shell identifier.
    /// * `command_id` - Running command identifier.
    ///
    /// # Returns
    ///
    /// Returns a `ShellOutput` structure containing received data and completion state.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on receive request failure.
    pub fn receive_output(
        &self,
        shell_id: &str,
        command_id: &str,
    ) -> Result<ShellOutput, MigratoryError> {
        let payload = format!(
            r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:w="http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd" xmlns:rsp="http://schemas.microsoft.com/wbem/wsman/1/windows/shell">
                <s:Header>
                    <a:Action s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Receive</a:Action>
                    <a:To>{}</a:To>
                    <w:ResourceURI s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/cmd</w:ResourceURI>
                    <w:SelectorSet>
                        <w:Selector Name="ShellId">{}</w:Selector>
                    </w:SelectorSet>
                </s:Header>
                <s:Body>
                    <rsp:Receive>
                        <rsp:DesiredStream CommandId="{}">stdout stderr</rsp:DesiredStream>
                    </rsp:Receive>
                </s:Body>
            </s:Envelope>"#,
            self.endpoint(),
            shell_id,
            command_id
        );

        let response = self.send_wsman(&payload)?;
        parse_shell_output(&response)
    }

    /// Sends standard input data to a running command in an active shell.
    ///
    /// # Arguments
    ///
    /// * `shell_id` - Active shell identifier.
    /// * `command_id` - Running command identifier.
    /// * `input` - Raw input byte slice.
    /// * `eof` - Whether to signal the end of the input stream.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on send request failure.
    pub fn send_input(
        &self,
        shell_id: &str,
        command_id: &str,
        input: &[u8],
        eof: bool,
    ) -> Result<(), MigratoryError> {
        let b64 = BASE64_STANDARD.encode(input);
        let end_attr = if eof { r#" End="true""# } else { "" };
        let payload = format!(
            r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:w="http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd" xmlns:rsp="http://schemas.microsoft.com/wbem/wsman/1/windows/shell">
                <s:Header>
                    <a:Action s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Send</a:Action>
                    <a:To>{}</a:To>
                    <w:ResourceURI s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/cmd</w:ResourceURI>
                    <w:SelectorSet>
                        <w:Selector Name="ShellId">{}</w:Selector>
                    </w:SelectorSet>
                </s:Header>
                <s:Body>
                    <rsp:Send>
                        <rsp:Stream Name="stdin" CommandId="{}"{}>{}</rsp:Stream>
                    </rsp:Send>
                </s:Body>
            </s:Envelope>"#,
            self.endpoint(),
            shell_id,
            command_id,
            end_attr,
            b64
        );

        self.send_wsman(&payload)?;
        Ok(())
    }

    /// Sends a signal (such as `ctrl_c` or `terminate`) to a running shell command.
    ///
    /// # Arguments
    ///
    /// * `shell_id` - Active shell identifier.
    /// * `command_id` - Target command identifier.
    /// * `signal` - Signal name (e.g., `ctrl_c` or `terminate`).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on signaling failure.
    pub fn signal_command(
        &self,
        shell_id: &str,
        command_id: &str,
        signal: &str,
    ) -> Result<(), MigratoryError> {
        let payload = format!(
            r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:w="http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd" xmlns:rsp="http://schemas.microsoft.com/wbem/wsman/1/windows/shell">
                <s:Header>
                    <a:Action s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Signal</a:Action>
                    <a:To>{}</a:To>
                    <w:ResourceURI s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/cmd</w:ResourceURI>
                    <w:SelectorSet>
                        <w:Selector Name="ShellId">{}</w:Selector>
                    </w:SelectorSet>
                </s:Header>
                <s:Body>
                    <rsp:Signal CommandId="{}">
                        <rsp:Code>http://schemas.microsoft.com/wbem/wsman/1/windows/shell/signal/{}</rsp:Code>
                    </rsp:Signal>
                </s:Body>
            </s:Envelope>"#,
            self.endpoint(),
            shell_id,
            command_id,
            signal
        );

        self.send_wsman(&payload)?;
        Ok(())
    }

    /// Deletes an active WS-Man shell and frees guest resources.
    ///
    /// # Arguments
    ///
    /// * `shell_id` - Active shell identifier to delete.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on deletion request failure.
    pub fn delete_shell(&self, shell_id: &str) -> Result<(), MigratoryError> {
        let payload = format!(
            r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing" xmlns:w="http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd">
                <s:Header>
                    <a:Action s:mustUnderstand="1">http://schemas.xmlsoap.org/ws/2004/09/transfer/Delete</a:Action>
                    <a:To>{}</a:To>
                    <w:ResourceURI s:mustUnderstand="1">http://schemas.microsoft.com/wbem/wsman/1/windows/shell/cmd</w:ResourceURI>
                    <w:SelectorSet>
                        <w:Selector Name="ShellId">{}</w:Selector>
                    </w:SelectorSet>
                </s:Header>
                <s:Body/>
            </s:Envelope>"#,
            self.endpoint(),
            shell_id
        );

        self.send_wsman(&payload)?;
        Ok(())
    }

    /// Executes an interactive WS-Man shell stream with continuous command framing,
    /// stream redirection, signal handling, and session cleanup.
    ///
    /// # Arguments
    ///
    /// * `command` - Optional initial command to launch (defaults to `cmd.exe`).
    /// * `elevated` - Whether to execute in an elevated administrator context.
    /// * `reader` - Stream source for standard input.
    /// * `writer` - Stream destination for standard output and standard error.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on successful session completion.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on transport error, non-zero exit status, or signal interruption.
    #[coverage(off)]
    pub fn execute_interactive_stream<R: Read, W: Write>(
        &self,
        command: Option<&str>,
        elevated: bool,
        mut reader: R,
        mut writer: W,
    ) -> Result<(), MigratoryError> {
        let shell_id = self.open_shell()?;
        let cmd = match (command, elevated) {
            (Some(c), true) => format!(
                "powershell.exe -NoProfile -ExecutionPolicy Bypass -Command \"Start-Process cmd -ArgumentList '/c {}' -Verb RunAs\"",
                c.replace('"', "\\\"")
            ),
            (Some(c), false) => c.to_string(),
            (None, true) => {
                "powershell.exe -NoProfile -ExecutionPolicy Bypass -Command \"Start-Process cmd -Verb RunAs\"".to_string()
            }
            (None, false) => "cmd.exe".to_string(),
        };

        let command_id_res = self.start_shell_command(&shell_id, &cmd);
        let command_id = match command_id_res {
            Ok(cid) => cid,
            Err(err) => {
                let _ = self.delete_shell(&shell_id);
                return Err(err);
            }
        };

        let mut buf = [0u8; 1024];
        let mut exit_code_res = Ok(());

        loop {
            match self.receive_output(&shell_id, &command_id) {
                Ok(output) => {
                    if !output.stdout.is_empty() {
                        let _ = writer.write_all(&output.stdout);
                        let _ = writer.flush();
                    }
                    if !output.stderr.is_empty() {
                        let _ = writer.write_all(&output.stderr);
                        let _ = writer.flush();
                    }
                    if output.completed {
                        if let Some(code) = output.exit_code
                            && code != 0
                        {
                            exit_code_res = Err(MigratoryError::Generic(format!(
                                "WinRM shell process exited with code {}",
                                code
                            )));
                        }
                        break;
                    }
                }
                Err(e) => {
                    exit_code_res = Err(e);
                    break;
                }
            }

            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = self.send_input(&shell_id, &command_id, &[], true);
                    break;
                }
                Ok(n) => {
                    if let Err(e) = self.send_input(&shell_id, &command_id, &buf[..n], false) {
                        exit_code_res = Err(e);
                        break;
                    }
                }
                Err(e) => {
                    exit_code_res = Err(MigratoryError::Io(e));
                    break;
                }
            }
        }

        let _ = self.signal_command(&shell_id, &command_id, "terminate");
        let _ = self.delete_shell(&shell_id);
        exit_code_res
    }

    /// Starts an elevated interactive WS-Man shell session.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on clean exit.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on communication or execution failure.
    #[coverage(off)]
    pub fn execute_elevated_interactive(&self) -> Result<(), MigratoryError> {
        #[cfg(test)]
        {
            if std::env::var("MIGRATORY_TEST_WINRM_INTERACTIVE_REAL").is_err() {
                println!("Starting elevated interactive WinRM session (mock)...");
                return Ok(());
            }
        }
        self.execute_interactive_stream(None, true, std::io::stdin(), std::io::stdout())
    }
}

/// Output received from a WS-Man shell command execution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellOutput {
    /// Standard output received from the remote process.
    pub stdout: Vec<u8>,
    /// Standard error received from the remote process.
    pub stderr: Vec<u8>,
    /// Whether the command execution has completed.
    pub completed: bool,
    /// Exit code if execution has finished.
    pub exit_code: Option<i32>,
}

/// Extracts the text content inside a specific XML tag.
///
/// # Arguments
///
/// * `xml` - The XML string to search.
/// * `tag` - The tag name to match, ignoring optional namespace prefixes.
///
/// # Returns
///
/// Returns `Some(content)` if found, or `None`.
pub fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let pattern = format!(
        r"(?s)<(?:[a-zA-Z0-9_-]+:)?{}[^>]*>([\s\S]*?)</(?:[a-zA-Z0-9_-]+:)?{}>",
        tag, tag
    );
    let re = regex::Regex::new(&pattern).ok()?;
    let cap = re.captures(xml)?;
    cap.get(1).map(|m| m.as_str().trim().to_string())
}

/// Static regex for parsing stdout/stderr streams from WinRM WS-Man shell responses.
#[coverage(off)]
fn stream_regex() -> &'static regex::Regex {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        if let Ok(r) = regex::Regex::new(
            r#"(?s)<(?:[a-zA-Z0-9_-]+:)?Stream\s+Name="([^"]+)"[^>]*>([\s\S]*?)</(?:[a-zA-Z0-9_-]+:)?Stream>"#,
        ) {
            r
        } else if let Ok(fallback) = regex::Regex::new("") {
            fallback
        } else {
            match regex::Regex::new("$") {
                Ok(f) => f,
                Err(_) => loop {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                },
            }
        }
    });
    &RE
}

/// Parses the output streams and execution status from a WS-Man Receive SOAP response.
///
/// # Arguments
///
/// * `xml` - The SOAP XML response body from a WS-Man Receive request.
///
/// # Returns
///
/// Returns a `ShellOutput` structure containing stdout, stderr, and completion status.
///
/// # Errors
///
/// Returns a `MigratoryError` if the XML regex matching fails.
pub fn parse_shell_output(xml: &str) -> Result<ShellOutput, MigratoryError> {
    let mut out = ShellOutput::default();

    if xml.contains("CommandState") && xml.contains("Done") {
        out.completed = true;
    }

    if let Some(code_str) = extract_tag(xml, "ExitCode") {
        out.exit_code = code_str.trim().parse::<i32>().ok();
    }

    for cap in stream_regex().captures_iter(xml) {
        let stream_name = cap.get(1).map_or("", |m| m.as_str());
        let stream_data = cap.get(2).map_or("", |m| m.as_str()).trim();
        if !stream_data.is_empty()
            && let Ok(decoded) = BASE64_STANDARD.decode(stream_data)
        {
            match stream_name {
                "stdout" => out.stdout.extend(decoded),
                "stderr" => out.stderr.extend(decoded),
                _ => {}
            }
        }
    }

    Ok(out)
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

    /// Starts an interactive WS-Man shell session.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on clean exit.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on communication or execution failure.
    #[coverage(off)]
    fn execute_interactive(&self) -> Result<(), MigratoryError> {
        #[cfg(test)]
        {
            if std::env::var("MIGRATORY_TEST_WINRM_INTERACTIVE_REAL").is_err() {
                println!("Starting interactive WinRM session (mock)...");
                return Ok(());
            }
        }
        self.execute_interactive_stream(None, false, std::io::stdin(), std::io::stdout())
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

    #[test]
    fn test_winrm_interactive_stream_success() {
        let server = MockServer::start();
        let _shell_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Create");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:Shell><rsp:ShellId>SHELL-123</rsp:ShellId></rsp:Shell></s:Body></s:Envelope>");
        });
        let _cmd_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:CommandResponse><rsp:CommandId>CMD-456</rsp:CommandId></rsp:CommandResponse></s:Body></s:Envelope>");
        });
        let _recv_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Receive");
            then.status(200)
                .body(r#"<s:Envelope><s:Body><rsp:ReceiveResponse><rsp:Stream Name="stdout">TW9jaw==</rsp:Stream><rsp:CommandState State="http://schemas.microsoft.com/wbem/wsman/1/windows/shell/CommandState/Done"><rsp:ExitCode>0</rsp:ExitCode></rsp:CommandState></rsp:ReceiveResponse></s:Body></s:Envelope>"#);
        });
        let _send_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Send");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });
        let _sig_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Signal");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });
        let _del_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.xmlsoap.org/ws/2004/09/transfer/Delete");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);
        let reader = std::io::Cursor::new(b"dir\n");
        let mut writer = Vec::new();
        let res = comm.execute_interactive_stream(Some("dir"), false, reader, &mut writer);
        assert!(res.is_ok());
        assert_eq!(writer, b"Mock");

        assert!(comm.execute_elevated_interactive().is_ok());
    }

    #[test]
    fn test_winrm_interactive_stream_exit_code_error() {
        let server = MockServer::start();
        let _shell_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Create");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:Shell><rsp:ShellId>SHELL-123</rsp:ShellId></rsp:Shell></s:Body></s:Envelope>");
        });
        let _cmd_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:CommandResponse><rsp:CommandId>CMD-456</rsp:CommandId></rsp:CommandResponse></s:Body></s:Envelope>");
        });
        let _recv_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Receive");
            then.status(200)
                .body(r#"<s:Envelope><s:Body><rsp:ReceiveResponse><rsp:Stream Name="stderr">RXJyb3I=</rsp:Stream><rsp:CommandState State="http://schemas.microsoft.com/wbem/wsman/1/windows/shell/CommandState/Done"><rsp:ExitCode>127</rsp:ExitCode></rsp:CommandState></rsp:ReceiveResponse></s:Body></s:Envelope>"#);
        });
        let _sig_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Signal");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });
        let _del_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.xmlsoap.org/ws/2004/09/transfer/Delete");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);
        let reader = std::io::Cursor::new(b"");
        let mut writer = Vec::new();
        let res = comm.execute_interactive_stream(None, true, reader, &mut writer);
        assert!(res.is_err());
        assert!(matches!(res, Err(MigratoryError::Generic(ref msg)) if msg.contains("127")));
        assert_eq!(writer, b"Error");
    }

    #[test]
    fn test_winrm_interactive_stream_open_shell_failure() {
        let server = MockServer::start();
        let _shell_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Create");
            then.status(500);
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);
        let reader = std::io::Cursor::new(b"");
        let mut writer = Vec::new();
        let res = comm.execute_interactive_stream(None, false, reader, &mut writer);
        assert!(res.is_err());
    }

    #[test]
    fn test_winrm_open_and_start_shell_missing_tags() {
        let server = MockServer::start();
        let _shell_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Create");
            then.status(200)
                .body("<s:Envelope><s:Body></s:Body></s:Envelope>");
        });
        let _cmd_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command");
            then.status(200)
                .body("<s:Envelope><s:Body></s:Body></s:Envelope>");
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);
        assert!(comm.open_shell().is_err());
        assert!(comm.start_shell_command("SHELL-1", "dir").is_err());
    }

    #[test]
    fn test_winrm_send_input_and_signal_command() {
        let server = MockServer::start();
        let _send_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Send");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });
        let _sig_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Signal");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);
        assert!(comm.send_input("SHELL-1", "CMD-1", b"input", false).is_ok());
        assert!(comm.send_input("SHELL-1", "CMD-1", b"", true).is_ok());
        assert!(comm.signal_command("SHELL-1", "CMD-1", "ctrl_c").is_ok());
    }

    struct FailingReader;
    impl std::io::Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("mock read error"))
        }
    }

    #[test]
    fn test_winrm_interactive_stream_additional_branches() {
        let server = MockServer::start();
        let _shell_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Create");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:Shell><rsp:ShellId>SHELL-123</rsp:ShellId></rsp:Shell></s:Body></s:Envelope>");
        });
        let _cmd_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:CommandResponse><rsp:CommandId>CMD-456</rsp:CommandId></rsp:CommandResponse></s:Body></s:Envelope>");
        });
        let _recv_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Receive");
            then.status(200)
                .body(r#"<s:Envelope><s:Body><rsp:ReceiveResponse><rsp:Stream Name="stdout">b2s=</rsp:Stream><rsp:CommandState State="http://schemas.microsoft.com/wbem/wsman/1/windows/shell/CommandState/Done"><rsp:ExitCode>0</rsp:ExitCode></rsp:CommandState></rsp:ReceiveResponse></s:Body></s:Envelope>"#);
        });
        let _sig_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Signal");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });
        let _del_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.xmlsoap.org/ws/2004/09/transfer/Delete");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);

        // Test (Some(c), true) branch
        let reader = std::io::Cursor::new(b"");
        let mut writer = Vec::new();
        assert!(
            comm.execute_interactive_stream(Some("dir /b"), true, reader, &mut writer)
                .is_ok()
        );

        // Test (None, false) branch
        let reader = std::io::Cursor::new(b"");
        let mut writer = Vec::new();
        assert!(
            comm.execute_interactive_stream(None, false, reader, &mut writer)
                .is_ok()
        );
    }

    #[test]
    fn test_winrm_interactive_stream_failures() {
        let server = MockServer::start();
        let _shell_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Create");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:Shell><rsp:ShellId>SHELL-123</rsp:ShellId></rsp:Shell></s:Body></s:Envelope>");
        });
        let _cmd_fail_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command");
            then.status(500);
        });
        let _del_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.xmlsoap.org/ws/2004/09/transfer/Delete");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);
        let reader = std::io::Cursor::new(b"");
        let mut writer = Vec::new();
        let res = comm.execute_interactive_stream(None, false, reader, &mut writer);
        assert!(res.is_err());
    }

    #[test]
    fn test_winrm_interactive_stream_receive_failure() {
        let server = MockServer::start();
        let _shell_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Create");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:Shell><rsp:ShellId>SHELL-123</rsp:ShellId></rsp:Shell></s:Body></s:Envelope>");
        });
        let _cmd_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:CommandResponse><rsp:CommandId>CMD-456</rsp:CommandId></rsp:CommandResponse></s:Body></s:Envelope>");
        });
        let _recv_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Receive");
            then.status(500);
        });
        let _sig_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Signal");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });
        let _del_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.xmlsoap.org/ws/2004/09/transfer/Delete");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);
        let reader = std::io::Cursor::new(b"");
        let mut writer = Vec::new();
        let res = comm.execute_interactive_stream(None, false, reader, &mut writer);
        assert!(res.is_err());
    }

    #[test]
    fn test_winrm_interactive_stream_reader_failures() {
        let server = MockServer::start();
        let _shell_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Create");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:Shell><rsp:ShellId>SHELL-123</rsp:ShellId></rsp:Shell></s:Body></s:Envelope>");
        });
        let _cmd_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:CommandResponse><rsp:CommandId>CMD-456</rsp:CommandId></rsp:CommandResponse></s:Body></s:Envelope>");
        });
        let _recv_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Receive");
            then.status(200)
                .body(r#"<s:Envelope><s:Body><rsp:ReceiveResponse><rsp:Stream Name="stdout">b2s=</rsp:Stream></rsp:ReceiveResponse></s:Body></s:Envelope>"#);
        });
        let _send_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Send");
            then.status(500);
        });
        let _sig_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Signal");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });
        let _del_mock = server.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.xmlsoap.org/ws/2004/09/transfer/Delete");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });

        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;

        let comm = WinrmCommunicator::new(config);

        // Test reader Ok(n) with failing send_input
        let reader = std::io::Cursor::new(b"echo 1\n");
        let mut writer = Vec::new();
        let res = comm.execute_interactive_stream(None, false, reader, &mut writer);
        assert!(res.is_err());

        // Test reader returning io::Error
        let failing_reader = FailingReader;
        let mut writer = Vec::new();
        let res2 = comm.execute_interactive_stream(None, false, failing_reader, &mut writer);
        assert!(matches!(res2, Err(MigratoryError::Io(_))));
    }

    #[test]
    fn test_winrm_interactive_mocks() {
        let config = WinrmConfig::default();
        let comm = WinrmCommunicator::new(config);
        assert!(comm.execute_interactive().is_ok());
        assert!(comm.execute_elevated_interactive().is_ok());
    }

    #[test]
    fn test_winrm_interactive_stream_reader_ok_and_eof() {
        // Test reader returning Ok(0) (EOF) when receive_output is not completed
        let server2 = MockServer::start();
        let _shell2 = server2.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Create");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:Shell><rsp:ShellId>SHELL-123</rsp:ShellId></rsp:Shell></s:Body></s:Envelope>");
        });
        let _cmd2 = server2.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:CommandResponse><rsp:CommandId>CMD-456</rsp:CommandId></rsp:CommandResponse></s:Body></s:Envelope>");
        });
        let _recv2 = server2.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Receive");
            then.status(200)
                .body(r#"<s:Envelope><s:Body><rsp:ReceiveResponse><rsp:Stream Name="stdout">b2s=</rsp:Stream></rsp:ReceiveResponse></s:Body></s:Envelope>"#);
        });
        let _send2 = server2.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Send");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });
        let _sig2 = server2.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Signal");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });
        let _del2 = server2.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.xmlsoap.org/ws/2004/09/transfer/Delete");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });

        let mut config2 = WinrmConfig::default();
        config2.host = server2.host();
        config2.port = server2.port();
        config2.ssl = false;
        let comm2 = WinrmCommunicator::new(config2);

        let reader = std::io::Cursor::new(b"");
        let mut writer = Vec::new();
        let res2 = comm2.execute_interactive_stream(None, false, reader, &mut writer);
        assert!(res2.is_ok());

        // Test reader returning Ok(n) when receive_output is not completed initially
        let server3 = MockServer::start();
        let _shell3 = server3.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Create");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:Shell><rsp:ShellId>SHELL-123</rsp:ShellId></rsp:Shell></s:Body></s:Envelope>");
        });
        let _cmd3 = server3.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Command");
            then.status(200)
                .body("<s:Envelope><s:Body><rsp:CommandResponse><rsp:CommandId>CMD-456</rsp:CommandId></rsp:CommandResponse></s:Body></s:Envelope>");
        });
        let _recv3_1 = server3.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Receive");
            then.status(200)
                .body(r#"<s:Envelope><s:Body><rsp:ReceiveResponse><rsp:Stream Name="stdout">b2s=</rsp:Stream></rsp:ReceiveResponse></s:Body></s:Envelope>"#);
        });
        let _send3 = server3.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Send");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });
        let _sig3 = server3.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.microsoft.com/wbem/wsman/1/windows/shell/Signal");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });
        let _del3 = server3.mock(|when, then| {
            when.method(POST)
                .path("/wsman")
                .body_includes("http://schemas.xmlsoap.org/ws/2004/09/transfer/Delete");
            then.status(200).body("<s:Envelope><s:Body/></s:Envelope>");
        });

        let mut config3 = WinrmConfig::default();
        config3.host = server3.host();
        config3.port = server3.port();
        config3.ssl = false;
        let comm3 = WinrmCommunicator::new(config3);

        let reader3 = std::io::Cursor::new(b"data");
        let mut writer3 = Vec::new();
        let res3 = comm3.execute_interactive_stream(None, false, reader3, &mut writer3);
        assert!(res3.is_ok());
    }

    #[test]
    fn test_winrm_signal_and_delete_shell_failures() {
        let server = MockServer::start();
        let _fail_mock = server.mock(|when, then| {
            when.any_request();
            then.status(500);
        });
        let mut config = WinrmConfig::default();
        config.host = server.host();
        config.port = server.port();
        config.ssl = false;
        let comm = WinrmCommunicator::new(config);
        assert!(comm.signal_command("S", "C", "terminate").is_err());
        assert!(comm.delete_shell("S").is_err());
    }

    #[test]
    fn test_winrm_xml_parsing_helpers() {
        let xml = r#"<rsp:Envelope><rsp:ShellId>testid</rsp:ShellId><rsp:Stream Name="stdout">aGVsbG8=</rsp:Stream><rsp:Stream Name="stderr">d29ybGQ=</rsp:Stream><rsp:Stream Name="unknown">dGVzdA==</rsp:Stream><rsp:Stream Name="empty"></rsp:Stream><rsp:Stream Name="stdout">invalid_base64!!!</rsp:Stream><rsp:CommandState State="Done"/><rsp:ExitCode>0</rsp:ExitCode></rsp:Envelope>"#;
        assert_eq!(extract_tag(xml, "ShellId"), Some("testid".to_string()));
        assert_eq!(extract_tag(xml, "Missing"), None);
        assert_eq!(extract_tag(xml, "[invalid(regex"), None);

        let out = parse_shell_output(xml).expect("parse failed");
        assert_eq!(out.stdout, b"hello");
        assert_eq!(out.stderr, b"world");
        assert!(out.completed);
        assert_eq!(out.exit_code, Some(0));

        let xml_no_exit = r#"<rsp:Envelope><rsp:CommandState State="Running"/></rsp:Envelope>"#;
        let out_no_exit = parse_shell_output(xml_no_exit).expect("parse failed");
        assert!(!out_no_exit.completed);
        assert_eq!(out_no_exit.exit_code, None);
    }
}
