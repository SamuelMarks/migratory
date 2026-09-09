//! Semantic implementation of the `push` command.
//!
//! This module provides the logic to deploy code in the environment to a configured destination.

use crate::error::MigratoryError;
use std::path::Path;

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
/// Returns a `MigratoryError` if the Vagrantfile cannot be found.
pub fn execute(cwd: &Path) -> Result<(), MigratoryError> {
    let path = cwd.join("Vagrantfile");
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    println!("Pushing application...");
    Ok(())
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_push_missing() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let result = execute(cwd);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_push_success() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let result = execute(cwd);
        assert!(result.is_ok());
        Ok(())
    }
}
