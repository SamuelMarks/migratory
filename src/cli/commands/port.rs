//! Semantic implementation of the `port` command.
//!
//! This module provides the logic to output the guest port bindings.

use crate::cli::PortArgs;
use crate::error::MigratoryError;
use std::path::Path;

/// Executes the `port` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `port` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found.
pub fn execute(cwd: &Path, args: &PortArgs) -> Result<(), MigratoryError> {
    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = crate::config::evaluate_vagrantfile(path_str).unwrap_or_default();

    if let Some(target_name) = &args.name
        && !env_config.machines.contains_key(target_name)
    {
        return Err(MigratoryError::NotFound(format!(
            "Machine '{}' not found",
            target_name
        )));
    }

    if let Some(guest_port) = args.guest {
        println!("Looking up host port for guest port {}", guest_port);
    } else {
        println!("The forwarded ports for the machine are listed below. Please note that");
        println!("these values may differ from values configured in the Vagrantfile if the");
        println!("provider supports automatic port collision resolution and the port was in use.");
    }

    for (m_name, machine) in &env_config.machines {
        if let Some(target_name) = &args.name
            && m_name != target_name
        {
            continue;
        }
        for net in &machine.vm.networks {
            if let crate::config::NetworkConfig::ForwardedPort {
                guest,
                host,
                protocol,
                ..
            } = net
            {
                if let Some(target_guest) = args.guest
                    && *guest != target_guest
                {
                    continue;
                }
                println!(
                    "{} ({}) -> {}",
                    guest,
                    protocol.as_deref().unwrap_or("tcp"),
                    host
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_port_missing() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let args = PortArgs {
            name: None,
            guest: None,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_port_success() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PortArgs {
            name: None,
            guest: Some(80),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_port_success_no_guest() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = PortArgs {
            name: None,
            guest: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        let args_target = PortArgs {
            name: Some("default".to_string()),
            guest: None,
        };
        assert!(execute(cwd, &args_target).is_ok());

        let args_missing = PortArgs {
            name: Some("nonexistent".to_string()),
            guest: None,
        };
        assert!(matches!(
            execute(cwd, &args_missing),
            Err(MigratoryError::NotFound(_))
        ));
    }

    #[test]
    fn test_execute_port_with_networks() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let ruby = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web" do |web|
    web.vm.network "private_network", ip: "192.168.56.10"
    web.vm.network "forwarded_port", guest: 80, host: 8080
    web.vm.network "forwarded_port", guest: 443, host: 8443, protocol: "udp"
  end
  config.vm.define "db" do |db|
    db.vm.network "forwarded_port", guest: 5432, host: 5432
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), ruby).expect("write failed");

        // Test web with specific guest port
        let args = PortArgs {
            name: Some("web".to_string()),
            guest: Some(80),
        };
        assert!(execute(cwd, &args).is_ok());

        // Test all ports
        let args_all = PortArgs {
            name: None,
            guest: None,
        };
        assert!(execute(cwd, &args_all).is_ok());

        // Test db
        let args_db = PortArgs {
            name: Some("db".to_string()),
            guest: None,
        };
        assert!(execute(cwd, &args_db).is_ok());
    }
}
