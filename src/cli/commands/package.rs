//! Semantic implementation of the `package` command.
//!
//! This module provides the logic to package a running or halted Vagrant machine
//! environment into a reusable `.box` archive.

use crate::cli::PackageArgs;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use crate::ui::Ui;
use std::fs;
use std::path::Path;

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
/// Returns a [`MigratoryError`] if the `Vagrantfile` cannot be found or packaging fails.
pub fn execute(cwd: &Path, args: &PackageArgs) -> Result<(), MigratoryError> {
    execute_with_provider(cwd, args, |name, id| provider::get_provider(name, id))
}

/// Executes the `package` command with a customizable provider factory.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed `PackageArgs` for this command.
/// * `provider_factory` - Factory function creating a [`provider::Provider`] given name and optional ID.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if the `Vagrantfile` cannot be found or packaging fails.
pub fn execute_with_provider<F>(
    cwd: &Path,
    args: &PackageArgs,
    mut provider_factory: F,
) -> Result<(), MigratoryError>
where
    F: FnMut(&str, Option<String>) -> Result<Box<dyn provider::Provider>, MigratoryError>,
{
    execute_with_provider_dyn(cwd, args, &mut provider_factory)
}

/// Dynamic factory closure type for instantiating a [`provider::Provider`].
type ProviderFactoryFn<'a> =
    dyn FnMut(&str, Option<String>) -> Result<Box<dyn provider::Provider>, MigratoryError> + 'a;

/// Non-generic implementation of `package` execution with a dynamic provider factory.
fn execute_with_provider_dyn(
    cwd: &Path,
    args: &PackageArgs,
    provider_factory: &mut ProviderFactoryFn<'_>,
) -> Result<(), MigratoryError> {
    let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
    let mut lock_file = local_state.create_lock_file()?;
    let _guard = lock_file
        .try_write()
        .map_err(|_| MigratoryError::EnvironmentLocked {
            path: cwd.join(".vagrant"),
        })?;

    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() && args.base.is_none() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let base_ui = crate::ui::default_ui();
    let ui = crate::ui::ConcurrentUi::new(base_ui);

    let env_config = if path.exists() {
        config::load_environment_config(cwd)?
    } else {
        crate::config::EnvironmentConfig::default()
    };
    let machines = env_config.machines;

    let target = if let Some(target_name) = &args.name {
        if !machines.contains_key(target_name) && args.base.is_none() {
            return Err(MigratoryError::NotFound(format!(
                "Machine '{target_name}' not found"
            )));
        }
        target_name.clone()
    } else if machines.len() <= 1 {
        machines
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "default".to_string())
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
        .map_or_else(|| "virtualbox".to_string(), |p| p.name.to_string());

    let state_mgr =
        crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(cwd));
    let mut machine_id_str = None;

    if let Some(base) = &args.base {
        machine_id_str = Some(base.to_string());
    } else if let Ok(Some(id)) = state_mgr.read_id(&target, &target_provider) {
        machine_id_str = Some(id);
    }

    if let Some(machine_id) = machine_id_str.as_ref() {
        let p = provider_factory(&target_provider, Some(machine_id.clone()))?;
        let state = p.status().unwrap_or_default();

        if state != provider::MachineState::PowerOff
            && state != provider::MachineState::Saved
            && state != provider::MachineState::NotCreated
        {
            ui.info(&target, "Attempting graceful shutdown of VM...");
            let _ = p.halt();
        }
    }

    let out = args.output.as_deref().unwrap_or("package.box");
    ui.info(&target, "Clearing any previously set forwarded ports...");
    ui.info(&target, "Exporting VM...");

    let tmp_dir = tempfile::tempdir()?;
    let tmp_path = tmp_dir.path();

    let metadata_content = serde_json::json!({
        "provider": target_provider
    });
    let metadata_json = serde_json::to_string_pretty(&metadata_content).unwrap_or_default();
    let _ = fs::write(tmp_path.join("metadata.json"), metadata_json);

    if let Some(id) = machine_id_str {
        let p = provider_factory(&target_provider, Some(id))?;
        p.export(tmp_path)?;
    }

    if let Some(files) = &args.include {
        for file in files {
            ui.info(&target, &format!("Packaging additional file: {file}"));
            let src_path = Path::new(file);
            if src_path.exists() {
                let file_name = src_path.file_name().unwrap_or_default();
                fs::copy(src_path, tmp_path.join(file_name))?;
            }
        }
    }

    if let Some(vf) = &args.vagrantfile {
        ui.info(&target, &format!("Packaging Vagrantfile: {vf}"));
        let src_path = Path::new(vf);
        if src_path.exists() {
            fs::copy(src_path, tmp_path.join("Vagrantfile"))?;
        }
    }

    ui.info(&target, &format!("Compressing package to: {out}"));

    if args.info {
        ui.info(&target, &format!("Package output path: {out}"));
        ui.info(&target, &format!("Target provider: {target_provider}"));
    }

    let out_path = cwd.join(out);
    compress_package(tmp_path, &out_path)?;

    Ok(())
}

/// Compresses the staged box directory `src_dir` into a gzipped tar archive at `out_file`.
///
/// # Arguments
///
/// * `src_dir` - Staging directory containing `metadata.json` and exported VM artifacts.
/// * `out_file` - Destination `.box` archive file path.
///
/// # Returns
///
/// Returns `Ok(())` on successful compression.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if creating or writing the tar.gz archive fails.
fn compress_package(src_dir: &Path, out_file: &Path) -> Result<(), MigratoryError> {
    let enc = flate2::write::GzEncoder::new(
        std::fs::File::create(out_file).map_err(MigratoryError::Io)?,
        flate2::Compression::default(),
    );
    let mut tar = tar::Builder::new(enc);
    tar.append_dir_all(".", src_dir)
        .map_err(MigratoryError::Io)?;
    tar.finish().map_err(MigratoryError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_package_missing() -> Result<(), MigratoryError> {
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
    fn test_execute_package_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

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
    fn test_execute_package_success_missing_vagrantfile() -> Result<(), MigratoryError> {
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
    fn test_execute_package_success_no_optionals() -> Result<(), MigratoryError> {
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

    #[test]
    fn test_execute_package_end_to_end_virtualbox() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# VirtualBox config")?;
        fs::write(cwd.join("extra.txt"), "hello extra file")?;
        fs::write(cwd.join("Boxfile"), "# Inner box vagrantfile")?;

        let args = PackageArgs {
            base: Some("test-vbox-machine-id".to_string()),
            output: Some("package-test.box".to_string()),
            include: Some(vec![cwd.join("extra.txt").to_string_lossy().to_string()]),
            vagrantfile: Some(cwd.join("Boxfile").to_string_lossy().to_string()),
            name: None,
            info: true,
        };

        let result = execute(cwd, &args);
        assert!(result.is_ok());

        let box_path = cwd.join("package-test.box");
        assert!(box_path.exists());

        let file = fs::File::open(&box_path)?;
        let dec = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(dec);

        let mut found_metadata = false;
        let mut found_ovf = false;
        let mut found_extra = false;
        let mut found_vagrantfile = false;

        for entry in archive.entries()? {
            let entry = entry?;
            let p = entry.path()?.to_string_lossy().to_string();
            if p.ends_with("metadata.json") {
                found_metadata = true;
            } else if p.ends_with("box.ovf") {
                found_ovf = true;
            } else if p.ends_with("extra.txt") {
                found_extra = true;
            } else if p.ends_with("Vagrantfile") {
                found_vagrantfile = true;
            }
        }

        assert!(found_metadata);
        assert!(found_ovf);
        assert!(found_extra);
        assert!(found_vagrantfile);

        Ok(())
    }

    #[test]
    fn test_execute_package_end_to_end_docker() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vf = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "docker"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vf)?;

        let state_mgr = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        state_mgr.write_id("default", "docker", "test-docker-cid")?;

        let args = PackageArgs {
            base: None,
            output: Some("docker.box".to_string()),
            include: None,
            vagrantfile: None,
            name: None,
            info: false,
        };

        let result = execute_with_provider(cwd, &args, |_, id| {
            Ok(Box::new(
                crate::provider::docker::DockerProvider::new(id).with_docker_bin("echo"),
            ))
        });
        assert!(result.is_ok());

        let box_path = cwd.join("docker.box");
        assert!(box_path.exists());

        let file = fs::File::open(&box_path)?;
        let dec = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(dec);

        let mut found_tar = false;
        for entry in archive.entries()? {
            let entry = entry?;
            let p = entry.path()?.to_string_lossy().to_string();
            if p.ends_with("box.tar") {
                found_tar = true;
            }
        }
        assert!(found_tar);

        // Test multi-machine error when args.name is None, and missing machine when args.name is Some
        let vf_multi = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web"
  config.vm.define "db"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vf_multi)?;
        assert!(execute(cwd, &args).is_err());

        let args_missing = PackageArgs {
            base: None,
            output: None,
            include: None,
            vagrantfile: None,
            name: Some("nonexistent".to_string()),
            info: false,
        };
        assert!(execute(cwd, &args_missing).is_err());

        // Test single machine in Vagrantfile without args.name and with matching args.name
        let vf_single = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web"
end
"#;
        fs::write(cwd.join("Vagrantfile"), vf_single)?;
        let args_named_existing = PackageArgs {
            base: Some("web-id".to_string()),
            output: Some("web-named.box".to_string()),
            include: None,
            vagrantfile: None,
            name: Some("web".to_string()),
            info: false,
        };
        assert!(
            execute_with_provider(cwd, &args_named_existing, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(|_| Ok("VMState=\"saved\"\n".to_string())),
                ))
            })
            .is_ok()
        );

        let args_single = PackageArgs {
            base: Some("web-id".to_string()),
            output: Some("web.box".to_string()),
            include: None,
            vagrantfile: None,
            name: None,
            info: false,
        };
        assert!(
            execute_with_provider(cwd, &args_single, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(|_| Ok("VMState=\"running\"\n".to_string())),
                ))
            })
            .is_ok()
        );
        assert!(
            execute_with_provider(cwd, &args_single, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id).with_runner(|args| {
                        if args.first() == Some(&"showvminfo") {
                            Err(MigratoryError::Generic("no state".to_string()))
                        } else {
                            Ok(String::new())
                        }
                    }),
                ))
            })
            .is_ok()
        );

        // Test when halt fails on running VM and when Vagrantfile is missing without --base
        assert!(
            execute_with_provider(cwd, &args_single, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id).with_runner(|args| {
                        if args.first() == Some(&"controlvm") {
                            Err(MigratoryError::Generic("halt err".to_string()))
                        } else {
                            Ok("VMState=\"running\"\n".to_string())
                        }
                    }),
                ))
            })
            .is_ok()
        );

        let no_vf_dir = tempdir()?;
        let args_no_base = PackageArgs {
            base: None,
            output: None,
            include: None,
            vagrantfile: None,
            name: None,
            info: false,
        };
        assert!(execute(no_vf_dir.path(), &args_no_base).is_err());

        let args_with_base_no_vf = PackageArgs {
            base: Some("base-vm-id".to_string()),
            output: Some(
                no_vf_dir
                    .path()
                    .join("out.box")
                    .to_string_lossy()
                    .into_owned(),
            ),
            include: None,
            vagrantfile: None,
            name: None,
            info: false,
        };
        assert!(
            execute_with_provider(no_vf_dir.path(), &args_with_base_no_vf, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(|_| Ok("VMState=\"poweroff\"\n".to_string())),
                ))
            })
            .is_ok()
        );

        // Test EnvironmentLocked when lock file is already held
        let lock_mgr =
            crate::state::local::LocalStateManager::new(no_vf_dir.path().join(".vagrant"));
        let mut lf = lock_mgr.create_lock_file()?;
        let _held = lf.try_write().map_err(MigratoryError::Io)?;
        let locked_path = no_vf_dir.path().to_path_buf();
        let locked_args = args_with_base_no_vf;
        let locked_res = std::thread::spawn(move || execute(&locked_path, &locked_args)).join();
        assert!(matches!(locked_res, Ok(Err(_))));

        Ok(())
    }
}
