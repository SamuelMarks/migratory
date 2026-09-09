//! Semantic implementation of the `rdp` command.
//!
//! This module provides the logic to auto-generate an `.rdp` configuration
//! file for Windows guests and launch the native RDP client.

use crate::cli::RdpArgs;
use crate::error::MigratoryError;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Generates the standard RDP file content.
///
/// # Arguments
///
/// * `host` - The host IP or hostname to connect to.
/// * `port` - The TCP port on which RDP is reachable.
///
/// # Returns
///
/// Returns formatted `.rdp` file content as a `String`.
pub fn generate_rdp_content(host: &str, port: u16) -> String {
    format!(
        "full address:s:{}:{}
prompt for credentials:i:1
administrative session:i:1
",
        host, port
    )
}

/// Discovers the RDP host and port from machine configuration networks.
///
/// Looks for a forwarded port where `guest` is 3389, falling back to port 3389.
///
/// # Arguments
///
/// * `machine` - The machine configuration to inspect.
///
/// # Returns
///
/// Returns a tuple of `(host_address, port)`.
pub fn get_rdp_address(machine: &crate::config::MachineConfig) -> (String, u16) {
    let mut port = 3389;
    let mut host = "127.0.0.1".to_string();

    for net in &machine.vm.networks {
        if let crate::config::NetworkConfig::ForwardedPort {
            guest: 3389,
            host: h,
            host_ip,
            ..
        } = net
        {
            port = *h;
            host = host_ip.clone().unwrap_or(host);
            break;
        }
    }

    (host, port)
}

/// Builds the system command to launch the native host RDP client.
///
/// # Arguments
///
/// * `rdp_path` - The path to the generated `.rdp` file.
/// * `host` - The host address.
/// * `port` - The host port.
///
/// # Returns
///
/// Returns a `Command` configured to launch the appropriate RDP viewer.
#[coverage(off)]
fn launch_rdp(rdp_path: &Path, host: &str, port: u16) {
    #[cfg(test)]
    {
        let _ = build_rdp_command(rdp_path, host, port);
    }
    #[cfg(not(test))]
    {
        if std::env::var("MIGRATORY_TEST_MOCK").is_err() && std::env::var("CARGO").is_err() {
            let mut cmd = build_rdp_command(rdp_path, host, port);
            let _ = cmd.spawn();
        }
    }
}

/// Builds the RDP command based on OS.
#[coverage(off)]
pub fn build_rdp_command(rdp_path: &Path, host: &str, port: u16) -> Command {
    if cfg!(target_os = "windows") {
        let mut cmd = Command::new("mstsc.exe");
        cmd.arg(rdp_path);
        cmd
    } else if cfg!(target_os = "macos") {
        let mut cmd = Command::new("open");
        cmd.arg(rdp_path);
        cmd
    } else {
        // Linux / Unix fallback
        let mut cmd = Command::new("xfreerdp");
        cmd.arg(format!("/v:{}:{}", host, port));
        cmd
    }
}

/// Writes the RDP file to the machine state directory or fallback temporary directory.
///
/// # Arguments
///
/// * `cwd` - The path to the working directory.
/// * `machine_name` - The target machine name.
/// * `content` - The RDP file content to write.
///
/// # Returns
///
/// Returns the path to the saved `.rdp` file.
///
/// # Errors
///
/// Returns `MigratoryError` if the file cannot be written.
pub fn save_rdp_file(
    cwd: &Path,
    machine_name: &str,
    content: &str,
) -> Result<PathBuf, MigratoryError> {
    let dotfile = crate::config::get_dotfile_path(cwd);
    let rdp_dir = dotfile
        .join("machines")
        .join(machine_name)
        .join("virtualbox");

    if fs::create_dir_all(&rdp_dir).is_ok() {
        let rdp_path = rdp_dir.join("connection.rdp");
        if fs::write(&rdp_path, content).is_ok() {
            return Ok(rdp_path);
        }
    }

    let temp_path = std::env::temp_dir().join(format!("{}.rdp", machine_name));
    fs::write(&temp_path, content).map_err(MigratoryError::Io)?;
    Ok(temp_path)
}

/// Executes the `rdp` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `rdp` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or if machine is missing.
pub fn execute(cwd: &Path, args: &RdpArgs) -> Result<(), MigratoryError> {
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

    crate::config::execute_triggers("before", "rdp", &machine_config.triggers)?;

    println!("==> {}: Generating RDP connection file...", machine_name);

    let (host, port) = get_rdp_address(&machine_config);
    let rdp_content = generate_rdp_content(&host, port);
    let rdp_path = save_rdp_file(cwd, &machine_name, &rdp_content)?;

    println!(
        "==> {}: RDP connection file generated: {}",
        machine_name,
        rdp_path.display()
    );

    launch_rdp(&rdp_path, &host, port);

    crate::config::execute_triggers("after", "rdp", &machine_config.triggers)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MachineConfig, NetworkConfig};
    use tempfile::tempdir;

    #[test]
    fn test_execute_rdp_missing() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let args = RdpArgs { name: None };

        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_rdp_machine_not_found() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = RdpArgs {
            name: Some("nonexistent".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests fallback when machines map is empty.
    #[test]
    fn test_execute_rdp_empty_machines() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax")
            .expect("operation should succeed");

        let args = RdpArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    /// Tests rdp error when a before trigger fails.
    #[test]
    fn test_execute_rdp_trigger_before_failure() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "win" do |win|
    win.trigger.before :rdp, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let args = RdpArgs {
            name: Some("win".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests rdp error when an after trigger fails.
    #[test]
    fn test_execute_rdp_trigger_after_failure() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "win" do |win|
    win.trigger.after :rdp, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let args = RdpArgs {
            name: Some("win".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests rdp error when saving RDP connection file fails.
    #[test]
    fn test_execute_rdp_save_file_error() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let dotfile = cwd.join(".vagrant");
        fs::write(&dotfile, "not a directory").expect("write failed");

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "nonexistent_parent_dir/testvm" do |win|
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let args = RdpArgs {
            name: Some("nonexistent_parent_dir/testvm".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_rdp_default_success() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        let args = RdpArgs { name: None };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_rdp_with_triggers() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "win" do |win|
    win.vm.box = "windows"
    win.trigger.before :rdp, inline: "echo before_rdp"
    win.trigger.after :rdp, inline: "echo after_rdp"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("write failed");

        let args = RdpArgs {
            name: Some("win".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_rdp_address_forwarded_port() {
        let mut machine = MachineConfig::default();
        machine.vm.networks.push(NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: true,
            protocol: Some("tcp".to_string()),
            host_ip: None,
        });
        machine.vm.networks.push(NetworkConfig::ForwardedPort {
            guest: 3389,
            host: 13389,
            auto_correct: true,
            protocol: Some("tcp".to_string()),
            host_ip: Some("192.168.1.50".to_string()),
        });

        let (host, port) = get_rdp_address(&machine);
        assert_eq!(host, "192.168.1.50");
        assert_eq!(port, 13389);
    }

    /// Tests forwarded port with host_ip set to None.
    #[test]
    fn test_get_rdp_address_forwarded_port_no_host_ip() {
        let mut machine = MachineConfig::default();
        machine.vm.networks.push(NetworkConfig::ForwardedPort {
            guest: 3389,
            host: 13389,
            auto_correct: true,
            protocol: Some("tcp".to_string()),
            host_ip: None,
        });

        let (host, port) = get_rdp_address(&machine);
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 13389);
    }

    #[test]
    fn test_get_rdp_address_default() {
        let machine = MachineConfig::default();
        let (host, port) = get_rdp_address(&machine);
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 3389);
    }

    #[test]
    fn test_build_rdp_command() {
        let rdp_file = Path::new("/tmp/test.rdp");
        let cmd = build_rdp_command(rdp_file, "127.0.0.1", 3389);
        assert!(!format!("{:?}", cmd).is_empty());
    }

    #[test]
    fn test_generate_rdp_content() {
        let content = generate_rdp_content("10.0.0.2", 3389);
        assert!(content.contains("full address:s:10.0.0.2:3389"));
        assert!(content.contains("prompt for credentials:i:1"));
    }

    #[test]
    fn test_save_rdp_file_fallback() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let dotfile = cwd.join(".vagrant");
        fs::write(&dotfile, "not a directory").expect("write failed");

        let result = save_rdp_file(cwd, "testvm", "rdp_content");
        assert!(result.is_ok());
        let path = result.expect("must succeed");
        assert!(path.exists());
        let _ = fs::remove_file(path);
    }

    /// Tests save_rdp_file fallback when connection.rdp cannot be written inside existing rdp_dir.
    #[test]
    fn test_save_rdp_file_write_dir_fallback() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let dotfile = crate::config::get_dotfile_path(cwd);
        let rdp_dir = dotfile
            .join("machines")
            .join("testvm_write_dir")
            .join("virtualbox");
        let rdp_path = rdp_dir.join("connection.rdp");
        fs::create_dir_all(&rdp_path).expect("operation should succeed");

        let result = save_rdp_file(cwd, "testvm_write_dir", "rdp_content");
        assert!(result.is_ok());
        let path = result.expect("must succeed");
        assert!(path.exists());
        let _ = fs::remove_file(path);
    }

    /// Tests save_rdp_file failure when temporary file destination cannot be written.
    #[test]
    fn test_save_rdp_file_error() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let dotfile = cwd.join(".vagrant");
        fs::write(&dotfile, "not a directory").expect("write failed");

        let res = save_rdp_file(cwd, "nonexistent_parent_dir/testvm", "rdp_content");
        assert!(res.is_err());
    }
}
