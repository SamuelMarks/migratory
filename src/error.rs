//! Core error definitions.
//!
//! This module provides the central error type used throughout the Migratory
//! crate, ensuring consistent error handling and reporting.

use derive_more::derive::{Display, Error, From};
use std::io;

/// Centralized error enum for Migratory.
///
/// This enum encapsulates all possible error states that can occur during
/// execution, using \`derive_more\` for ergonomic formatting and standard
/// library error trait implementations.
#[derive(Debug, Display, Error, From)]
pub enum MigratoryError {
    /// A generic error string, often used as a fallback for external library errors.
    #[display("Generic error: {}", _0)]
    #[error(ignore)]
    #[from(ignore)]
    Generic(String),

    /// Represents an underlying \`std::io::Error\`.
    #[display("I/O error: {}", _0)]
    Io(io::Error),

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

    /// Indicates a validation or configuration error.
    #[display("Validation error: {}", _0)]
    #[error(ignore)]
    #[from(ignore)]
    Validation(String),

    /// Indicates an error during JSON serialization or deserialization.
    #[display("JSON error: {}", _0)]
    Json(serde_json::Error),

    /// Indicates a network request error (e.g. from reqwest).
    #[display("Network error: {}", _0)]
    Reqwest(reqwest::Error),

    /// Indicates an error parsing or evaluating a regular expression.
    #[display("Regex error: {}", _0)]
    Regex(regex::Error),

    /// Indicates a failure during process or command execution.
    #[display("Command execution failed: {}", _0)]
    #[error(ignore)]
    #[from(ignore)]
    Command(String),
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::all,
        clippy::panic,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::undocumented_unsafe_blocks
    )]
    use super::*;
    use std::error::Error;

    #[test]
    fn test_error_display() {
        assert_eq!(
            MigratoryError::Generic("test".to_string()).to_string(),
            "Generic error: test"
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

        let io_err = io::Error::new(io::ErrorKind::NotFound, "io fail");
        assert_eq!(MigratoryError::Io(io_err).to_string(), "I/O error: io fail");
    }

    #[test]
    fn test_error_source() {
        let generic_err = MigratoryError::Generic("test".to_string());
        assert!(generic_err.source().is_none());

        let io_err = io::Error::new(io::ErrorKind::Other, "test error");
        let mig_err = MigratoryError::Io(io_err);
        assert!(mig_err.source().is_some());
    }

    #[test]
    fn test_from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let mig_err: MigratoryError = io_err.into();
        let is_io = match mig_err {
            MigratoryError::Io(_) => true,
            _ => false,
        };
        assert!(is_io);
    }
}
