//! Semantic implementation of the `ssh-config` command.
//!
//! This module provides the logic to output an OpenSSH valid configuration
//! string for the machine.

use crate::cli::SshConfigArgs;
use crate::error::MigratoryError;
use crate::provider;
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
    let path = cwd.join("Vagrantfile");
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = get_env_config(path_str);
    let state_mgr = provider::StateManager::new(cwd.join(".vagrant"));

    let machines = if env_config.machines.is_empty() {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "default".to_string(),
            crate::config::MachineConfig::default(),
        );
        m
    } else {
        env_config.machines
    };

    let target = args.name.as_deref().unwrap_or("default");

    if let Some(machine) = machines.get(target) {
        let _ = state_mgr.read_id(target, "virtualbox")?;

        let priv_key = if let Some(p) = &machine.ssh.private_key_path {
            p.clone()
        } else {
            ensure_keys(cwd, target)?
        };

        let host_alias = args.host.as_deref().unwrap_or(target);

        // Output an OpenSSH-compatible configuration.
        println!("Host {}", host_alias);
        println!("  HostName {}", machine.ssh.host);
        println!("  User {}", machine.ssh.username);
        println!("  Port {}", machine.ssh.port);
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

#[coverage(off)]
fn get_env_config(path_str: &str) -> crate::config::EnvironmentConfig {
    crate::config::evaluate_vagrantfile(path_str).unwrap_or_default()
}

#[cfg(test)]
#[coverage(off)]
mod tests {

    #[test]
    fn test_execute_ssh_config_read_id_error() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&state_dir).expect("operation should succeed");

        // create a directory instead of a file for `id` to cause io error when read_id attempts to read it
        fs::create_dir_all(state_dir.join("id")).expect("operation should succeed");

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_ssh_config_missing() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = SshConfigArgs {
            name: None,
            host: None,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_ssh_config_success() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        // ensure_keys should have created the dummy key
        let key_path = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("ssh")
            .join("private_key");
        assert!(key_path.exists());
    }

    #[test]
    fn test_execute_ssh_config_machine_not_found() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = SshConfigArgs {
            name: Some("nonexistent_machine".to_string()),
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(
            matches!(result, Err(MigratoryError::NotFound(msg)) if msg == "Machine 'nonexistent_machine' not found")
        );
    }

    #[test]
    fn test_execute_ssh_config_empty_config() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        // Write an invalid ruby script to fail parsing and return default empty config
        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")
            .expect("operation should succeed");

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_ssh_config_with_host() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let key_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("ssh");
        std::fs::create_dir_all(&key_dir).expect("operation should succeed");
        let priv_key_path = key_dir.join("private_key");
        std::fs::write(&priv_key_path, "EXISTING KEY\n").expect("operation should succeed");

        let args = SshConfigArgs {
            name: None,
            host: Some("custom_host".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ensure_keys_existing() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let key_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("ssh");
        std::fs::create_dir_all(&key_dir)?;
        let priv_key_path = key_dir.join("private_key");
        std::fs::write(&priv_key_path, "EXISTING KEY\n")?;

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

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
    fn test_ensure_keys_create_dir_error() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let machines_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(machines_dir.join("virtualbox")).expect("operation should succeed");
        fs::write(machines_dir.join("virtualbox").join("id"), "valid-id")
            .expect("operation should succeed");
        fs::write(machines_dir.join("ssh"), "not a dir").expect("operation should succeed");

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::Io(_))));
    }

    #[test]
    #[cfg(unix)]
    fn test_ensure_keys_write_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let machines_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(machines_dir.join("virtualbox")).expect("operation should succeed");
        fs::write(machines_dir.join("virtualbox").join("id"), "valid-id")
            .expect("operation should succeed");

        let key_dir = machines_dir.join("ssh");
        fs::create_dir_all(&key_dir).expect("operation should succeed");
        fs::set_permissions(&key_dir, fs::Permissions::from_mode(0o555))
            .expect("operation should succeed");

        let args = SshConfigArgs {
            name: None,
            host: None,
        };
        let result = execute(cwd, &args);
        fs::set_permissions(&key_dir, fs::Permissions::from_mode(0o755))
            .expect("operation should succeed");
        assert!(matches!(result, Err(MigratoryError::Io(_))));
    }

    #[test]
    fn test_execute_ssh_config_custom_key_path() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.ssh.private_key_path = "/custom/path/key"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = SshConfigArgs {
            name: None,
            host: Some("custom-alias".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
}
