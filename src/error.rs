//! Core error definitions.
//!
//! This module provides the central error type used throughout the Migratory
//! crate, ensuring consistent error handling and reporting.

use derive_more::derive::Display;
use std::io;

/// Centralized error enum for Migratory.
///
/// This enum encapsulates all possible error states that can occur during
/// execution, using `derive_more::Display` for ergonomic formatting.
#[derive(Debug, Display)]
pub enum MigratoryError {
    /// A generic error string, often used as a fallback for external library errors.
    #[display("Generic error: {}", _0)]
    Generic(String),

    /// Represents an underlying `std::io::Error`.
    #[display("I/O error: {}", _0)]
    Io(io::Error),

    /// Indicates that a requested resource (like a file or directory) already exists.
    #[display("Already exists: {}", _0)]
    AlreadyExists(String),

    /// Indicates that a requested resource was not found.
    #[display("Not found: {}", _0)]
    NotFound(String),

    /// Indicates a validation or configuration error.
    #[display("Validation error: {}", _0)]
    Validation(String),
}

impl std::error::Error for MigratoryError {
    /// Provides access to the underlying source error, if any.
    ///
    /// # Returns
    ///
    /// Returns `Some(err)` if this error wraps another error (like `io::Error`),
    /// otherwise returns `None`.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MigratoryError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for MigratoryError {
    /// Converts a standard `io::Error` into a `MigratoryError::Io`.
    ///
    /// # Arguments
    ///
    /// * `err` - The `io::Error` to convert.
    ///
    /// # Returns
    ///
    /// Returns the wrapped `MigratoryError`.
    fn from(err: io::Error) -> Self {
        MigratoryError::Io(err)
    }
}

#[cfg(test)]
#[coverage(off)]
mod tests {
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

        let not_io = match MigratoryError::Generic("foo".to_string()) {
            MigratoryError::Io(_) => true,
            _ => false,
        };
        assert!(!not_io);
    }
}
