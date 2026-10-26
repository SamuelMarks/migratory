//! Core error definitions.
//!
//! This module provides the central error type used throughout the Migratory
//! crate, ensuring consistent error handling and reporting via a single
//! strongly-typed enum derived with `derive_more`.

use derive_more::derive::{Display, Error, From};
use std::io;
use std::path::PathBuf;

/// Centralized error enum for Migratory.
///
/// This enum encapsulates all possible error states that can occur during
/// execution, using `derive_more` for ergonomic formatting, `From` conversions,
/// and standard library `Error` trait implementations.
#[derive(Debug, Display, Error, From)]
pub enum MigratoryError {
    /// A generic error string, used for general runtime messages.
    #[display("Generic error: {}", _0)]
    #[error(ignore)]
    #[from(ignore)]
    Generic(String),

    /// Represents an underlying `std::io::Error`.
    #[display("I/O error: {}", _0)]
    Io(io::Error),

    /// Indicates a configuration parsing or evaluation failure.
    #[display("Configuration error: {}", _0)]
    #[error(ignore)]
    #[from(ignore)]
    Config(String),

    /// Indicates that a requested resource (like a file or directory) already exists.
    #[display("Already exists: {}", _0)]
    #[error(ignore)]
    #[from(ignore)]
    AlreadyExists(String),

    /// Indicates that a requested resource was not found.
    #[display("Not found: {}", _0)]
    #[error(ignore)]
    #[from(ignore)]
    NotFound(String),

    /// Indicates a validation error in user input or configuration.
    #[display("Validation error: {}", _0)]
    #[error(ignore)]
    #[from(ignore)]
    Validation(String),

    /// Indicates an error during JSON serialization or deserialization.
    #[display("JSON error: {}", _0)]
    Json(serde_json::Error),

    /// Indicates a network request error from `reqwest`.
    #[display("Network error: {}", _0)]
    Reqwest(reqwest::Error),

    /// Indicates an error parsing or evaluating a regular expression.
    #[display("Regex error: {}", _0)]
    Regex(regex::Error),

    /// Indicates an error from the underlying `ssh2` library.
    #[display("SSH error: {}", _0)]
    Ssh(ssh2::Error),

    /// Indicates a filesystem watcher error from `notify`.
    #[display("Filesystem watch error: {}", _0)]
    Notify(notify::Error),

    /// Indicates a Base64 decoding error.
    #[display("Base64 decode error: {}", _0)]
    Base64(base64::DecodeError),

    /// Indicates a UTF-8 string conversion error from owned bytes.
    #[display("UTF-8 conversion error: {}", _0)]
    Utf8(std::string::FromUtf8Error),

    /// Indicates a UTF-8 slice validation error.
    #[display("UTF-8 slice error: {}", _0)]
    StrUtf8(std::str::Utf8Error),

    /// Indicates an integer parsing error.
    #[display("Integer parse error: {}", _0)]
    ParseInt(std::num::ParseIntError),

    /// Indicates a failure during process or command execution.
    #[display("Command execution failed: {}", _0)]
    #[error(ignore)]
    #[from(ignore)]
    Command(String),

    /// Indicates a structured external CLI command execution failure.
    #[display("Command '{}' failed (status: {:?}): {}", program, status, stderr)]
    #[error(ignore)]
    #[from(ignore)]
    CommandExecution {
        /// The executable program name or path.
        program: String,
        /// The exit code, if terminated normally.
        status: Option<i32>,
        /// Captured standard error output.
        stderr: String,
    },

    /// Indicates that the Vagrant environment lock is currently held by another process.
    #[display("Vagrant environment at '{}' is locked by another process", path.display())]
    #[error(ignore)]
    #[from(ignore)]
    EnvironmentLocked {
        /// Path to the locked state directory or lock file.
        path: PathBuf,
    },

    /// Indicates that the requested hypervisor provider is unknown or unsupported.
    #[display("Unknown provider: {}", provider)]
    #[error(ignore)]
    #[from(ignore)]
    UnsupportedProvider {
        /// The unsupported provider identifier.
        provider: String,
    },

    /// Indicates that a specific lifecycle operation is not supported by the provider.
    #[display("{} not supported by provider: {}", operation, provider)]
    #[error(ignore)]
    #[from(ignore)]
    UnsupportedOperation {
        /// The provider name.
        provider: String,
        /// The unsupported operation description.
        operation: String,
    },

    /// Indicates that a machine is in an unexpected state for the requested operation.
    #[display(
        "The VM '{}' is in state '{}', but expected one of: {:?}",
        machine,
        current,
        expected
    )]
    #[error(ignore)]
    #[from(ignore)]
    InvalidMachineState {
        /// Target machine name.
        machine: String,
        /// Current machine state string.
        current: String,
        /// List of acceptable states for the operation.
        expected: Vec<String>,
    },

    /// Indicates an authentication failure with a guest or remote service.
    #[display("Authentication failed for '{}': {}", target, reason)]
    #[error(ignore)]
    #[from(ignore)]
    AuthenticationFailed {
        /// Authentication target (e.g., host or service name).
        target: String,
        /// Explanation of the authentication failure.
        reason: String,
    },

    /// Indicates that an operation exceeded its configured timeout.
    #[display("Operation '{}' timed out after {}s", operation, duration_secs)]
    #[error(ignore)]
    #[from(ignore)]
    Timeout {
        /// The operation that timed out.
        operation: String,
        /// Timeout threshold in seconds.
        duration_secs: u64,
    },

    /// Indicates that an error occurred during middleware rollback/recovery.
    #[display(
        "Error during recovery: {}, original error: {}",
        recovery_error,
        original_error
    )]
    #[error(ignore)]
    #[from(ignore)]
    RecoveryFailed {
        /// The error that occurred during the recovery phase.
        recovery_error: Box<MigratoryError>,
        /// The original error that triggered recovery.
        original_error: Box<MigratoryError>,
    },
}

/// Extension trait for attaching contextual messages to `Result` values
/// and converting errors into [`MigratoryError`].
pub trait ResultExt<T> {
    /// Wraps an error with a descriptive context prefix into a [`MigratoryError::Generic`].
    ///
    /// # Arguments
    ///
    /// * `msg` - Contextual prefix describing the operation that failed.
    ///
    /// # Returns
    ///
    /// Returns `Ok(T)` on success, or `Err(MigratoryError)` with context on failure.
    ///
    /// # Errors
    ///
    /// Returns [`MigratoryError::Generic`] if the underlying result is `Err`.
    fn wrap_err(self, msg: &str) -> Result<T, MigratoryError>;

    /// Converts any displayable error directly into [`MigratoryError::Generic`].
    ///
    /// # Returns
    ///
    /// Returns `Ok(T)` on success, or `Err(MigratoryError)` on failure.
    ///
    /// # Errors
    ///
    /// Returns [`MigratoryError::Generic`] if the underlying result is `Err`.
    fn wrap_generic(self) -> Result<T, MigratoryError>;
}

impl<T, E: std::fmt::Display> ResultExt<T> for Result<T, E> {
    fn wrap_err(self, msg: &str) -> Result<T, MigratoryError> {
        self.map_err(|e| MigratoryError::Generic(format!("{}: {}", msg, e)))
    }

    fn wrap_generic(self) -> Result<T, MigratoryError> {
        self.map_err(|e| MigratoryError::Generic(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn test_error_display_and_variants() -> Result<(), MigratoryError> {
        assert_eq!(
            MigratoryError::Generic("test".to_string()).to_string(),
            "Generic error: test"
        );
        assert_eq!(
            MigratoryError::Config("bad config".to_string()).to_string(),
            "Configuration error: bad config"
        );
        assert_eq!(
            MigratoryError::AlreadyExists("file".to_string()).to_string(),
            "Already exists: file"
        );
        assert_eq!(
            MigratoryError::NotFound("file".to_string()).to_string(),
            "Not found: file"
        );
        assert_eq!(
            MigratoryError::Validation("bad value".to_string()).to_string(),
            "Validation error: bad value"
        );
        assert_eq!(
            MigratoryError::Command("cmd failed".to_string()).to_string(),
            "Command execution failed: cmd failed"
        );
        assert_eq!(
            MigratoryError::CommandExecution {
                program: "vboxmanage".to_string(),
                status: Some(1),
                stderr: "vbox err".to_string(),
            }
            .to_string(),
            "Command 'vboxmanage' failed (status: Some(1)): vbox err"
        );
        assert_eq!(
            MigratoryError::EnvironmentLocked {
                path: PathBuf::from("/tmp/.vagrant"),
            }
            .to_string(),
            "Vagrant environment at '/tmp/.vagrant' is locked by another process"
        );
        assert_eq!(
            MigratoryError::UnsupportedProvider {
                provider: "foo".to_string(),
            }
            .to_string(),
            "Unknown provider: foo"
        );
        assert_eq!(
            MigratoryError::UnsupportedOperation {
                provider: "docker".to_string(),
                operation: "Snapshot save".to_string(),
            }
            .to_string(),
            "Snapshot save not supported by provider: docker"
        );
        assert_eq!(
            MigratoryError::InvalidMachineState {
                machine: "default".to_string(),
                current: "poweroff".to_string(),
                expected: vec!["running".to_string()],
            }
            .to_string(),
            r#"The VM 'default' is in state 'poweroff', but expected one of: ["running"]"#
        );
        assert_eq!(
            MigratoryError::AuthenticationFailed {
                target: "127.0.0.1".to_string(),
                reason: "bad key".to_string(),
            }
            .to_string(),
            "Authentication failed for '127.0.0.1': bad key"
        );
        assert_eq!(
            MigratoryError::Timeout {
                operation: "ssh_wait".to_string(),
                duration_secs: 30,
            }
            .to_string(),
            "Operation 'ssh_wait' timed out after 30s"
        );
        assert_eq!(
            MigratoryError::RecoveryFailed {
                recovery_error: Box::new(MigratoryError::Generic("rec".to_string())),
                original_error: Box::new(MigratoryError::Generic("orig".to_string())),
            }
            .to_string(),
            "Error during recovery: Generic error: rec, original error: Generic error: orig"
        );

        let io_err = io::Error::new(io::ErrorKind::NotFound, "io fail");
        assert_eq!(MigratoryError::Io(io_err).to_string(), "I/O error: io fail");
        Ok(())
    }

    #[test]
    fn test_error_source_and_conversions() {
        let generic_err = MigratoryError::Generic("test".to_string());
        assert!(generic_err.source().is_none());

        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let mig_err: MigratoryError = io_err.into();
        assert!(mig_err.source().is_some());
        assert!(matches!(mig_err, MigratoryError::Io(_)));

        let int_res = "not_an_int".parse::<i32>().map_err(MigratoryError::from);
        assert!(int_res.is_err_and(|e| e.to_string().contains("Integer parse error:")));

        let _ = String::from_utf8(vec![0xFF, 0xFE]).map_err(|utf8_err| {
            let valid_err = utf8_err.utf8_error();
            let mig_str_utf8: MigratoryError = valid_err.into();
            let mig_utf8: MigratoryError = utf8_err.into();
            assert!(mig_str_utf8.to_string().contains("UTF-8 slice error:"));
            assert!(mig_utf8.to_string().contains("UTF-8 conversion error:"));
        });

        use base64::Engine;
        let b64_res = base64::engine::general_purpose::STANDARD
            .decode("%%%")
            .map_err(MigratoryError::from);
        assert!(b64_res.is_err_and(|e| e.to_string().contains("Base64 decode error:")));

        let invalid_pattern = String::from("[unclosed");
        let regex_res = regex::Regex::new(&invalid_pattern).map_err(MigratoryError::from);
        assert!(regex_res.is_err_and(|e| e.to_string().contains("Regex error:")));

        let json_res =
            serde_json::from_str::<serde_json::Value>("{bad").map_err(MigratoryError::from);
        assert!(json_res.is_err_and(|e| e.to_string().contains("JSON error:")));

        let ssh_err = ssh2::Error::from_errno(ssh2::ErrorCode::Session(-1));
        let mig_ssh: MigratoryError = ssh_err.into();
        assert!(mig_ssh.to_string().contains("SSH error:"));

        let notify_err = notify::Error::generic("watch failed");
        let mig_notify: MigratoryError = notify_err.into();
        assert!(mig_notify.to_string().contains("Filesystem watch error:"));
    }

    #[test]
    fn test_result_ext_helpers() {
        let ok_val: Result<i32, &str> = Ok(42);
        assert!(ok_val.wrap_err("ctx").is_ok());
        assert!(ok_val.wrap_generic().is_ok());

        let err_val: Result<i32, &str> = Err("boom");
        let wrapped = err_val.wrap_err("context");
        assert_eq!(
            wrapped.err().map(|e| e.to_string()),
            Some("Generic error: context: boom".to_string())
        );

        let wrapped_gen = err_val.wrap_generic();
        assert_eq!(
            wrapped_gen.err().map(|e| e.to_string()),
            Some("Generic error: boom".to_string())
        );
    }
}
