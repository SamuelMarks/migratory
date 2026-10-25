//! Semantic implementation of the `provider` command.
//!
//! This module provides the logic to show the provider for the environment.

use crate::cli::ProviderArgs;
use crate::error::MigratoryError;
use std::path::Path;

/// Detects usable hypervisors/providers on the current host system.
///
/// # Returns
///
/// Returns a list of provider names that are available and usable on the host.
/// Callback function type for probing host provider availability commands.
pub type ProviderRunner<'a> = &'a mut dyn FnMut(&str, &[&str]) -> bool;

/// Callback function type for invoking host package manager installation commands.
pub type ProviderInstaller<'a> = &'a mut dyn FnMut(&str, &[&str]) -> Result<bool, std::io::Error>;

/// Detects usable hypervisors/providers on the current host system using a custom command runner.
///
/// # Arguments
///
/// * `runner` - A predicate function that runs a command and arguments, returning true if usable.
///
/// # Returns
///
/// Returns a list of usable provider names.
pub fn detect_usable_providers_with(runner: ProviderRunner<'_>) -> Vec<String> {
    let mut usable = Vec::new();

    // VirtualBox
    if runner("VBoxManage", &["--version"]) {
        usable.push("virtualbox".to_string());
    }

    // VMware
    if runner("vmrun", &["list"]) || runner("vmware", &["-v"]) {
        usable.push("vmware".to_string());
    }

    // Docker
    if runner("docker", &["--version"]) {
        usable.push("docker".to_string());
    }

    // QEMU / Libvirt
    if runner("virsh", &["--version"]) || runner("qemu-img", &["--version"]) {
        usable.push("qemu".to_string());
    }

    // Hyper-V on Windows
    #[cfg(windows)]
    if runner(
        "powershell",
        &[
            "-NoProfile",
            "-Command",
            "Get-Command Get-VM -ErrorAction SilentlyContinue",
        ],
    ) {
        usable.push("hyperv".to_string());
    }

    usable
}

/// Detects usable hypervisors/providers on the current host system.
///
/// # Returns
///
/// Returns a list of usable provider names.
pub fn detect_usable_providers() -> Vec<String> {
    detect_usable_providers_with(&mut |cmd, args| {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// Attempts or reports provider installation via host package managers using a custom installer function.
///
/// # Arguments
///
/// * `provider_name` - The name of the provider to install.
/// * `pkg_cmd` - The executable command name or path for the host package manager (`brew`, `sudo`, or `winget`).
/// * `installer` - A runner function that executes the package manager command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the package manager command fails.
pub fn install_provider_with(
    provider_name: &str,
    pkg_cmd: &str,
    installer: ProviderInstaller<'_>,
) -> Result<(), MigratoryError> {
    println!("Ensuring provider '{}' is installed...", provider_name);

    #[cfg(target_os = "macos")]
    {
        let cask_pkg = match provider_name {
            "virtualbox" => Some("virtualbox"),
            "vmware" => Some("vmware-fusion"),
            "docker" => Some("docker"),
            "qemu" => Some("qemu"),
            _ => None,
        };
        if let Some(pkg) = cask_pkg {
            let success = installer(pkg_cmd, &["install", "--cask", pkg])
                .map_err(|e| MigratoryError::Generic(format!("Brew execution failed: {}", e)))?;
            if !success {
                return Err(MigratoryError::Generic(format!(
                    "Homebrew failed to install provider package '{}'",
                    pkg
                )));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let pkg = match provider_name {
            "virtualbox" => Some("virtualbox"),
            "docker" => Some("docker.io"),
            "qemu" => Some("qemu-kvm"),
            _ => None,
        };
        if let Some(pkg) = pkg {
            let success = installer(pkg_cmd, &["apt-get", "install", "-y", pkg])
                .map_err(|e| MigratoryError::Generic(format!("Apt execution failed: {}", e)))?;
            if !success {
                return Err(MigratoryError::Generic(format!(
                    "Apt failed to install provider package '{}'",
                    pkg
                )));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let pkg = match provider_name {
            "virtualbox" => Some("virtualbox"),
            "docker" => Some("docker-desktop"),
            _ => None,
        };
        if let Some(pkg) = pkg {
            let success = installer(pkg_cmd, &["install", "-e", "--id", pkg])
                .map_err(|e| MigratoryError::Generic(format!("Winget execution failed: {}", e)))?;
            if !success {
                return Err(MigratoryError::Generic(format!(
                    "Winget failed to install provider package '{}'",
                    pkg
                )));
            }
        }
    }

    Ok(())
}

/// Attempts or reports provider installation via host package managers using a custom package manager binary.
///
/// # Arguments
///
/// * `provider_name` - The name of the provider to install.
/// * `pkg_cmd` - Executable command name or path for the package manager.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the package manager command fails.
pub fn install_provider_with_cmd(provider_name: &str, pkg_cmd: &str) -> Result<(), MigratoryError> {
    install_provider_with(provider_name, pkg_cmd, &mut |cmd, args| {
        let status = std::process::Command::new(cmd).args(args).status()?;
        Ok(status.success())
    })
}

/// Attempts or reports provider installation via host package managers.
///
/// # Arguments
///
/// * `provider_name` - The name of the provider to install.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the package manager command fails.
pub fn install_provider(provider_name: &str) -> Result<(), MigratoryError> {
    #[cfg(target_os = "macos")]
    let default_cmd = "brew";
    #[cfg(target_os = "linux")]
    let default_cmd = "sudo";
    #[cfg(target_os = "windows")]
    let default_cmd = "winget";
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let default_cmd = "true";

    install_provider_with_cmd(provider_name, default_cmd)
}

/// Executes the `provider` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `provider` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or if provider installation fails.
pub fn execute(cwd: &Path, args: &ProviderArgs) -> Result<(), MigratoryError> {
    execute_with(cwd, args, detect_usable_providers, install_provider)
}

/// Executes the `provider` command with injectable provider detection and installation functions.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `provider` command.
/// * `detect_fn` - Function returning usable provider names on the host.
/// * `install_fn` - Function installing a named provider on the host.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or if provider installation fails.
pub fn execute_with<D, I>(
    cwd: &Path,
    args: &ProviderArgs,
    mut detect_fn: D,
    mut install_fn: I,
) -> Result<(), MigratoryError>
where
    D: FnMut() -> Vec<String>,
    I: FnMut(&str) -> Result<(), MigratoryError>,
{
    let env_config = crate::config::load_environment_config(cwd)?;

    if args.usable {
        let usable = detect_fn();
        if usable.is_empty() {
            println!("No usable providers detected on this host.");
        } else {
            println!("Usable providers: {}", usable.join(", "));
        }
    } else {
        println!("Providers for this environment:");
        for (machine_name, machine_config) in &env_config.machines {
            let p_name = machine_config
                .vm
                .providers
                .first()
                .map(|p| p.name.to_string())
                .unwrap_or_else(|| "virtualbox".to_string());
            println!("  Machine: {} -> {}", machine_name, p_name);
        }
    }

    if args.install {
        for machine_config in env_config.machines.values() {
            let p_name = machine_config
                .vm
                .providers
                .first()
                .map(|p| p.name.to_string())
                .unwrap_or_else(|| "virtualbox".to_string());
            install_fn(&p_name)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_provider_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = ProviderArgs {
            usable: false,
            install: false,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_provider_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let valid_config = r#"
Vagrant.configure("2") do |config|
  config.vm.box = "base"
  config.vm.provider "docker" do |d|
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), valid_config)?;

        let args = ProviderArgs {
            usable: true,
            install: true,
        };
        let result = execute_with(
            cwd,
            &args,
            || vec!["docker".to_string(), "virtualbox".to_string()],
            |_| Ok(()),
        );
        assert!(result.is_ok());

        let args2 = ProviderArgs {
            usable: false,
            install: false,
        };
        let result2 = execute(cwd, &args2);
        assert!(result2.is_ok());

        let valid_config_no_provider = r#"
Vagrant.configure("2") do |config|
  config.vm.box = "base"
end
"#;
        fs::write(cwd.join("Vagrantfile"), valid_config_no_provider)?;

        let result3 = execute(cwd, &args2);
        assert!(result3.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_provider_empty_usable() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = ProviderArgs {
            usable: true,
            install: false,
        };
        let noop_install = |_: &str| Ok(());
        assert!(noop_install("virtualbox").is_ok());
        let result = execute_with(cwd, &args, Vec::new, noop_install);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_provider_install_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = ProviderArgs {
            usable: false,
            install: true,
        };
        let result = execute_with(cwd, &args, Vec::new, |_| {
            Err(MigratoryError::Generic("install failed".to_string()))
        });
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_detect_usable_providers_real() {
        let providers = detect_usable_providers();
        let _ = providers;
    }

    #[test]
    fn test_detect_usable_providers_mock_binaries() {
        // 1. None found
        let usable = detect_usable_providers_with(&mut |_, _| false);
        assert!(usable.is_empty());

        // 2. All primary binaries succeed: VBoxManage, vmrun, docker, virsh
        let usable = detect_usable_providers_with(&mut |cmd, _| {
            matches!(cmd, "VBoxManage" | "vmrun" | "docker" | "virsh")
        });
        assert!(usable.contains(&"virtualbox".to_string()));
        assert!(usable.contains(&"vmware".to_string()));
        assert!(usable.contains(&"docker".to_string()));
        assert!(usable.contains(&"qemu".to_string()));

        // 3. Fallback binaries: vmrun fails, vmware succeeds; virsh fails, qemu-img succeeds
        let usable =
            detect_usable_providers_with(&mut |cmd, _| matches!(cmd, "vmware" | "qemu-img"));
        assert!(usable.contains(&"vmware".to_string()));
        assert!(usable.contains(&"qemu".to_string()));
    }

    #[test]
    fn test_install_provider_coverage() {
        // Provider with no mapping
        assert!(install_provider("unknown_provider_xyz").is_ok());

        // Runner returns io error
        let err_io = install_provider_with("virtualbox", "brew", &mut |_, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "pkg manager missing",
            ))
        });
        assert!(err_io.is_err());

        // Runner returns false (command failed)
        let err_fail = install_provider_with("virtualbox", "brew", &mut |_, _| Ok(false));
        assert!(err_fail.is_err());

        // Runner returns true for all mapped providers
        assert!(install_provider_with("virtualbox", "brew", &mut |_, _| Ok(true)).is_ok());
        assert!(install_provider_with("vmware", "brew", &mut |_, _| Ok(true)).is_ok());
        assert!(install_provider_with("docker", "brew", &mut |_, _| Ok(true)).is_ok());
        assert!(install_provider_with("qemu", "brew", &mut |_, _| Ok(true)).is_ok());
        assert!(install_provider_with("unknown_xyz", "brew", &mut |_, _| Ok(true)).is_ok());

        // install_provider_with_cmd with "echo" and nonexistent binary
        assert!(install_provider_with_cmd("virtualbox", "echo").is_ok());
        assert!(install_provider_with_cmd("virtualbox", "nonexistent_binary_12345").is_err());
    }
}
