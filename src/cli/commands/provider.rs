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
    if let Ok(mock) = std::env::var("MIGRATORY_TEST_MOCK_USABLE_PROVIDERS") {
        if mock.trim().is_empty() {
            return Vec::new();
        }
        return mock.split(',').map(|s| s.trim().to_string()).collect();
    }

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
/// * `brew_cmd` - The executable command name or path for Homebrew.
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
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))] brew_cmd: &str,
    installer: ProviderInstaller<'_>,
) -> Result<(), MigratoryError> {
    println!("Ensuring provider '{}' is installed...", provider_name);

    if std::env::var("MIGRATORY_TEST_MOCK_INSTALL_ERROR").is_ok() {
        return Err(MigratoryError::Generic(format!(
            "Failed to install provider '{}'",
            provider_name
        )));
    }

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
            let success = installer(brew_cmd, &["install", "--cask", pkg])
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
            let cmd = std::env::var("MIGRATORY_TEST_LINUX_INSTALL_CMD")
                .unwrap_or_else(|_| "sudo".to_string());
            let success = installer(&cmd, &["apt-get", "install", "-y", pkg])
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
            let cmd = std::env::var("MIGRATORY_TEST_WINDOWS_INSTALL_CMD")
                .unwrap_or_else(|_| "winget".to_string());
            let success = installer(&cmd, &["install", "-e", "--id", pkg])
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
    if std::env::var("MIGRATORY_TEST_MOCK_INSTALL_SUCCESS").is_ok() {
        return Ok(());
    }
    let brew_cmd = std::env::var("MIGRATORY_TEST_BREW_CMD").unwrap_or_else(|_| "brew".to_string());
    install_provider_with(provider_name, &brew_cmd, &mut |cmd, args| {
        let status = std::process::Command::new(cmd).args(args).status()?;
        Ok(status.success())
    })
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
    let path = cwd.join("Vagrantfile");
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_string_lossy();
    let env_config = crate::config::evaluate_vagrantfile(&path_str).unwrap_or_default();

    if args.usable {
        let usable = detect_usable_providers();
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
                .map(|p| p.name.clone())
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
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "virtualbox".to_string());
            install_provider(&p_name)?;
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
    fn test_execute_provider_missing() {
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        let args = ProviderArgs {
            usable: false,
            install: false,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_provider_success() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();

        let valid_config = r#"
Vagrant.configure("2") do |config|
  config.vm.box = "base"
  config.vm.provider "docker" do |d|
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), valid_config).expect("write failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_USABLE_PROVIDERS", "docker, virtualbox");
            std::env::set_var("MIGRATORY_TEST_MOCK_INSTALL_SUCCESS", "1");
        }

        let args = ProviderArgs {
            usable: true,
            install: true,
        };
        let result = execute(cwd, &args);
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
        fs::write(cwd.join("Vagrantfile"), valid_config_no_provider).expect("write failed");

        let result3 = execute(cwd, &args2);
        assert!(result3.is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_USABLE_PROVIDERS");
            std::env::remove_var("MIGRATORY_TEST_MOCK_INSTALL_SUCCESS");
        }
    }

    #[test]
    fn test_execute_provider_empty_usable() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_USABLE_PROVIDERS", "");
        }

        let args = ProviderArgs {
            usable: true,
            install: false,
        };
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_USABLE_PROVIDERS");
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_provider_install_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("write failed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_INSTALL_ERROR", "1");
        }

        let args = ProviderArgs {
            usable: false,
            install: true,
        };
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_INSTALL_ERROR");
        }
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_usable_providers_real() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_USABLE_PROVIDERS");
        }
        let providers = detect_usable_providers();
        let _ = providers;
    }

    #[test]
    fn test_detect_usable_providers_mock_binaries() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_USABLE_PROVIDERS");
        }

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

        // Real detection execution test
        let _ = detect_usable_providers();
    }

    #[test]
    fn test_install_provider_coverage() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        // Provider with no mapping
        assert!(install_provider("unknown_provider_xyz").is_ok());

        #[cfg(target_os = "macos")]
        {
            // Runner returns io error
            let err_io = install_provider_with("virtualbox", "brew", &mut |_, _| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "brew missing",
                ))
            });
            assert!(err_io.is_err());

            // Runner returns false (brew command failed)
            let err_fail = install_provider_with("virtualbox", "brew", &mut |_, _| Ok(false));
            assert!(err_fail.is_err());

            // Runner returns true for all mapped providers
            assert!(install_provider_with("virtualbox", "brew", &mut |_, _| Ok(true)).is_ok());
            assert!(install_provider_with("vmware", "brew", &mut |_, _| Ok(true)).is_ok());
            assert!(install_provider_with("docker", "brew", &mut |_, _| Ok(true)).is_ok());
            assert!(install_provider_with("qemu", "brew", &mut |_, _| Ok(true)).is_ok());

            // Real install_provider with mock success env var
            unsafe {
                std::env::set_var("MIGRATORY_TEST_MOCK_INSTALL_SUCCESS", "1");
            }
            assert!(install_provider("virtualbox").is_ok());
            unsafe {
                std::env::remove_var("MIGRATORY_TEST_MOCK_INSTALL_SUCCESS");
            }

            // Real install_provider with custom brew command "true" (tests closure execution)
            unsafe {
                std::env::set_var("MIGRATORY_TEST_BREW_CMD", "true");
            }
            assert!(install_provider("virtualbox").is_ok());

            // Real install_provider with nonexistent binary (tests ? operator on status())
            unsafe {
                std::env::set_var("MIGRATORY_TEST_BREW_CMD", "nonexistent_binary_12345");
            }
            assert!(install_provider("virtualbox").is_err());
            unsafe {
                std::env::remove_var("MIGRATORY_TEST_BREW_CMD");
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Runner returns io error
            let err_io = install_provider_with("virtualbox", "", &mut |_, _| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "sudo missing",
                ))
            });
            assert!(err_io.is_err());

            // Runner returns false (apt command failed)
            let err_fail = install_provider_with("virtualbox", "", &mut |_, _| Ok(false));
            assert!(err_fail.is_err());

            // Runner returns true for all mapped providers
            assert!(install_provider_with("virtualbox", "", &mut |_, _| Ok(true)).is_ok());
            assert!(install_provider_with("docker", "", &mut |_, _| Ok(true)).is_ok());
            assert!(install_provider_with("qemu", "", &mut |_, _| Ok(true)).is_ok());

            // Runner returns true for unmapped provider
            assert!(install_provider_with("unknown_xyz", "", &mut |_, _| Ok(true)).is_ok());

            // Real install_provider with mock success env var
            unsafe {
                std::env::set_var("MIGRATORY_TEST_MOCK_INSTALL_SUCCESS", "1");
            }
            assert!(install_provider("virtualbox").is_ok());
            unsafe {
                std::env::remove_var("MIGRATORY_TEST_MOCK_INSTALL_SUCCESS");
            }

            // Real install_provider with custom linux command "true" (tests closure execution)
            unsafe {
                std::env::set_var("MIGRATORY_TEST_LINUX_INSTALL_CMD", "true");
            }
            assert!(install_provider("virtualbox").is_ok());

            // Real install_provider with nonexistent binary (tests ? operator on status())
            unsafe {
                std::env::set_var(
                    "MIGRATORY_TEST_LINUX_INSTALL_CMD",
                    "nonexistent_binary_12345",
                );
            }
            assert!(install_provider("virtualbox").is_err());
            unsafe {
                std::env::remove_var("MIGRATORY_TEST_LINUX_INSTALL_CMD");
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Runner returns io error
            let err_io = install_provider_with("virtualbox", "", &mut |_, _| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "winget missing",
                ))
            });
            assert!(err_io.is_err());

            // Runner returns false (winget command failed)
            let err_fail = install_provider_with("virtualbox", "", &mut |_, _| Ok(false));
            assert!(err_fail.is_err());

            // Runner returns true for all mapped providers
            assert!(install_provider_with("virtualbox", "", &mut |_, _| Ok(true)).is_ok());
            assert!(install_provider_with("docker", "", &mut |_, _| Ok(true)).is_ok());

            // Runner returns true for unmapped provider
            assert!(install_provider_with("unknown_xyz", "", &mut |_, _| Ok(true)).is_ok());

            // Real install_provider with mock success env var
            unsafe {
                std::env::set_var("MIGRATORY_TEST_MOCK_INSTALL_SUCCESS", "1");
            }
            assert!(install_provider("virtualbox").is_ok());
            unsafe {
                std::env::remove_var("MIGRATORY_TEST_MOCK_INSTALL_SUCCESS");
            }

            // Real install_provider with custom command
            unsafe {
                std::env::set_var("MIGRATORY_TEST_WINDOWS_INSTALL_CMD", "cmd.exe");
            }
            let _ = install_provider("virtualbox");

            // Real install_provider with nonexistent binary
            unsafe {
                std::env::set_var(
                    "MIGRATORY_TEST_WINDOWS_INSTALL_CMD",
                    "nonexistent_binary_12345",
                );
            }
            assert!(install_provider("virtualbox").is_err());
            unsafe {
                std::env::remove_var("MIGRATORY_TEST_WINDOWS_INSTALL_CMD");
            }
        }
    }
}
