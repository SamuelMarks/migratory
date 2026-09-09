//! Semantic implementation of the `validate` command.
//!
//! This module provides the logic to validate the syntax and configuration
//! of an existing Vagrantfile.

use crate::cli::ValidateArgs;
use crate::config;
use crate::error::MigratoryError;
use std::path::Path;

/// Executes the `validate` command, checking if the Vagrantfile exists and can be evaluated.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `validate` command.
///
/// # Returns
///
/// Returns `Ok(())` on success, indicating the file exists and is valid.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile does not exist or if it fails evaluation.
pub fn execute(cwd: &Path, args: &ValidateArgs) -> Result<(), MigratoryError> {
    let path = cwd.join("Vagrantfile");
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    if args.ignore_provider {
        println!("Validating Vagrantfile (ignoring provider validation)...");
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    match config::evaluate_vagrantfile(path_str) {
        Ok(_) => {
            println!("Vagrantfile validated successfully.");
            Ok(())
        }
        Err(e) => {
            println!("Vagrantfile validation failed.");
            Err(e)
        }
    }
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_validate_missing() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = ValidateArgs {
            ignore_provider: false,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_validate_success() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = ValidateArgs {
            ignore_provider: true,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_validate_failure() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write invalid ruby to make parsing fail
        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")
            .expect("operation should succeed");

        let args = ValidateArgs {
            ignore_provider: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }
}
