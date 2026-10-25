//! Autocomplete command implementation.
//!
//! Handles the `autocomplete` subcommand to install shell completion scripts.

use crate::cli::{AutocompleteCommands, AutocompleteInstallArgs, Cli};
use crate::error::MigratoryError;
use clap::CommandFactory;
use clap_complete::{Shell, generate};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Executes the `autocomplete` command.
///
/// # Arguments
///
/// * `cmd` - The parsed `AutocompleteCommands` subcommand.
/// * `writer` - A writable destination for output (e.g. stdout).
///
/// # Returns
///
/// Returns `Ok(())` on successful execution, or a `MigratoryError` on failure.
///
/// # Errors
///
/// Returns a `MigratoryError` if writing to the output stream or modifying the profile fails.
pub fn execute(cmd: &AutocompleteCommands, mut writer: impl Write) -> Result<(), MigratoryError> {
    let shell_env = std::env::var("SHELL").unwrap_or_default();
    let home_dir = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let vagrant_home = crate::config::get_vagrant_home();
    execute_with_env(cmd, &mut writer, &shell_env, &home_dir, &vagrant_home)
}

/// Executes the `autocomplete` command with explicit environment configuration.
///
/// # Arguments
///
/// * `cmd` - The parsed `AutocompleteCommands` subcommand.
/// * `writer` - A writable destination for output.
/// * `shell_env` - Value of the `SHELL` environment variable.
/// * `home_dir` - User's home directory path.
/// * `vagrant_home` - Vagrant home directory path.
///
/// # Returns
///
/// Returns `Ok(())` on successful execution, or a `MigratoryError` on failure.
///
/// # Errors
///
/// Returns a `MigratoryError` if writing to the output stream or modifying the profile fails.
pub fn execute_with_env(
    cmd: &AutocompleteCommands,
    writer: &mut dyn Write,
    shell_env: &str,
    home_dir: &std::path::Path,
    vagrant_home: &std::path::Path,
) -> Result<(), MigratoryError> {
    match cmd {
        AutocompleteCommands::Install(args) => {
            install_autocomplete(args, writer, shell_env, home_dir, vagrant_home)
        }
    }
}

/// Determines the completion script filename and profile path for a target shell.
///
/// # Arguments
///
/// * `shell` - Target shell variant (`Bash`, `Zsh`, `Fish`, etc.).
/// * `home_dir` - User's home directory path.
///
/// # Returns
///
/// Returns a tuple of `(script_filename, profile_file_path)`.
fn get_shell_paths(shell: Shell, home_dir: &std::path::Path) -> (&'static str, PathBuf) {
    match shell {
        Shell::Zsh => ("migratory-autocomplete.zsh", home_dir.join(".zshrc")),
        Shell::Fish => ("migratory.fish", home_dir.join(".config/fish/config.fish")),
        _ => ("migratory-autocomplete.bash", home_dir.join(".bashrc")),
    }
}

/// Ensures that the parent directory of `path` exists, creating it recursively if needed.
///
/// # Arguments
///
/// * `path` - File path whose parent directory should be created.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if creating the parent directory fails.
fn ensure_parent_dir(path: &std::path::Path) -> Result<(), MigratoryError> {
    if let Some(parent) = path.parent() {
        if parent.as_os_str().is_empty() {
            return Ok(());
        }
        fs::create_dir_all(parent).map_err(|e| {
            MigratoryError::Generic(format!(
                "Failed to create profile dir {}: {}",
                parent.display(),
                e
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
struct FailingWriter;

#[cfg(test)]
impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("write failed"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Appends the completion sourcing block to the opened shell profile stream.
///
/// # Arguments
///
/// * `file` - Mutable writer for the target shell profile file.
/// * `append_content` - Completion block text to append.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if writing to the profile file fails.
fn append_to_profile_impl(
    file: &mut dyn Write,
    append_content: &str,
) -> Result<(), MigratoryError> {
    file.write_all(append_content.as_bytes())
        .map_err(|e| MigratoryError::Generic(format!("Failed to append to profile file: {}", e)))?;
    Ok(())
}

/// Generates and installs the shell autocomplete script and updates the user's shell profile.
///
/// # Arguments
///
/// * `args` - Parsed installation arguments specifying target shells.
/// * `writer` - Output writer for confirmation messages.
/// * `shell_env` - Value of the `SHELL` environment variable.
/// * `home_dir` - User's home directory path.
/// * `vagrant_home` - Vagrant home directory path.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if generating the completion script or updating the profile fails.
fn install_autocomplete(
    args: &AutocompleteInstallArgs,
    writer: &mut dyn Write,
    shell_env: &str,
    home_dir: &std::path::Path,
    vagrant_home: &std::path::Path,
) -> Result<(), MigratoryError> {
    let shell = if args.bash {
        Shell::Bash
    } else if args.zsh {
        Shell::Zsh
    } else if args.fish {
        Shell::Fish
    } else if shell_env.contains("zsh") {
        Shell::Zsh
    } else if shell_env.contains("fish") {
        Shell::Fish
    } else {
        Shell::Bash
    };

    fs::create_dir_all(vagrant_home)
        .map_err(|e| MigratoryError::Generic(format!("Failed to create vagrant home: {}", e)))?;

    let (script_name, profile_file) = get_shell_paths(shell, home_dir);

    let script_path = vagrant_home.join(script_name);
    let profile_path = profile_file;

    ensure_parent_dir(&profile_path)?;

    let mut script_file = fs::File::create(&script_path).map_err(|e| {
        MigratoryError::Generic(format!(
            "Failed to create script file {}: {}",
            script_path.display(),
            e
        ))
    })?;

    let mut app = Cli::command();
    generate(shell, &mut app, "migratory", &mut script_file);

    let profile_content = if profile_path.exists() {
        fs::read_to_string(&profile_path).unwrap_or_default()
    } else {
        String::new()
    };

    let marker_start = "# >>>> Migratory command completion (start)";
    let marker_end = "# <<<<  Migratory command completion (end)";

    if !profile_content.contains(marker_start) {
        let source_cmd = format!("source \"{}\"", script_path.display());
        let append_content = format!("\n{}\n{}\n{}\n", marker_start, source_cmd, marker_end);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&profile_path)
            .map_err(|e| {
                MigratoryError::Generic(format!(
                    "Failed to open profile file {}: {}",
                    profile_path.display(),
                    e
                ))
            })?;
        append_to_profile_impl(&mut file, &append_content)?;
    }

    writeln!(
        writer,
        "Autocomplete installed at paths:\n- {}",
        profile_path.display()
    )
    .map_err(|e| MigratoryError::Generic(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_env() -> Result<(tempfile::TempDir, PathBuf, PathBuf), MigratoryError> {
        let temp = tempdir()?;
        let home_dir = temp.path().join("home");
        fs::create_dir_all(&home_dir)?;
        let vagrant_home = home_dir.join(".vagrant.d");
        Ok((temp, home_dir, vagrant_home))
    }

    #[test]
    fn test_execute_install_bash() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute_with_env(&cmd, &mut out, "/bin/bash", &home_dir, &vagrant_home).is_ok());

        let output_str = String::from_utf8(out)?;
        assert!(output_str.contains("Autocomplete installed at paths:"));
        assert!(output_str.contains(".bashrc"));

        assert!(vagrant_home.join("migratory-autocomplete.bash").exists());

        let profile_content = fs::read_to_string(home_dir.join(".bashrc"))?;
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
        Ok(())
    }

    #[test]
    fn test_execute_install_zsh() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;

        let args = AutocompleteInstallArgs {
            bash: false,
            zsh: true,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute_with_env(&cmd, &mut out, "/bin/bash", &home_dir, &vagrant_home).is_ok());

        let output_str = String::from_utf8(out)?;
        assert!(output_str.contains(".zshrc"));

        let profile_content = fs::read_to_string(home_dir.join(".zshrc"))?;
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
        Ok(())
    }

    #[test]
    fn test_execute_install_fish() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;

        let args = AutocompleteInstallArgs {
            bash: false,
            zsh: false,
            fish: true,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute_with_env(&cmd, &mut out, "/bin/bash", &home_dir, &vagrant_home).is_ok());

        let output_str = String::from_utf8(out)?;
        assert!(output_str.contains(".config/fish/config.fish"));

        let profile_content = fs::read_to_string(home_dir.join(".config/fish/config.fish"))?;
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
        Ok(())
    }

    #[test]
    fn test_execute_install_shell_env_zsh() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;

        let args = AutocompleteInstallArgs {
            bash: false,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute_with_env(&cmd, &mut out, "/bin/zsh", &home_dir, &vagrant_home).is_ok());

        let profile_content = fs::read_to_string(home_dir.join(".zshrc"))?;
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
        Ok(())
    }

    #[test]
    fn test_execute_install_shell_env_fish() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;

        let args = AutocompleteInstallArgs {
            bash: false,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(
            execute_with_env(
                &cmd,
                &mut out,
                "/opt/homebrew/bin/fish",
                &home_dir,
                &vagrant_home
            )
            .is_ok()
        );

        let profile_content = fs::read_to_string(home_dir.join(".config/fish/config.fish"))?;
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
        Ok(())
    }

    #[test]
    fn test_execute_install_shell_env_bash() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;

        let args = AutocompleteInstallArgs {
            bash: false,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute_with_env(&cmd, &mut out, "/bin/bash", &home_dir, &vagrant_home).is_ok());

        let profile_content = fs::read_to_string(home_dir.join(".bashrc"))?;
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
        Ok(())
    }

    #[test]
    fn test_execute_install_already_installed() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute_with_env(&cmd, &mut out, "/bin/bash", &home_dir, &vagrant_home).is_ok());

        let mut out2 = Vec::new();
        assert!(execute_with_env(&cmd, &mut out2, "/bin/bash", &home_dir, &vagrant_home).is_ok());

        let profile_content = fs::read_to_string(home_dir.join(".bashrc"))?;
        let count = profile_content
            .matches("# >>>> Migratory command completion (start)")
            .count();
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn test_execute_install_write_error() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);

        let mut out = FailingWriter;
        let result = execute_with_env(&cmd, &mut out, "/bin/bash", &home_dir, &vagrant_home);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_install_vagrant_home_creation_error() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;

        fs::write(&vagrant_home, "not a dir")?;

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();

        let result = execute_with_env(&cmd, &mut out, "/bin/bash", &home_dir, &vagrant_home);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_install_script_creation_error() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;

        fs::create_dir_all(&vagrant_home)?;
        fs::create_dir(vagrant_home.join("migratory-autocomplete.bash"))?;

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();

        let result = execute_with_env(&cmd, &mut out, "/bin/bash", &home_dir, &vagrant_home);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_install_profile_creation_error() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;

        let profile = home_dir.join(".bashrc");
        fs::create_dir(&profile)?;

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();

        let result = execute_with_env(&cmd, &mut out, "/bin/bash", &home_dir, &vagrant_home);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_get_shell_paths_unsupported() {
        let result = super::get_shell_paths(
            clap_complete::Shell::PowerShell,
            std::path::Path::new("/home"),
        );
        assert_eq!(result.0, "migratory-autocomplete.bash");
    }

    #[test]
    fn test_execute_install_profile_exists() -> Result<(), MigratoryError> {
        let (_temp, home_dir, vagrant_home) = setup_env()?;
        let profile = home_dir.join(".bashrc");
        std::fs::write(&profile, "existing content")?;

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        let result = execute_with_env(&cmd, &mut out, "/bin/bash", &home_dir, &vagrant_home);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_ensure_parent_dir_none() {
        let result = super::ensure_parent_dir(std::path::Path::new("/"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_ensure_parent_dir_empty() {
        let result = super::ensure_parent_dir(std::path::Path::new("file_only.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_append_to_profile_impl_error() {
        let mut writer = FailingWriter;
        let result = super::append_to_profile_impl(&mut writer, "content");
        assert!(result.is_err());
    }

    #[test]
    fn test_failing_writer_flush() {
        use std::io::Write;
        let mut writer = FailingWriter;
        assert!(writer.flush().is_ok());
    }
}

#[cfg(test)]
mod extra_autocomplete_tests {
    use super::*;
    use tempfile::tempdir;

    /// Tests error when profile parent dir cannot be created.
    #[test]
    fn test_execute_install_profile_dir_creation_error() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let home_dir = temp.path().join("home_file");
        std::fs::write(&home_dir, "file")?;
        let vagrant_home = temp.path().join("vagrant");

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();

        let result = execute_with_env(&cmd, &mut out, "/bin/bash", &home_dir, &vagrant_home);
        assert!(result.is_err());

        let args_none = AutocompleteInstallArgs {
            fish: false,
            bash: false,
            zsh: false,
        };
        let _ = execute(&AutocompleteCommands::Install(args_none), &mut Vec::new());
        Ok(())
    }
}
