//! Semantic implementation of the `ssh-config` command.
//!
//! This module provides the logic to output an OpenSSH valid configuration
//! string for the machine.

use crate::cli::SshConfigArgs;
use crate::error::MigratoryError;
use crate::state::local::LocalStateManager;
use std::path::Path;

/// Generates a new SSH key pair, assuming insertion into the guest.
///
/// In Vagrant, `vagrant ssh-config` doesn't just output config, it triggers
/// an initialization of keys if not present (replacing insecure default keys).
/// This function mocks that behavior.
fn ensure_keys(cwd: &Path, machine_name: &str) -> Result<String, MigratoryError> {
    let key_dir = cwd
        .join(".vagrant")
        .join("machines")
        .join(machine_name)
        .join("ssh");
    std::fs::create_dir_all(&key_dir)?;

    let priv_key_path = key_dir.join("private_key");
    if !priv_key_path.exists() {
        // Here we would use ssh2 to generate an RSA key.
        // For demonstration we just write a dummy private key file.
        std::fs::write(&priv_key_path, "DUMMY PRIVATE KEY\n")?;
    }

    Ok(priv_key_path.to_string_lossy().to_string())
}

/// Executes the `ssh-config` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `ssh-config` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found.
pub fn execute(cwd: &Path, args: &SshConfigArgs) -> Result<(), MigratoryError> {
    execute_with_home(cwd, args, &crate::config::get_vagrant_home())
}

/// Executes the `ssh-config` command with an explicit Vagrant home directory.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `ssh-config` command.
/// * `vagrant_home` - Path to the Vagrant home directory.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or key generation fails.
pub fn execute_with_home(
    cwd: &Path,
    args: &SshConfigArgs,
    vagrant_home: &Path,
) -> Result<(), MigratoryError> {
    let env_config = crate::config::load_environment_config(cwd)?;
    let state_mgr = LocalStateManager::new(cwd.join(".vagrant"));

    let machines = env_config.machines;

    let target = args.name.as_deref().unwrap_or("default");

    if let Some(machine) = machines.get(target) {
        let target_provider = machine
            .vm
            .providers
            .first()
            .map(|p| p.name.to_string())
            .unwrap_or_else(|| "virtualbox".to_string());
        let machine_state = state_mgr.machine_state(target, &target_provider);
        let _ = machine_state.get_id()?;

        let target_guest_port = machine.ssh.guest_port.unwrap_or(22);
        let effective_host = machine_state
            .get_guest_ip()
            .ok()
            .flatten()
            .unwrap_or_else(|| machine.ssh.host.clone());
        let effective_port =
            if let Ok(Some(persisted)) = machine_state.get_forwarded_host_port(target_guest_port) {
                persisted
            } else if effective_host == "127.0.0.1" {
                machine
                    .vm
                    .networks
                    .iter()
                    .find_map(|net| match net {
                        crate::config::NetworkConfig::ForwardedPort { guest, host, .. }
                            if *guest == target_guest_port =>
                        {
                            Some(*host)
                        }
                        _ => None,
                    })
                    .unwrap_or(machine.ssh.port)
            } else {
                machine.ssh.port
            };

        let priv_key = if let Some(p) = &machine.ssh.private_key_path {
            p.to_string_lossy().to_string()
        } else {
            let provider_key = cwd
                .join(".vagrant")
                .join("machines")
                .join(target)
                .join(&target_provider)
                .join("private_key");
            if provider_key.exists() {
                provider_key.to_string_lossy().to_string()
            } else if !machine.ssh.insert_key {
                let insecure_key =
                    crate::communicator::ssh::ensure_insecure_private_key(vagrant_home)?;
                insecure_key.to_string_lossy().to_string()
            } else {
                ensure_keys(cwd, target)?
            }
        };

        let host_alias = args.host.as_deref().unwrap_or(target);

        // Output an OpenSSH-compatible configuration.
        println!("Host {}", host_alias);
        println!("  HostName {}", effective_host);
        println!("  User {}", machine.ssh.username);
        println!("  Port {}", effective_port);
        println!("  UserKnownHostsFile /dev/null");
        println!("  StrictHostKeyChecking no");
        println!("  PasswordAuthentication no");
        println!("  IdentityFile {}", priv_key);
        println!("  IdentitiesOnly yes");
        println!("  LogLevel FATAL");
    } else {
        return Err(MigratoryError::NotFound(format!(
            "Machine '{}' not found",
            target
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_ssh_config_read_id_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&state_dir)?;

        // create a directory instead of a file for `id` to cause io error when read_id attempts to read it
        fs::create_dir_all(state_dir.join("id"))?;

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_ssh_config_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = SshConfigArgs {
            name: None,
            host: None,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_ssh_config_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        let key_path = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("ssh")
            .join("private_key");
        assert!(key_path.exists());
        Ok(())
    }

    #[test]
    fn test_execute_ssh_config_machine_not_found() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = SshConfigArgs {
            name: Some("nonexistent_machine".to_string()),
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(
            matches!(result, Err(MigratoryError::NotFound(msg)) if msg == "Machine 'nonexistent_machine' not found")
        );
        Ok(())
    }

    #[test]
    fn test_execute_ssh_config_empty_config() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")?;

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        assert!(execute(cwd, &args).is_err());

        fs::write(cwd.join("Vagrantfile"), "# Empty Vagrantfile")?;
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_ssh_config_with_host() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let key_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("ssh");
        std::fs::create_dir_all(&key_dir)?;
        let priv_key_path = key_dir.join("private_key");
        std::fs::write(&priv_key_path, "EXISTING KEY\n")?;

        let args = SshConfigArgs {
            name: None,
            host: Some("custom_host".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_ensure_keys_existing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let key_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("ssh");
        std::fs::create_dir_all(&key_dir)?;
        let priv_key_path = key_dir.join("private_key");
        std::fs::write(&priv_key_path, "EXISTING KEY\n")?;

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        assert_eq!(std::fs::read_to_string(&priv_key_path)?, "EXISTING KEY\n");
        Ok(())
    }

    #[test]
    fn test_ensure_keys_create_dir_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let machines_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(machines_dir.join("virtualbox"))?;
        fs::write(machines_dir.join("virtualbox").join("id"), "valid-id")?;
        fs::write(machines_dir.join("ssh"), "not a dir")?;

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::Io(_))));
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_ensure_keys_write_error() -> Result<(), MigratoryError> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let machines_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(machines_dir.join("virtualbox"))?;
        fs::write(machines_dir.join("virtualbox").join("id"), "valid-id")?;

        let key_dir = machines_dir.join("ssh");
        fs::create_dir_all(&key_dir)?;
        fs::set_permissions(&key_dir, fs::Permissions::from_mode(0o555))?;

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        fs::set_permissions(&key_dir, fs::Permissions::from_mode(0o755))?;
        assert!(matches!(result, Err(MigratoryError::Io(_))));
        Ok(())
    }

    #[test]
    fn test_execute_ssh_config_custom_key_path() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.ssh.private_key_path = "/custom/path/key"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = SshConfigArgs {
            name: None,
            host: Some("custom-alias".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_ssh_config_with_provider_key() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "qemu"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let prov_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("qemu");
        fs::create_dir_all(&prov_dir)?;
        fs::write(prov_dir.join("id"), "qemu-id-1234")?;
        fs::write(prov_dir.join("private_key"), "QEMU PRIVATE KEY")?;

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_ssh_config_insert_key_false() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.ssh.insert_key = false
  config.vm.network "forwarded_port", guest: 80, host: 8080
  config.vm.network "forwarded_port", guest: 22, host: 2222, id: "ssh"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute_with_home(cwd, &args, &dir.path().join("vagrant_home"));
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_ssh_config_remote_host() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.ssh.host = "192.168.122.100"
  config.ssh.port = 22
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_ssh_config_insecure_key_err() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.ssh.insert_key = false
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;

        let blocker = dir.path().join("blocker_file");
        fs::write(&blocker, "blocking")?;

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute_with_home(cwd, &args, &blocker.join("sub"));
        assert!(result.is_err());

        // Also test when machine_state has a persisted forwarded port for ssh
        let state_mgr = LocalStateManager::new(cwd.join(".vagrant"));
        let ms = state_mgr.machine_state("default", "virtualbox");
        ms.set_forwarded_ports(&[crate::state::local::PersistedForwardedPort {
            guest: 22,
            host: 2299,
            protocol: "tcp".to_string(),
            host_ip: None,
        }])?;
        assert!(execute(cwd, &args).is_ok());
        Ok(())
    }
}
