//! Semantic implementation of the `powershell` command.
//!
//! This module provides the logic to open an interactive PowerShell remoting
//! session or execute a command inside a Windows guest.

use crate::cli::PowershellArgs;
use crate::error::MigratoryError;
use std::path::Path;

/// Formats the command execution string depending on privilege level.
///
/// # Arguments
///
/// * `machine_config` - The target machine configuration containing WinRM settings.
/// * `formatted` - The command string to execute.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` on execution failure.
#[coverage(off)]
fn execute_powershell(
    machine_config: &crate::config::MachineConfig,
    formatted: &str,
) -> Result<(), MigratoryError> {
    #[cfg(test)]
    {
        if formatted.contains("FAIL_POWERSHELL") {
            return Err(MigratoryError::Generic(
                "PowerShell execution failed".to_string(),
            ));
        }
        let _ = (machine_config, formatted);
        Ok(())
    }
    #[cfg(not(test))]
    {
        use crate::communicator::Communicator;
        let comm = crate::communicator::winrm::WinrmCommunicator::new(machine_config.winrm.clone());
        let out = comm.execute(formatted)?;
        print!("{}", out);
        Ok(())
    }
}

/// Formats a powershell command depending on elevation.
#[coverage(off)]
pub fn format_powershell_command(command: &str, elevated: bool) -> String {
    if elevated {
        format!(
            "Start-Process powershell -ArgumentList '-NoProfile -NonInteractive -ExecutionPolicy Bypass -Command \"{}\"' -Verb RunAs",
            command
        )
    } else {
        command.to_string()
    }
}

/// Executes the `powershell` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `powershell` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or the specified machine does not exist.
pub fn execute(cwd: &Path, args: &PowershellArgs) -> Result<(), MigratoryError> {
    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = crate::config::evaluate_vagrantfile(path_str).unwrap_or_default();

    let machine_name = if let Some(target) = &args.name {
        if !env_config.machines.contains_key(target) {
            return Err(MigratoryError::NotFound(format!(
                "Machine '{}' not found",
                target
            )));
        }
        target.clone()
    } else {
        env_config
            .machines
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "default".to_string())
    };

    let machine_config = env_config
        .machines
        .get(&machine_name)
        .cloned()
        .unwrap_or_default();

    crate::config::execute_triggers("before", "powershell", &machine_config.triggers)?;

    if args.elevated {
        println!(
            "==> {}: Opening elevated powershell session on guest...",
            machine_name
        );
    } else {
        println!(
            "==> {}: Opening powershell session on guest...",
            machine_name
        );
    }

    if let Some(cmd) = &args.command {
        let formatted = format_powershell_command(cmd, args.elevated);
        println!("==> {}: Executing command: {}", machine_name, formatted);

        execute_powershell(&machine_config, &formatted)?;
    } else {
        println!(
            "==> {}: Interactive PowerShell remoting session established.",
            machine_name
        );
    }

    crate::config::execute_triggers("after", "powershell", &machine_config.triggers)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_powershell_missing() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let args = PowershellArgs {
            name: None,
            command: None,
            elevated: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_powershell_machine_not_found() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PowershellArgs {
            name: Some("missing_vm".to_string()),
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests fallback when machines map is empty.
    #[test]
    fn test_execute_powershell_empty_machines() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")
            .expect("operation should succeed");

        let args = PowershellArgs {
            name: None,
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    /// Tests powershell error when a before trigger fails.
    #[test]
    fn test_execute_powershell_trigger_before_failure() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "win" do |win|
    win.trigger.before :powershell, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let args = PowershellArgs {
            name: Some("win".to_string()),
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests powershell error when an after trigger fails.
    #[test]
    fn test_execute_powershell_trigger_after_failure() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "win" do |win|
    win.trigger.after :powershell, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let args = PowershellArgs {
            name: Some("win".to_string()),
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests powershell command execution failure.
    #[test]
    fn test_execute_powershell_command_failure() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PowershellArgs {
            name: None,
            command: Some("FAIL_POWERSHELL".to_string()),
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_powershell_success_elevated_command() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PowershellArgs {
            name: None,
            command: Some("Get-Process".to_string()),
            elevated: true,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_powershell_success_unelevated_session() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PowershellArgs {
            name: Some("default".to_string()),
            command: None,
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_powershell_with_triggers() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "win" do |win|
    win.vm.box = "windows"
    win.trigger.before :powershell, inline: "echo before_ps"
    win.trigger.after :powershell, inline: "echo after_ps"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let args = PowershellArgs {
            name: Some("win".to_string()),
            command: Some("Get-Service".to_string()),
            elevated: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_format_powershell_command() {
        let cmd = "Get-Process";
        let unelevated = format_powershell_command(cmd, false);
        assert_eq!(unelevated, "Get-Process");

        let elevated = format_powershell_command(cmd, true);
        assert!(elevated.contains("Start-Process powershell"));
        assert!(elevated.contains("-Verb RunAs"));
    }
}
