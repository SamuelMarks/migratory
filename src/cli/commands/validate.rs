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
    if args.ignore_provider {
        println!("Validating Vagrantfile (ignoring provider validation)...");
    }

    match config::load_environment_config(cwd) {
        Ok(_) => {
            println!("Vagrantfile validated successfully.");
            Ok(())
        }
        Err(MigratoryError::NotFound(e)) => Err(MigratoryError::NotFound(e)),
        Err(e) => {
            println!("Vagrantfile validation failed.");
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_validate_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = ValidateArgs {
            ignore_provider: false,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_validate_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = ValidateArgs {
            ignore_provider: true,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_validate_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")?;

        let args = ValidateArgs {
            ignore_provider: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }
}
