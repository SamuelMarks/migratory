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
    match cmd {
        AutocompleteCommands::Install(args) => install_autocomplete(args, &mut writer),
    }
}

fn get_shell_paths(shell: Shell, home_dir: &std::path::Path) -> (&'static str, PathBuf) {
    match shell {
        Shell::Zsh => ("migratory-autocomplete.zsh", home_dir.join(".zshrc")),
        Shell::Fish => ("migratory.fish", home_dir.join(".config/fish/config.fish")),
        _ => ("migratory-autocomplete.bash", home_dir.join(".bashrc")),
    }
}

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
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "write failed",
        ))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn append_to_profile_impl(
    file: &mut dyn Write,
    append_content: &str,
) -> Result<(), MigratoryError> {
    #[cfg(test)]
    let mut failing = FailingWriter;
    #[cfg(test)]
    let file = if std::env::var("MIGRATORY_TEST_MOCK_PROFILE_WRITE_ERROR").is_ok() {
        &mut failing as &mut dyn Write
    } else {
        file
    };

    file.write_all(append_content.as_bytes())
        .map_err(|e| MigratoryError::Generic(format!("Failed to append to profile file: {}", e)))?;
    Ok(())
}

fn install_autocomplete(
    args: &AutocompleteInstallArgs,
    writer: &mut dyn Write,
) -> Result<(), MigratoryError> {
    let shell = if args.bash {
        Shell::Bash
    } else if args.zsh {
        Shell::Zsh
    } else if args.fish {
        Shell::Fish
    } else {
        let shell_env = std::env::var("SHELL").unwrap_or_default();
        if shell_env.contains("zsh") {
            Shell::Zsh
        } else if shell_env.contains("fish") {
            Shell::Fish
        } else {
            Shell::Bash
        }
    };

    let home_dir = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("~"));

    let vagrant_home = std::env::var("VAGRANT_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir.join(".vagrant.d"));

    fs::create_dir_all(&vagrant_home)
        .map_err(|e| MigratoryError::Generic(format!("Failed to create vagrant home: {}", e)))?;

    let (script_name, profile_file) = get_shell_paths(shell, &home_dir);

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

    fn setup_env() -> (tempfile::TempDir, PathBuf) {
        let temp = tempdir().expect("operation should succeed");
        let home_dir = temp.path().join("home");
        fs::create_dir_all(&home_dir).expect("operation should succeed");

        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::set_var("VAGRANT_HOME", home_dir.join(".vagrant.d"));
            std::env::set_var("SHELL", "/bin/bash");
        }
        (temp, home_dir)
    }

    #[test]
    fn test_execute_install_bash() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let (_temp, home_dir) = setup_env();

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        let output_str = String::from_utf8(out).expect("Invalid utf8");
        assert!(output_str.contains("Autocomplete installed at paths:"));
        assert!(output_str.contains(".bashrc"));

        // Verify script exists
        let vagrant_dir = home_dir.join(".vagrant.d");
        assert!(vagrant_dir.join("migratory-autocomplete.bash").exists());

        // Verify profile modified
        let profile_content = fs::read_to_string(home_dir.join(".bashrc")).unwrap();
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
    }

    #[test]
    fn test_execute_install_zsh() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let (_temp, home_dir) = setup_env();

        let args = AutocompleteInstallArgs {
            bash: false,
            zsh: true,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        let output_str = String::from_utf8(out).expect("Invalid utf8");
        assert!(output_str.contains(".zshrc"));

        // Verify profile modified
        let profile_content = fs::read_to_string(home_dir.join(".zshrc")).unwrap();
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
    }

    #[test]
    fn test_execute_install_fish() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let (_temp, home_dir) = setup_env();

        let args = AutocompleteInstallArgs {
            bash: false,
            zsh: false,
            fish: true,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        let output_str = String::from_utf8(out).expect("Invalid utf8");
        assert!(output_str.contains(".config/fish/config.fish"));

        // Verify profile modified
        let profile_content =
            fs::read_to_string(home_dir.join(".config/fish/config.fish")).unwrap();
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
    }

    #[test]
    fn test_execute_install_shell_env_zsh() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let (_temp, home_dir) = setup_env();
        unsafe {
            std::env::set_var("SHELL", "/bin/zsh");
        }

        let args = AutocompleteInstallArgs {
            bash: false,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        let profile_content = fs::read_to_string(home_dir.join(".zshrc")).unwrap();
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
    }

    #[test]
    fn test_execute_install_shell_env_fish() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let (_temp, home_dir) = setup_env();
        unsafe {
            std::env::set_var("SHELL", "/opt/homebrew/bin/fish");
        }

        let args = AutocompleteInstallArgs {
            bash: false,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        let profile_content =
            fs::read_to_string(home_dir.join(".config/fish/config.fish")).unwrap();
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
    }

    #[test]
    fn test_execute_install_shell_env_bash() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let (_temp, home_dir) = setup_env();
        unsafe {
            std::env::set_var("SHELL", "/bin/bash");
        }

        let args = AutocompleteInstallArgs {
            bash: false,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        let profile_content = fs::read_to_string(home_dir.join(".bashrc")).unwrap();
        assert!(profile_content.contains("# >>>> Migratory command completion (start)"));
    }

    #[test]
    fn test_execute_install_already_installed() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let (_temp, home_dir) = setup_env();

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args.clone());
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        // Run again
        let mut out2 = Vec::new();
        assert!(execute(&cmd, &mut out2).is_ok());

        // Ensure marker isn't duplicated
        let profile_content = fs::read_to_string(home_dir.join(".bashrc")).unwrap();
        let count = profile_content
            .matches("# >>>> Migratory command completion (start)")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_execute_install_write_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let (_temp, _home_dir) = setup_env();

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);

        let mut out = FailingWriter;
        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_install_vagrant_home_creation_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let (_temp, home_dir) = setup_env();

        // Make VAGRANT_HOME a file so it fails to create a directory
        let vh = home_dir.join(".vagrant.d");
        fs::write(&vh, "not a dir").unwrap();

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();

        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_install_script_creation_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let (_temp, home_dir) = setup_env();

        let vh = home_dir.join(".vagrant.d");
        fs::create_dir_all(&vh).unwrap();
        // Make the script path a directory so it fails to create a file
        fs::create_dir(vh.join("migratory-autocomplete.bash")).unwrap();

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();

        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_install_profile_creation_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let (_temp, home_dir) = setup_env();

        // Make .bashrc a directory so it fails to open it
        let profile = home_dir.join(".bashrc");
        fs::create_dir(&profile).unwrap();

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();

        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
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
    fn test_execute_install_profile_exists() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .unwrap();
        let (_temp, home_dir) = setup_env();
        let profile = home_dir.join(".bashrc");
        std::fs::write(&profile, "existing content").unwrap();

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        let result = execute(&cmd, &mut out);
        assert!(result.is_ok());
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
    use crate::cli::commands::box_cmd::tests::ENV_LOCK;
    use tempfile::tempdir;

    /// Tests error when profile parent dir cannot be created.
    #[test]
    fn test_execute_install_profile_dir_creation_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");

        let temp = tempdir().expect("operation should succeed");
        let home_dir = temp.path().join("home_file");
        std::fs::write(&home_dir, "file").expect("operation should succeed");

        unsafe { std::env::set_var("HOME", &home_dir) };
        unsafe { std::env::set_var("VAGRANT_HOME", temp.path().join("vagrant")) };

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();

        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
    }

    /// Tests fallback when HOME is unset.
    #[test]
    fn test_execute_install_home_unset() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let orig_cwd = std::env::current_dir().expect("operation should succeed");
        let temp = tempdir().expect("operation should succeed");
        std::env::set_current_dir(temp.path()).expect("operation should succeed");

        let orig_home = std::env::var("HOME").unwrap_or_default();

        unsafe { std::env::remove_var("HOME") };
        unsafe { std::env::set_var("VAGRANT_HOME", temp.path().join("vagrant")) };

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        let _ = execute(&cmd, &mut out);

        unsafe { std::env::set_var("HOME", orig_home) };
        unsafe { std::env::remove_var("VAGRANT_HOME") };
        std::env::set_current_dir(orig_cwd).expect("operation should succeed");
    }

    /// Tests fallback when VAGRANT_HOME is unset.
    #[test]
    fn test_execute_install_vagrant_home_unset() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let temp = tempdir().expect("operation should succeed");
        let home_dir = temp.path().join("home");
        std::fs::create_dir_all(&home_dir).expect("operation should succeed");

        unsafe { std::env::set_var("HOME", &home_dir) };
        unsafe { std::env::remove_var("VAGRANT_HOME") };

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        unsafe { std::env::remove_var("VAGRANT_HOME") };
    }

    /// Tests profile file write error during append.
    #[test]
    fn test_execute_install_profile_write_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let temp = tempdir().expect("operation should succeed");
        let home_dir = temp.path().join("home");
        std::fs::create_dir_all(&home_dir).expect("operation should succeed");

        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::set_var("VAGRANT_HOME", home_dir.join(".vagrant.d"));
            std::env::set_var("MIGRATORY_TEST_MOCK_PROFILE_WRITE_ERROR", "1");
        }

        let args = AutocompleteInstallArgs {
            bash: true,
            zsh: false,
            fish: false,
        };
        let cmd = AutocompleteCommands::Install(args);
        let mut out = Vec::new();
        let res = execute(&cmd, &mut out);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_PROFILE_WRITE_ERROR");
        }
        assert!(res.is_err());
    }
}
