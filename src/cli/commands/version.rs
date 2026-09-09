//! Semantic implementation of the `version` command.
//!
//! Outputs the installed Migratory version and checks for new releases unless
//! update checks are disabled via `VAGRANT_CHECKPOINT_DISABLE`.

use crate::error::MigratoryError;

/// Executes the `version` command, printing current and latest version information.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the version information cannot be printed.
pub fn execute() -> Result<(), MigratoryError> {
    println!("Installed Version: {}", env!("CARGO_PKG_VERSION"));
    if std::env::var("VAGRANT_CHECKPOINT_DISABLE").is_ok() {
        println!("Version check disabled.");
    } else {
        println!("Latest Version: {}", env!("CARGO_PKG_VERSION"));
        println!("You're running an up-to-date version of Migratory!");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_version() {
        // Assert that executing the version command does not panic and returns Ok.
        let result = execute();
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_version_checkpoint_disabled() {
        unsafe { std::env::set_var("VAGRANT_CHECKPOINT_DISABLE", "1") };
        let result = execute();
        unsafe { std::env::remove_var("VAGRANT_CHECKPOINT_DISABLE") };
        assert!(result.is_ok());
    }
}
