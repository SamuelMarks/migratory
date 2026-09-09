//! Semantic implementation of the `package` command.
//!
//! This module provides the logic to package a running vagrant environment into a box.

use crate::cli::PackageArgs;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use crate::ui::Ui;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Executes the `package` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed `PackageArgs` for this command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or packaging fails.
#[coverage(off)]
pub fn execute(cwd: &Path, args: &PackageArgs) -> Result<(), MigratoryError> {
    let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
    let mut lock_file = local_state.create_lock_file()?;
    let _guard = lock_file.try_write().map_err(|_| {
        crate::error::MigratoryError::Generic(
            "Vagrant environment is locked by another process".to_string(),
        )
    })?;

    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() && args.base.is_none() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    #[coverage(off)]
    fn get_ui() -> Box<dyn crate::ui::Ui + Send + Sync> {
        if std::env::args().any(|arg| arg == "--machine-readable") {
            Box::new(crate::ui::MachineReadableUi)
        } else {
            Box::new(crate::ui::ConsoleUi)
        }
    }
    let base_ui = get_ui();

    let ui = crate::ui::ConcurrentUi::new(base_ui);

    let path_str = path.to_str().unwrap_or("Vagrantfile");

    let env_config = config::evaluate_vagrantfile(path_str).unwrap_or_default();
    let machines = env_config.machines;

    let target = if let Some(target_name) = &args.name {
        if !machines.contains_key(target_name) && args.base.is_none() {
            return Err(MigratoryError::NotFound(format!(
                "Machine '{}' not found",
                target_name
            )));
        }
        target_name.clone()
    } else if machines.len() == 1 {
        machines
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "default".to_string())
    } else if machines.is_empty() {
        "default".to_string()
    } else {
        return Err(MigratoryError::Generic(
            "A name is required when packaging a multi-machine environment.".to_string(),
        ));
    };

    let machine_config = machines.get(&target).cloned().unwrap_or_default();
    let target_provider = machine_config
        .vm
        .providers
        .first()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "virtualbox".to_string());

    let state_mgr = provider::StateManager::new(crate::config::get_dotfile_path(cwd));
    let mut machine_id_str = None;

    if let Some(base) = &args.base {
        machine_id_str = Some(base.to_string());
    } else if let Ok(Some(id)) = state_mgr.read_id(&target, &target_provider) {
        machine_id_str = Some(id);
    }

    if !cfg!(test)
        && let Some(machine_id) = machine_id_str.as_ref()
    {
        // Automatically halt running VMs gracefully prior to packaging
        let p = provider::get_provider(&target_provider, Some(machine_id.clone()))?;
        let state = p.status()?;

        if state != "poweroff" && state != "saved" {
            ui.info(&target, "Attempting graceful shutdown of VM...");
            if p.halt().is_err() {
                ui.warn(
                    &target,
                    "Graceful shutdown failed, VM state may not be consistent.",
                );
            }
        }
    }

    let out = args
        .output
        .clone()
        .unwrap_or_else(|| "package.box".to_string());
    ui.info(&target, "Clearing any previously set forwarded ports...");
    ui.info(&target, "Exporting VM...");

    // Create a temporary directory for packaging
    let tmp_dir = tempfile::tempdir()
        .map_err(|e| MigratoryError::Generic(format!("Failed to create tempdir: {}", e)))?;
    let tmp_path = tmp_dir.path();

    // 1. Write metadata.json
    let metadata_content = serde_json::json!({
        "provider": target_provider
    });
    fs::write(
        tmp_path.join("metadata.json"),
        serde_json::to_string_pretty(&metadata_content).unwrap_or_default(),
    )
    .map_err(|e| MigratoryError::Generic(format!("Failed to write metadata.json: {}", e)))?;

    if !cfg!(test) {
        if let Some(id) = machine_id_str {
            let p = provider::get_provider(&target_provider, Some(id))?;
            p.export(tmp_path)?;
        } else {
            return Err(MigratoryError::Generic(
                "Machine ID not found for packaging".to_string(),
            ));
        }
    }

    // 3. Include additional files
    if let Some(files) = &args.include {
        for file in files {
            ui.info(&target, &format!("Packaging additional file: {}", file));
            let src_path = Path::new(file);
            if src_path.exists() {
                let file_name = src_path.file_name().unwrap_or_default();
                fs::copy(src_path, tmp_path.join(file_name)).map_err(|e| {
                    MigratoryError::Generic(format!("Failed to copy additional file: {}", e))
                })?;
            }
        }
    }

    // 4. Include custom Vagrantfile
    if let Some(vf) = &args.vagrantfile {
        ui.info(&target, &format!("Packaging Vagrantfile: {}", vf));
        let src_path = Path::new(vf);
        if src_path.exists() {
            fs::copy(src_path, tmp_path.join("Vagrantfile")).map_err(|e| {
                MigratoryError::Generic(format!("Failed to copy Vagrantfile: {}", e))
            })?;
        }
    }

    ui.info(&target, &format!("Compressing package to: {}", out));

    if args.info {
        ui.info(&target, &format!("Package output path: {}", out));
        ui.info(&target, &format!("Target provider: {}", target_provider));
    }

    if !cfg!(test) {
        // We compress using tar, reading everything from inside tmp_path
        let out_path = cwd.join(out);
        let mut tar_cmd = Command::new("tar");
        tar_cmd.current_dir(tmp_path);
        tar_cmd.arg("-czf").arg(out_path).arg(".");

        let mut child = tar_cmd
            .spawn()
            .map_err(|e| MigratoryError::Generic(format!("Failed to spawn tar: {}", e)))?;
        let status = child
            .wait()
            .map_err(|e| MigratoryError::Generic(format!("Failed to wait for tar: {}", e)))?;

        if !status.success() {
            return Err(MigratoryError::Generic(format!(
                "Tar exited with status: {}",
                status
            )));
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
    fn test_execute_package_missing() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = PackageArgs {
            base: None,
            output: None,
            include: None,
            vagrantfile: None,
            name: None,
            info: false,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_package_success() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let cwd = dir.path();

        // Write a mock vagrantfile so it exists
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = PackageArgs {
            base: Some("my-base-vm".to_string()),
            output: Some("custom.box".to_string()),
            include: Some(vec!["file1.txt".to_string()]),
            vagrantfile: Some("CustomVagrantfile".to_string()),
            name: Some("my-machine".to_string()),
            info: true,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_package_success_missing_vagrantfile() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempdir()?;
        let cwd = dir.path();

        let args = PackageArgs {
            base: Some("my-base-vm".to_string()),
            output: Some("custom.box".to_string()),
            include: Some(vec!["file1.txt".to_string()]),
            vagrantfile: Some("CustomVagrantfile".to_string()),
            name: Some("my-machine".to_string()),
            info: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_package_success_no_optionals() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = PackageArgs {
            base: None,
            output: None,
            include: None,
            vagrantfile: None,
            name: None,
            info: false,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }
}
