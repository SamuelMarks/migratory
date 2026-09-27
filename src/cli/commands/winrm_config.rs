//! Semantic implementation of the `winrm-config` command.
//!
//! This module provides the logic to output the WinRM valid configuration.

use crate::cli::WinrmConfigArgs;
use crate::error::MigratoryError;
use std::path::Path;

/// Executes the `winrm-config` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `winrm-config` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found.
pub fn execute(cwd: &Path, args: &WinrmConfigArgs) -> Result<(), MigratoryError> {
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
            .unwrap_or("default".to_string())
    };

    let machine_config = env_config
        .machines
        .get(&machine_name)
        .cloned()
        .unwrap_or_default();

    let host_val = if let Some(host) = &args.host {
        host.as_str()
    } else {
        machine_config.winrm.host.as_str()
    };

    let effective_port = if host_val == "127.0.0.1" {
        machine_config
            .vm
            .networks
            .iter()
            .find_map(|net| match net {
                crate::config::NetworkConfig::ForwardedPort { guest, host, .. }
                    if *guest == machine_config.winrm.port =>
                {
                    Some(*host)
                }
                _ => None,
            })
            .unwrap_or(machine_config.winrm.port)
    } else {
        machine_config.winrm.port
    };

    println!("HostName {}", host_val);
    println!("Port {}", effective_port);
    println!("User {}", machine_config.winrm.username);
    let pass = machine_config
        .winrm
        .password
        .as_deref()
        .unwrap_or("vagrant");
    println!("Password {}", pass);
    println!(
        "Transport {}",
        machine_config.winrm.transport.as_deref().unwrap_or("ntlm")
    );
    println!("Certificates managed");
    Ok(())
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_winrm_config_missing() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_winrm_config_success() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = WinrmConfigArgs {
            name: None,
            host: Some("custom-host".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        let args_target = WinrmConfigArgs {
            name: Some("default".to_string()),
            host: None,
        };
        assert!(execute(cwd, &args_target).is_ok());

        let args_missing = WinrmConfigArgs {
            name: Some("nonexistent".to_string()),
            host: None,
        };
        assert!(matches!(
            execute(cwd, &args_missing),
            Err(MigratoryError::NotFound(_))
        ));
    }

    #[test]
    fn test_execute_winrm_config_no_host() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_winrm_config_with_password() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let ruby = r#"
Vagrant.configure("2") do |config|
  config.winrm.password = "secret"
  config.winrm.transport = "plaintext"
end
"#;
        fs::write(cwd.join("Vagrantfile"), ruby).expect("operation should succeed");

        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_winrm_config_empty_machines() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // An empty json or syntax that evaluates to 0 machines
        fs::write(cwd.join("Vagrantfile"), "").expect("operation should succeed");

        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_winrm_config_forwarded_port() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let ruby = r#"
Vagrant.configure("2") do |config|
  config.vm.network "forwarded_port", guest: 80, host: 8080
  config.vm.network "forwarded_port", guest: 5985, host: 55985, id: "winrm"
end
"#;
        fs::write(cwd.join("Vagrantfile"), ruby).expect("operation should succeed");

        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_winrm_config_remote_host() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let ruby = r#"
Vagrant.configure("2") do |config|
  config.winrm.host = "10.0.0.5"
  config.winrm.port = 5986
end
"#;
        fs::write(cwd.join("Vagrantfile"), ruby).expect("operation should succeed");

        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
}
