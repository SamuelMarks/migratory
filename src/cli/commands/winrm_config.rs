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
    let env_config = crate::config::load_environment_config(cwd)?;

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

    let target_provider_name = if let Some(p) = machine_config.vm.providers.first() {
        p.name.as_str()
    } else {
        "virtualbox"
    };
    let state_mgr =
        crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(cwd));
    let machine_state = state_mgr.machine_state(&machine_name, target_provider_name);
    let persisted_ip = machine_state.get_guest_ip().ok().flatten();

    let host_val = if let Some(host) = &args.host {
        host.as_str()
    } else if let Some(ref ip) = persisted_ip {
        ip.as_str()
    } else {
        machine_config.winrm.host.as_str()
    };

    let effective_port = if let Ok(Some(persisted)) =
        machine_state.get_forwarded_host_port(machine_config.winrm.port)
    {
        persisted
    } else if host_val == "127.0.0.1" {
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
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_winrm_config_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_winrm_config_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

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
        Ok(())
    }

    #[test]
    fn test_execute_winrm_config_no_host() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_winrm_config_with_password() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let ruby = r#"
Vagrant.configure("2") do |config|
  config.winrm.password = "secret"
  config.winrm.transport = "plaintext"
end
"#;
        fs::write(cwd.join("Vagrantfile"), ruby)?;

        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_winrm_config_empty_machines() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "")?;

        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_winrm_config_forwarded_port() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let ruby = r#"
Vagrant.configure("2") do |config|
  config.vm.network "forwarded_port", guest: 80, host: 8080
  config.vm.network "forwarded_port", guest: 5985, host: 55985, id: "winrm"
end
"#;
        fs::write(cwd.join("Vagrantfile"), ruby)?;

        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_winrm_config_remote_host() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let ruby = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "virtualbox"
  config.winrm.host = "10.0.0.5"
  config.winrm.port = 5986
end
"#;
        fs::write(cwd.join("Vagrantfile"), ruby)?;

        let args = WinrmConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        // Test persisted guest IP and persisted forwarded port
        let state_mgr = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let ms = state_mgr.machine_state("default", "virtualbox");
        ms.set_guest_ip("10.0.0.77")?;
        ms.set_forwarded_ports(&[crate::state::local::PersistedForwardedPort {
            guest: 5986,
            host: 55986,
            protocol: "tcp".to_string(),
            host_ip: None,
        }])?;
        assert!(execute(cwd, &args).is_ok());
        Ok(())
    }
}
