//! Semantic implementation of the `plugin` command and its subcommands.
//!
//! This module provides the logic to manage plugins.

use crate::cli::PluginCommands;
use crate::error::MigratoryError;
use std::io::Write;
use std::process::Command;

/// Resolves the Vagrant home directory from optional VAGRANT_HOME and HOME environment strings.
///
/// # Arguments
///
/// * `vagrant_home` - The value of the `VAGRANT_HOME` environment variable, if set.
/// * `home` - The value of the `HOME` environment variable, if set.
///
/// # Returns
///
/// Returns the resolved directory path string.
pub fn resolve_vagrant_home(vagrant_home: Option<&str>, home: Option<&str>) -> String {
    vagrant_home
        .map(|s| s.to_string())
        .unwrap_or_else(|| match home {
            Some(h) => format!("{}/.vagrant.d", h),
            None => ".vagrant.d".to_string(),
        })
}

/// Executes a Ruby gem command using a custom `gem` binary and Vagrant home directory.
///
/// # Arguments
///
/// * `gem_bin` - Executable name or path for `gem`.
/// * `home_dir` - Resolved Vagrant home directory.
/// * `args` - Command arguments to pass to `gem`.
/// * `out` - The output stream to write to.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the gem command execution fails.
fn execute_gem_command_with(
    gem_bin: &str,
    home_dir: &str,
    args: &[&str],
    out: &mut dyn Write,
) -> Result<(), MigratoryError> {
    let mut cmd = Command::new(gem_bin);
    let gem_home = format!("{}/gems", home_dir);

    cmd.env("GEM_HOME", &gem_home);
    cmd.env("GEM_PATH", &gem_home);
    cmd.args(args);

    let output = cmd
        .output()
        .map_err(|e| MigratoryError::Generic(format!("Failed to spawn gem {}: {}", args[0], e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MigratoryError::Generic(format!(
            "Plugin {} failed with status: {}. Error: {}",
            args[0], output.status, stderr
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    write!(out, "{}", stdout).map_err(|e| MigratoryError::Generic(e.to_string()))?;
    Ok(())
}

/// Resolves the `gem` binary name based on whether tests are running.
fn resolve_gem_bin(is_test: bool) -> &'static str {
    if is_test { "echo" } else { "gem" }
}

/// Executes the `plugin` command.
///
/// # Arguments
///
/// * `cmd` - The specific plugin subcommand to execute.
/// * `out` - The trait object output stream to write to.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the subcommand fails.
pub fn execute(cmd: &PluginCommands, out: &mut dyn Write) -> Result<(), MigratoryError> {
    let home_dir = resolve_vagrant_home(
        std::env::var("VAGRANT_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    );
    let gem_bin = resolve_gem_bin(cfg!(test));
    execute_with_config(cmd, out, &home_dir, gem_bin)
}

/// Executes the `plugin` command with explicit Vagrant home directory and `gem` binary path.
///
/// # Arguments
///
/// * `cmd` - The specific plugin subcommand to execute.
/// * `out` - The trait object output stream to write to.
/// * `home_dir` - Resolved Vagrant home directory.
/// * `gem_bin` - Executable name or path for `gem`.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the subcommand fails.
pub fn execute_with_config(
    cmd: &PluginCommands,
    out: &mut dyn Write,
    home_dir: &str,
    gem_bin: &str,
) -> Result<(), MigratoryError> {
    match cmd {
        PluginCommands::Expunge(args) => {
            writeln!(out, "Expunging plugins...")
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            let mut gem_args = vec!["uninstall", "--all", "-x", "-I"];
            if args.local {
                gem_args.push("--local");
            }
            if args.global_only {
                gem_args.push("--user-install");
            }
            execute_gem_command_with(gem_bin, home_dir, &gem_args, out)?;
        }
        PluginCommands::Install(args) => {
            if std::path::Path::new(&args.name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wasm"))
            {
                writeln!(out, "Installing Rust-native WASM plugin '{}'...", args.name)
                    .map_err(|e| MigratoryError::Generic(e.to_string()))?;
                let wasm_dir = std::path::PathBuf::from(home_dir).join("wasm");
                std::fs::create_dir_all(&wasm_dir)
                    .map_err(|e| MigratoryError::Generic(e.to_string()))?;
                let src = std::path::Path::new(&args.name);
                if src.exists() {
                    let file_name = src.file_name().unwrap_or_default();
                    std::fs::copy(src, wasm_dir.join(file_name))
                        .map_err(|e| MigratoryError::Generic(e.to_string()))?;
                } else {
                    return Err(MigratoryError::NotFound(args.name.clone()));
                }
            } else {
                writeln!(out, "Installing plugin '{}'...", args.name)
                    .map_err(|e| MigratoryError::Generic(e.to_string()))?;
                let mut gem_args = vec!["install", &args.name];
                if let Some(ver) = &args.plugin_version {
                    gem_args.push("-v");
                    gem_args.push(ver);
                }
                if let Some(src) = &args.plugin_source {
                    gem_args.push("--source");
                    gem_args.push(src);
                }
                if args.local {
                    gem_args.push("--local");
                }
                execute_gem_command_with(gem_bin, home_dir, &gem_args, out)?;
            }
        }
        PluginCommands::License(args) => {
            writeln!(
                out,
                "Installing license for plugin '{}' from '{}'...",
                args.name, args.license_file
            )
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

            let license_path = std::path::Path::new(&args.license_file);
            if !license_path.exists() {
                return Err(MigratoryError::NotFound(args.license_file.clone()));
            }

            let license_dir = std::path::PathBuf::from(home_dir)
                .join("license")
                .join(&args.name);
            let _ = std::fs::create_dir_all(&license_dir);
            let dest_file = license_dir.join("license.lic");
            std::fs::copy(license_path, &dest_file).map_err(MigratoryError::Io)?;
            writeln!(
                out,
                "License installed successfully for plugin '{}'.",
                args.name
            )
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        PluginCommands::List(args) => {
            writeln!(out, "Listing plugins...")
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            let mut gem_args = vec!["list"];
            if args.local {
                gem_args.push("--local");
            }
            execute_gem_command_with(gem_bin, home_dir, &gem_args, out)?;
        }
        PluginCommands::Repair(args) => {
            writeln!(out, "Repairing Ruby plugin environment...")
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            let mut gem_args = vec!["pristine", "--all"];
            if args.local {
                gem_args.push("--local");
            }
            execute_gem_command_with(gem_bin, home_dir, &gem_args, out)?;
        }
        PluginCommands::Uninstall(args) => {
            writeln!(out, "Uninstalling plugin '{}'...", args.name)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            let mut gem_args = vec!["uninstall", &args.name, "-x", "-I"];
            if args.local {
                gem_args.push("--local");
            }
            execute_gem_command_with(gem_bin, home_dir, &gem_args, out)?;
        }
        PluginCommands::Update(args) => {
            if let Some(name) = &args.name {
                writeln!(out, "Updating plugin '{}'...", name)
                    .map_err(|e| MigratoryError::Generic(e.to_string()))?;
                execute_gem_command_with(gem_bin, home_dir, &["update", name], out)?;
            } else {
                writeln!(out, "Updating all Ruby plugin dependencies...")
                    .map_err(|e| MigratoryError::Generic(e.to_string()))?;
                execute_gem_command_with(gem_bin, home_dir, &["update"], out)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::*;

    #[test]
    fn test_execute_plugin_expunge() {
        let mut out = Vec::new();
        let cmd = PluginCommands::Expunge(PluginExpungeArgs {
            force: false,
            reinstall: false,
            local: true,
            local_only: false,
            global_only: true,
        });
        let result = execute(&cmd, &mut out);
        assert!(result.is_ok());
        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("Expunging plugins..."));
        assert!(out_str.contains("--local"));
        assert!(out_str.contains("--user-install"));
    }

    #[test]
    fn test_execute_plugin_install() {
        let mut out = Vec::new();
        let cmd = PluginCommands::Install(PluginInstallArgs {
            name: "test".to_string(),
            plugin_source: None,
            plugin_version: None,
            local: false,
            plugin_clean_sources: false,
            entry_point: None,
            verbose: false,
        });
        let result = execute(&cmd, &mut out);
        assert!(result.is_ok());
        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("Installing plugin 'test'..."));
    }

    #[test]
    fn test_execute_plugin_license() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let license_file = dir.path().join("my-license.lic");
        std::fs::write(&license_file, "license")?;

        let mut out = Vec::new();
        let cmd = PluginCommands::License(PluginLicenseArgs {
            name: "test-plugin".to_string(),
            license_file: license_file.to_string_lossy().to_string(),
        });
        let result = execute_with_config(&cmd, &mut out, &dir.path().to_string_lossy(), "echo");
        assert!(result.is_ok());
        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("Installing license for plugin 'test-plugin'"));
        Ok(())
    }

    #[test]
    fn test_execute_plugin_license_real_success() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let src_license = dir.path().join("my-license.lic");
        std::fs::write(&src_license, "SAMPLE_LICENSE_DATA")?;

        let vagrant_home = dir.path().join("vagrant_home");

        let mut out = Vec::new();
        let cmd = PluginCommands::License(PluginLicenseArgs {
            name: "licensed-plugin".to_string(),
            license_file: src_license.to_string_lossy().to_string(),
        });
        let result = execute_with_config(&cmd, &mut out, &vagrant_home.to_string_lossy(), "echo");
        assert!(result.is_ok());

        let installed_file = vagrant_home
            .join("license")
            .join("licensed-plugin")
            .join("license.lic");
        assert!(installed_file.exists());
        let content = std::fs::read_to_string(installed_file)?;
        assert_eq!(content, "SAMPLE_LICENSE_DATA");
        Ok(())
    }

    #[test]
    fn test_execute_plugin_license_not_found() {
        let mut out = Vec::new();
        let cmd = PluginCommands::License(PluginLicenseArgs {
            name: "licensed-plugin".to_string(),
            license_file: "/nonexistent/path/to/missing.lic".to_string(),
        });
        let result = execute(&cmd, &mut out);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_plugin_list() {
        let mut out = Vec::new();
        let cmd = PluginCommands::List(PluginListArgs { local: true });
        let result = execute(&cmd, &mut out);
        assert!(result.is_ok());
        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("Listing plugins..."));
        assert!(out_str.contains("--local"));
    }

    #[test]
    fn test_execute_plugin_repair() {
        let mut out = Vec::new();
        let cmd = PluginCommands::Repair(PluginRepairArgs { local: true });
        let result = execute(&cmd, &mut out);
        assert!(result.is_ok());
        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("Repairing Ruby plugin environment..."));
        assert!(out_str.contains("--local"));
    }

    #[test]
    fn test_execute_plugin_uninstall() {
        let mut out = Vec::new();
        let cmd = PluginCommands::Uninstall(PluginUninstallArgs {
            name: "test-plugin".to_string(),
            local: true,
        });
        let result = execute(&cmd, &mut out);
        assert!(result.is_ok());
        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("Uninstalling plugin 'test-plugin'..."));
        assert!(out_str.contains("--local"));
    }

    #[test]
    fn test_execute_plugin_update() {
        let mut out = Vec::new();
        let cmd = PluginCommands::Update(PluginUpdateArgs {
            name: None,
            local: false,
        });
        let result = execute(&cmd, &mut out);
        assert!(result.is_ok());
        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("Updating all Ruby plugin dependencies..."));
    }

    #[test]
    fn test_execute_plugin_update_with_name() {
        let mut out = Vec::new();
        let cmd = PluginCommands::Update(PluginUpdateArgs {
            name: Some("test-plugin".to_string()),
            local: false,
        });
        let result = execute(&cmd, &mut out);
        assert!(result.is_ok());
        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("Updating plugin 'test-plugin'..."));
    }

    #[test]
    fn test_execute_plugin_install_with_options() {
        let mut out = Vec::new();
        let cmd = PluginCommands::Install(PluginInstallArgs {
            name: "test-plugin".to_string(),
            plugin_source: Some("https://example.com".to_string()),
            plugin_version: Some("1.2.3".to_string()),
            local: true,
            plugin_clean_sources: false,
            entry_point: None,
            verbose: false,
        });
        let result = execute(&cmd, &mut out);
        assert!(result.is_ok());
        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("Installing plugin 'test-plugin'..."));
        assert!(out_str.contains("-v"));
        assert!(out_str.contains("1.2.3"));
        assert!(out_str.contains("--source"));
        assert!(out_str.contains("https://example.com"));
    }

    struct FailWriter;
    impl std::io::Write for FailWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("write error"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("flush error"))
        }
    }

    struct FailOnLicenseInstalledWriter;
    impl std::io::Write for FailOnLicenseInstalledWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if String::from_utf8_lossy(buf).contains("License installed successfully") {
                Err(std::io::Error::other("write error"))
            } else {
                Ok(buf.len())
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_execute_plugin_license_second_write_fail() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let src_license = dir.path().join("my-license.lic");
        std::fs::write(&src_license, "SAMPLE_LICENSE_DATA")?;

        let mut out = FailOnLicenseInstalledWriter;
        assert!(out.flush().is_ok());
        let cmd = PluginCommands::License(PluginLicenseArgs {
            name: "licensed-plugin".to_string(),
            license_file: src_license.to_string_lossy().to_string(),
        });
        assert!(
            execute_with_config(&cmd, &mut out, &dir.path().to_string_lossy(), "echo").is_err()
        );
        Ok(())
    }

    #[test]
    fn test_execute_plugin_license_copy_failure() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let src_license = dir.path().join("my-license.lic");
        std::fs::write(&src_license, "SAMPLE_LICENSE_DATA")?;

        let vagrant_home = dir.path().join("vagrant_home");
        let license_dir = vagrant_home.join("license").join("bad-copy-plugin");
        std::fs::create_dir_all(&license_dir)?;
        // Create license.lic as a directory to make fs::copy fail
        std::fs::create_dir_all(license_dir.join("license.lic"))?;

        let mut out = Vec::new();
        let cmd = PluginCommands::License(PluginLicenseArgs {
            name: "bad-copy-plugin".to_string(),
            license_file: src_license.to_string_lossy().to_string(),
        });
        assert!(
            execute_with_config(&cmd, &mut out, &vagrant_home.to_string_lossy(), "echo").is_err()
        );
        Ok(())
    }

    #[test]
    fn test_resolve_vagrant_home() {
        assert_eq!(
            resolve_vagrant_home(Some("/custom/path"), None),
            "/custom/path"
        );
        assert_eq!(
            resolve_vagrant_home(None, Some("/home/user")),
            "/home/user/.vagrant.d"
        );
        assert_eq!(resolve_vagrant_home(None, None), ".vagrant.d");
    }

    #[test]
    fn test_execute_plugin_write_error() {
        let mut out = FailWriter;
        let cmd = PluginCommands::List(PluginListArgs { local: false });
        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_failwriter_flush() {
        use std::io::Write;
        let mut out = FailWriter;
        assert!(out.flush().is_err());
    }

    #[test]
    fn test_execute_gem_command_error() {
        let mut out = Vec::new();
        let result = execute_gem_command_with("false", "/tmp", &["list"], &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_gem_command_real() -> Result<(), MigratoryError> {
        let mut out = Vec::new();
        let _ = execute_gem_command_with("echo", "/tmp", &["--version"], &mut out);

        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join("test-plugin.wasm"), "dummy")?;

        let cmd = PluginCommands::Install(PluginInstallArgs {
            name: temp
                .path()
                .join("test-plugin.wasm")
                .to_string_lossy()
                .to_string(),
            plugin_source: None,
            plugin_version: None,
            local: false,
            plugin_clean_sources: false,
            entry_point: None,
            verbose: false,
        });
        assert!(
            execute_with_config(&cmd, &mut out, &temp.path().to_string_lossy(), "echo").is_ok()
        );

        let cmd2 = PluginCommands::License(PluginLicenseArgs {
            name: "test".to_string(),
            license_file: "NONEXISTENT_LICENSE".to_string(),
        });
        assert!(
            execute_with_config(&cmd2, &mut out, &temp.path().to_string_lossy(), "echo").is_err()
        );
        Ok(())
    }

    #[test]
    fn test_execute_gem_command_not_found() {
        let mut out = Vec::new();
        let args = vec!["bogus_arg"];
        let result =
            execute_gem_command_with("nonexistent_gem_binary_xyz", "/tmp", &args, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_gem_command_exit_status() {
        let mut out = Vec::new();
        let args = vec!["bogus_arg"];
        let result = execute_gem_command_with("false", "/tmp", &args, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_plugin_install_wasm_missing_file() {
        let args = PluginCommands::Install(PluginInstallArgs {
            name: "missing_plugin.wasm".to_string(),
            plugin_version: None,
            plugin_source: None,
            local: false,
            entry_point: None,
            plugin_clean_sources: false,
            verbose: false,
        });

        let mut out = Vec::new();
        let result = execute_with_config(&args, &mut out, "/tmp", "echo");
        assert!(result.is_err());
    }

    #[test]
    fn test_plugin_install_wasm_success() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let dummy_wasm = dir.path().join("dummy.wasm");
        std::fs::write(&dummy_wasm, b"dummy")?;

        let args = PluginCommands::Install(PluginInstallArgs {
            name: dummy_wasm.to_string_lossy().into_owned(),
            plugin_version: None,
            plugin_source: None,
            local: false,
            entry_point: None,
            plugin_clean_sources: false,
            verbose: false,
        });

        let mut out = Vec::new();
        let result = execute_with_config(&args, &mut out, &dir.path().to_string_lossy(), "echo");
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_plugin_update_specific() {
        let args = PluginCommands::Update(PluginUpdateArgs {
            name: Some("specific_plugin".to_string()),
            local: false,
        });

        let mut out = Vec::new();
        let result = execute_with_config(&args, &mut out, "/tmp", "false");
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_plugin_fail_writer_all_commands() {
        // Expunge
        let mut out = FailWriter;
        let cmd = PluginCommands::Expunge(PluginExpungeArgs {
            force: false,
            reinstall: false,
            local: false,
            local_only: false,
            global_only: false,
        });
        assert!(execute(&cmd, &mut out).is_err());

        // Install WASM
        let mut out = FailWriter;
        let cmd = PluginCommands::Install(PluginInstallArgs {
            name: "test.wasm".to_string(),
            plugin_source: None,
            plugin_version: None,
            local: false,
            plugin_clean_sources: false,
            entry_point: None,
            verbose: false,
        });
        assert!(execute(&cmd, &mut out).is_err());

        // Install Gem
        let mut out = FailWriter;
        let cmd = PluginCommands::Install(PluginInstallArgs {
            name: "test-gem".to_string(),
            plugin_source: None,
            plugin_version: None,
            local: false,
            plugin_clean_sources: false,
            entry_point: None,
            verbose: false,
        });
        assert!(execute(&cmd, &mut out).is_err());

        // License
        let mut out = FailWriter;
        let cmd = PluginCommands::License(PluginLicenseArgs {
            name: "test".to_string(),
            license_file: "LICENSE".to_string(),
        });
        assert!(execute(&cmd, &mut out).is_err());

        // Repair
        let mut out = FailWriter;
        let cmd = PluginCommands::Repair(PluginRepairArgs { local: false });
        assert!(execute(&cmd, &mut out).is_err());

        // Uninstall
        let mut out = FailWriter;
        let cmd = PluginCommands::Uninstall(PluginUninstallArgs {
            name: "test".to_string(),
            local: false,
        });
        assert!(execute(&cmd, &mut out).is_err());

        // Update with name
        let mut out = FailWriter;
        let cmd = PluginCommands::Update(PluginUpdateArgs {
            name: Some("test".to_string()),
            local: false,
        });
        assert!(execute(&cmd, &mut out).is_err());

        // Update without name
        let mut out = FailWriter;
        let cmd = PluginCommands::Update(PluginUpdateArgs {
            name: None,
            local: false,
        });
        assert!(execute(&cmd, &mut out).is_err());

        // execute_gem_command_with with FailWriter
        let mut out = FailWriter;
        assert!(execute_gem_command_with("echo", "/tmp", &["list"], &mut out).is_err());
    }

    #[test]
    fn test_execute_plugin_all_branches() {
        assert_eq!(resolve_gem_bin(true), "echo");
        assert_eq!(resolve_gem_bin(false), "gem");
        let mut out = Vec::new();

        assert!(
            execute(
                &PluginCommands::Expunge(crate::cli::PluginExpungeArgs {
                    force: true,
                    local: true,
                    global_only: true,
                    local_only: true,
                    reinstall: true,
                }),
                &mut out,
            )
            .is_ok()
        );
        assert!(
            execute(
                &PluginCommands::Install(crate::cli::PluginInstallArgs {
                    name: "foo".to_string(),
                    local: true,
                    plugin_source: Some("url".to_string()),
                    plugin_version: Some("1.0".to_string()),
                    plugin_clean_sources: false,
                    entry_point: None,
                    verbose: false,
                }),
                &mut out,
            )
            .is_ok()
        );
        assert!(
            execute(
                &PluginCommands::List(crate::cli::PluginListArgs { local: false }),
                &mut out,
            )
            .is_ok()
        );
        assert!(
            execute(
                &PluginCommands::Repair(crate::cli::PluginRepairArgs { local: true }),
                &mut out,
            )
            .is_ok()
        );
        assert!(
            execute(
                &PluginCommands::Uninstall(crate::cli::PluginUninstallArgs {
                    name: "foo".to_string(),
                    local: true,
                }),
                &mut out,
            )
            .is_ok()
        );
        assert!(
            execute(
                &PluginCommands::Update(crate::cli::PluginUpdateArgs {
                    name: None,
                    local: false,
                }),
                &mut out,
            )
            .is_ok()
        );
    }

    #[test]
    fn test_plugin_install_wasm_fs_errors() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let blocked_file = dir.path().join("not_a_dir");
        std::fs::write(&blocked_file, b"content")?;

        let dummy_wasm = dir.path().join("test.wasm");
        std::fs::write(&dummy_wasm, b"wasm content")?;

        let args = PluginCommands::Install(PluginInstallArgs {
            name: dummy_wasm.to_string_lossy().into_owned(),
            plugin_version: None,
            plugin_source: None,
            local: false,
            entry_point: None,
            plugin_clean_sources: false,
            verbose: false,
        });

        // 1. Test create_dir_all failure by passing a file path as home_dir
        let mut out = Vec::new();
        assert!(
            execute_with_config(&args, &mut out, &blocked_file.to_string_lossy(), "echo").is_err()
        );

        // 2. Test copy failure by making destination a directory
        let home_dir = dir.path().join("home");
        let wasm_dir = home_dir.join("wasm");
        std::fs::create_dir_all(&wasm_dir)?;
        std::fs::create_dir_all(wasm_dir.join("test.wasm"))?;

        let mut out2 = Vec::new();
        assert!(
            execute_with_config(&args, &mut out2, &home_dir.to_string_lossy(), "echo").is_err()
        );
        Ok(())
    }

    #[test]
    fn test_execute_plugin_gem_command_errors() {
        let mut out = Vec::new();

        // Expunge error
        let cmd = PluginCommands::Expunge(PluginExpungeArgs {
            force: false,
            reinstall: false,
            local: false,
            local_only: false,
            global_only: false,
        });
        assert!(execute_with_config(&cmd, &mut out, "/tmp", "false").is_err());

        // Install gem error
        let cmd = PluginCommands::Install(PluginInstallArgs {
            name: "test-plugin".to_string(),
            plugin_source: None,
            plugin_version: None,
            local: false,
            plugin_clean_sources: false,
            entry_point: None,
            verbose: false,
        });
        assert!(execute_with_config(&cmd, &mut out, "/tmp", "false").is_err());

        // List error
        let cmd = PluginCommands::List(PluginListArgs { local: false });
        assert!(execute_with_config(&cmd, &mut out, "/tmp", "false").is_err());

        // Repair error
        let cmd = PluginCommands::Repair(PluginRepairArgs { local: false });
        assert!(execute_with_config(&cmd, &mut out, "/tmp", "false").is_err());

        // Uninstall error
        let cmd = PluginCommands::Uninstall(PluginUninstallArgs {
            name: "test".to_string(),
            local: false,
        });
        assert!(execute_with_config(&cmd, &mut out, "/tmp", "false").is_err());

        // Update with name error
        let cmd = PluginCommands::Update(PluginUpdateArgs {
            name: Some("test".to_string()),
            local: false,
        });
        assert!(execute_with_config(&cmd, &mut out, "/tmp", "false").is_err());

        // Update without name error
        let cmd = PluginCommands::Update(PluginUpdateArgs {
            name: None,
            local: false,
        });
        assert!(execute_with_config(&cmd, &mut out, "/tmp", "false").is_err());
    }
}
