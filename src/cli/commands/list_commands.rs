//! Semantic implementation of the `list-commands` command.

use crate::error::MigratoryError;
use clap::CommandFactory;

/// Executes the `list-commands` command, printing available commands.
///
/// # Errors
/// Returns a `MigratoryError` if the command information cannot be retrieved or printed.
pub fn execute() -> Result<(), MigratoryError> {
    let cmd = crate::cli::Cli::command();
    println!(
        "Below is a listing of all available Migratory commands and a brief\ndescription of what they do.\n"
    );
    for subcmd in cmd.get_subcommands() {
        let name = subcmd.get_name();
        let about = subcmd
            .get_about()
            .map(|a| a.to_string())
            .unwrap_or_default();
        println!("{:<15} {}", name, about);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_list_commands() {
        // Assert that executing the list-commands command does not panic and returns Ok.
        let result = execute();
        assert!(result.is_ok());
    }
}
