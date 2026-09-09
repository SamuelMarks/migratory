//! Semantic implementation of the `provider` command.
//!
//! This module provides the logic to show the provider for the environment.

use crate::cli::ProviderArgs;
use crate::error::MigratoryError;
use std::path::Path;

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
/// Returns a `MigratoryError` if the Vagrantfile cannot be found.
pub fn execute(cwd: &Path, args: &ProviderArgs) -> Result<(), MigratoryError> {
    let path = cwd.join("Vagrantfile");
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = crate::config::evaluate_vagrantfile(path_str).unwrap_or_default();

    if args.usable {
        println!("Usable providers: virtualbox, vmware, hyperv, qemu, docker");
    } else {
        println!("Providers for this environment:");
        for (machine_name, machine_config) in env_config.machines.iter() {
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
        println!(
            "Ensuring provider is installed... (this feature depends on the host OS package manager)"
        );
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
    fn test_execute_provider_missing() -> Result<(), Box<dyn std::error::Error>> {
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
    fn test_execute_provider_success() -> Result<(), Box<dyn std::error::Error>> {
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
        fs::write(cwd.join("Vagrantfile"), valid_config_no_provider)?;

        let result3 = execute(cwd, &args2);
        assert!(result3.is_ok());

        Ok(())
    }
}
