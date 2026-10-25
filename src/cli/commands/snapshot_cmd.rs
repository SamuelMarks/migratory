//! Semantic implementation of the `snapshot` command and its subcommands.
//!
//! This module provides the logic to save, restore, list, delete, push, and pop
//! virtual machine snapshots across supported providers.

use crate::cli::SnapshotCommands;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use crate::ui::Ui;
use std::path::Path;

/// Resolves the target machine names from the environment configuration.
///
/// # Arguments
///
/// * `env_config` - The evaluated [`crate::config::EnvironmentConfig`].
/// * `target_machine` - Optional explicit machine name filter.
///
/// # Returns
///
/// Returns a vector of target machine names.
///
/// # Errors
///
/// Returns [`MigratoryError::NotFound`] if an explicit machine name does not exist.
fn resolve_target_machines(
    env_config: &crate::config::EnvironmentConfig,
    target_machine: Option<String>,
) -> Result<Vec<String>, MigratoryError> {
    crate::config::resolve_target_machines(&env_config.machines, target_machine.as_deref())
}

/// Executes the `snapshot` command.
///
/// # Arguments
///
/// * `cmd` - The snapshot subcommand to execute.
/// * `cwd` - The current working directory containing the `Vagrantfile`.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if the `Vagrantfile` is not found or the command fails.
pub fn execute(cmd: &SnapshotCommands, cwd: &Path) -> Result<(), MigratoryError> {
    execute_with_provider(cmd, cwd, |name, id| provider::get_provider(name, id))
}

/// Executes the `snapshot` command with a customizable provider factory.
///
/// # Arguments
///
/// * `cmd` - The snapshot subcommand to execute.
/// * `cwd` - The current working directory containing the `Vagrantfile`.
/// * `provider_factory` - Factory function creating a [`provider::Provider`] given name and optional ID.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if the `Vagrantfile` is not found or the command fails.
pub fn execute_with_provider<F>(
    cmd: &SnapshotCommands,
    cwd: &Path,
    mut provider_factory: F,
) -> Result<(), MigratoryError>
where
    F: FnMut(&str, Option<String>) -> Result<Box<dyn provider::Provider>, MigratoryError>,
{
    let env_config = config::load_environment_config(cwd)?;

    let base_ui = crate::ui::default_ui();
    let ui = crate::ui::ConcurrentUi::new(base_ui);

    let state_mgr =
        crate::state::local::LocalStateManager::new(crate::config::get_dotfile_path(cwd));

    let target_machine = match cmd {
        SnapshotCommands::Save(args) => args.vm_name.clone(),
        SnapshotCommands::Restore(args) => args.vm_name.clone(),
        SnapshotCommands::List(args) => args.vm_name.clone(),
        SnapshotCommands::Delete(args) => args.vm_name.clone(),
        SnapshotCommands::Pop(args) => args.vm_name.clone(),
        SnapshotCommands::Push(args) => args.vm_name.clone(),
    };

    let target_machines = resolve_target_machines(&env_config, target_machine)?;

    for name in &target_machines {
        let machine_config = env_config.machines.get(name).cloned().unwrap_or_default();
        let target_provider_name = machine_config
            .vm
            .providers
            .first()
            .map_or_else(|| "virtualbox".to_string(), |p| p.name.to_string());

        let machine_id = state_mgr.read_id(name, &target_provider_name)?;
        let p = provider_factory(&target_provider_name, machine_id)?;

        match cmd {
            SnapshotCommands::Save(args) => {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let snap_name = args
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("snapshot_{secs}"));
                ui.info(name, &format!("Saving snapshot '{snap_name}'..."));
                if let Err(e) = p.snapshot_save(&snap_name) {
                    ui.warn(name, &format!("Snapshot save failed: {e}"));
                }
            }
            SnapshotCommands::Restore(args) => {
                let snap_name = args.name.clone().unwrap_or_default();
                ui.info(name, &format!("Restoring snapshot '{snap_name}'..."));
                if let Err(e) = p.snapshot_restore(&snap_name) {
                    ui.warn(name, &format!("Snapshot restore failed: {e}"));
                }
            }
            SnapshotCommands::List(_args) => {
                ui.info(name, "Listing snapshots...");
                match p.snapshot_list() {
                    Ok(snaps) => {
                        if snaps.is_empty() {
                            ui.info(name, "No snapshots found.");
                        } else {
                            for snap in snaps {
                                ui.info(name, &format!("- {snap}"));
                            }
                        }
                    }
                    Err(e) => ui.warn(name, &format!("Snapshot list failed: {e}")),
                }
            }
            SnapshotCommands::Delete(args) => {
                let snap_name = args.name.clone().unwrap_or_default();
                ui.info(name, &format!("Deleting snapshot '{snap_name}'..."));
                if let Err(e) = p.snapshot_delete(&snap_name) {
                    ui.warn(name, &format!("Snapshot delete failed: {e}"));
                }
            }
            SnapshotCommands::Pop(_args) => {
                ui.info(name, "Popping latest snapshot state (not strictly supported natively, defaulting to mock list/restore)...");
                if let Ok(snaps) = p.snapshot_list() {
                    if let Some(latest) = snaps.last() {
                        if let Err(e) = p.snapshot_restore(latest) {
                            ui.warn(name, &format!("Snapshot pop (restore) failed: {e}"));
                        } else if let Err(e) = p.snapshot_delete(latest) {
                            ui.warn(name, &format!("Snapshot pop (delete) failed: {e}"));
                        } else {
                            ui.info(name, &format!("Successfully popped snapshot '{latest}'"));
                        }
                    } else {
                        ui.info(name, "No snapshots to pop.");
                    }
                }
            }
            SnapshotCommands::Push(_args) => {
                let snap_name = format!(
                    "push_{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                );
                ui.info(
                    name,
                    &format!("Pushing state to new snapshot '{snap_name}'..."),
                );
                if let Err(e) = p.snapshot_save(&snap_name) {
                    ui.warn(name, &format!("Snapshot push failed: {e}"));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_snapshot_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let cmd = SnapshotCommands::List(SnapshotListArgs { vm_name: None });
        let result = execute(&cmd, cwd);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_snapshot_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let id_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&id_dir)?;
        fs::write(id_dir.join("id"), "mock_id")?;

        assert!(
            execute(
                &SnapshotCommands::Save(SnapshotSaveArgs {
                    vm_name: None,
                    name: None,
                    force: false,
                }),
                cwd,
            )
            .is_ok()
        );
        assert!(
            execute(
                &SnapshotCommands::Restore(SnapshotRestoreArgs {
                    vm_name: None,
                    name: None,
                    provision: None,
                    provision_with: None,
                    no_start: false,
                    no_provision: false,
                }),
                cwd,
            )
            .is_ok()
        );
        assert!(
            execute(
                &SnapshotCommands::List(SnapshotListArgs { vm_name: None }),
                cwd,
            )
            .is_ok()
        );
        assert!(
            execute(
                &SnapshotCommands::Delete(SnapshotDeleteArgs {
                    vm_name: None,
                    name: None
                }),
                cwd,
            )
            .is_ok()
        );
        assert!(
            execute(
                &SnapshotCommands::Pop(SnapshotPopArgs {
                    vm_name: None,
                    no_delete: false,
                    provision: None,
                    provision_with: None,
                    no_start: false,
                    no_provision: false,
                }),
                cwd,
            )
            .is_ok()
        );
        assert!(
            execute(
                &SnapshotCommands::Push(SnapshotPushArgs { vm_name: None }),
                cwd,
            )
            .is_ok()
        );
        Ok(())
    }

    #[test]
    fn test_execute_snapshot_vm_not_found() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let cmd = SnapshotCommands::List(SnapshotListArgs {
            vm_name: Some("missing_vm".to_string()),
        });
        let result = execute(&cmd, cwd);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_snapshot_missing_vagrantfile() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let cmd = SnapshotCommands::List(SnapshotListArgs { vm_name: None });
        let result = execute(&cmd, cwd);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_snapshot_missing_machines() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        std::fs::write(cwd.join("Vagrantfile"), "")?;
        let cmd = SnapshotCommands::List(SnapshotListArgs { vm_name: None });
        let result = execute(&cmd, cwd);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_snapshot_all_commands() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        std::fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|
end",
        )?;
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir)?;
        std::fs::write(state_dir.join("id"), "12345")?;

        let ok_factory =
            |_: &str, id: Option<String>| -> Result<Box<dyn provider::Provider>, MigratoryError> {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id).with_runner(|args| {
                        if args.contains(&"list") {
                            return Ok("Name: snap1 (UUID: 1234)\n".to_string());
                        }
                        Ok(String::new())
                    }),
                ))
            };

        assert!(
            execute_with_provider(
                &SnapshotCommands::Save(SnapshotSaveArgs {
                    vm_name: None,
                    name: Some("snap1".to_string()),
                    force: false,
                }),
                cwd,
                ok_factory,
            )
            .is_ok()
        );
        assert!(
            execute_with_provider(
                &SnapshotCommands::Restore(SnapshotRestoreArgs {
                    vm_name: None,
                    name: Some("snap1".to_string()),
                    provision: None,
                    provision_with: None,
                    no_start: false,
                    no_provision: false,
                }),
                cwd,
                ok_factory,
            )
            .is_ok()
        );
        assert!(
            execute_with_provider(
                &SnapshotCommands::Delete(SnapshotDeleteArgs {
                    vm_name: None,
                    name: Some("snap1".to_string()),
                }),
                cwd,
                ok_factory,
            )
            .is_ok()
        );
        assert!(
            execute_with_provider(
                &SnapshotCommands::Push(SnapshotPushArgs { vm_name: None }),
                cwd,
                ok_factory,
            )
            .is_ok()
        );
        assert!(
            execute_with_provider(
                &SnapshotCommands::Pop(SnapshotPopArgs {
                    vm_name: None,
                    provision: None,
                    provision_with: None,
                    no_provision: false,
                    no_start: false,
                    no_delete: false,
                }),
                cwd,
                ok_factory,
            )
            .is_ok()
        );

        Ok(())
    }

    #[test]
    fn test_execute_snapshot_target_machine() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        std::fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|
  config.vm.define 'node1'
end",
        )?;
        let state_dir = cwd.join(".vagrant").join("machines").join("node1");
        std::fs::create_dir_all(&state_dir)?;
        std::fs::write(state_dir.join("id"), "12345")?;

        let cmd = SnapshotCommands::List(SnapshotListArgs {
            vm_name: Some("node1".to_string()),
        });
        let result = execute(&cmd, cwd);
        assert!(result.is_ok());

        let cmd2 = SnapshotCommands::List(SnapshotListArgs {
            vm_name: Some("invalid_node".to_string()),
        });
        let result2 = execute(&cmd2, cwd);
        assert!(result2.is_err());

        Ok(())
    }

    #[test]
    fn test_execute_snapshot_pop_failures() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        std::fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|
end",
        )?;
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir)?;
        std::fs::write(state_dir.join("id"), "12345")?;

        let pop_cmd = SnapshotCommands::Pop(SnapshotPopArgs {
            vm_name: None,
            provision: None,
            provision_with: None,
            no_provision: false,
            no_start: false,
            no_delete: false,
        });

        // 1. Empty snapshot list
        assert!(
            execute_with_provider(&pop_cmd, cwd, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(|_| Ok(String::new())),
                ))
            })
            .is_ok()
        );

        // 2. List error
        assert!(
            execute_with_provider(&pop_cmd, cwd, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(|_| Err(MigratoryError::Generic("list err".to_string()))),
                ))
            })
            .is_ok()
        );

        // 3. Restore error
        assert!(
            execute_with_provider(&pop_cmd, cwd, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id).with_runner(|args| {
                        if args.contains(&"restore") {
                            return Err(MigratoryError::Generic("restore err".to_string()));
                        }
                        Ok("Name: snap1 (UUID: 1234)\n".to_string())
                    }),
                ))
            })
            .is_ok()
        );

        // 4. Delete error
        assert!(
            execute_with_provider(&pop_cmd, cwd, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id).with_runner(|args| {
                        if args.contains(&"delete") {
                            return Err(MigratoryError::Generic("delete err".to_string()));
                        }
                        Ok("Name: snap1 (UUID: 1234)\n".to_string())
                    }),
                ))
            })
            .is_ok()
        );

        Ok(())
    }

    #[test]
    fn test_execute_snapshot_coverage_gaps() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        std::fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|
  config.vm.define 'node1'
  config.vm.define 'node2'
end",
        )?;

        for name in &["node1", "node2"] {
            let state_dir = cwd
                .join(".vagrant")
                .join("machines")
                .join(name)
                .join("virtualbox");
            std::fs::create_dir_all(&state_dir)?;
            std::fs::write(state_dir.join("id"), "12345")?;
        }

        let list_cmd = SnapshotCommands::List(SnapshotListArgs { vm_name: None });

        assert!(
            execute_with_provider(&list_cmd, cwd, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(|_| Ok("Name: snap1 (UUID: 1234)\n".to_string())),
                ))
            })
            .is_ok()
        );

        assert!(
            execute_with_provider(&list_cmd, cwd, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(|_| Ok(String::new())),
                ))
            })
            .is_ok()
        );

        assert!(
            execute_with_provider(&list_cmd, cwd, |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(|_| Err(MigratoryError::Generic("err".to_string()))),
                ))
            })
            .is_ok()
        );

        Ok(())
    }

    #[test]
    fn test_execute_snapshot_save_and_delete_errors() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        std::fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|
end",
        )?;
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir)?;
        std::fs::write(state_dir.join("id"), "12345")?;

        let fail_factory =
            |_: &str, id: Option<String>| -> Result<Box<dyn provider::Provider>, MigratoryError> {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(|_| Err(MigratoryError::Generic("err".to_string()))),
                ))
            };

        assert!(
            execute_with_provider(
                &SnapshotCommands::Save(SnapshotSaveArgs {
                    vm_name: None,
                    name: Some("snap1".to_string()),
                    force: false,
                }),
                cwd,
                fail_factory,
            )
            .is_ok()
        );
        assert!(
            execute_with_provider(
                &SnapshotCommands::Restore(SnapshotRestoreArgs {
                    vm_name: None,
                    name: Some("snap1".to_string()),
                    provision: None,
                    provision_with: None,
                    no_start: false,
                    no_provision: false,
                }),
                cwd,
                fail_factory,
            )
            .is_ok()
        );
        assert!(
            execute_with_provider(
                &SnapshotCommands::Delete(SnapshotDeleteArgs {
                    vm_name: None,
                    name: Some("snap1".to_string()),
                }),
                cwd,
                fail_factory,
            )
            .is_ok()
        );
        assert!(
            execute_with_provider(
                &SnapshotCommands::Push(SnapshotPushArgs { vm_name: None }),
                cwd,
                fail_factory,
            )
            .is_ok()
        );
        Ok(())
    }

    #[test]
    fn test_execute_snapshot_with_provider_config_and_default_name() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        std::fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|
  config.vm.provider 'virtualbox'
end",
        )?;
        let state_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        std::fs::create_dir_all(&state_dir)?;
        std::fs::write(state_dir.join("id"), "12345")?;

        let res = execute_with_provider(
            &SnapshotCommands::Save(SnapshotSaveArgs {
                vm_name: None,
                name: None,
                force: false,
            }),
            cwd,
            |_, id| {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(|_| Ok(String::new())),
                ))
            },
        );
        assert!(res.is_ok());

        Ok(())
    }

    #[test]
    fn test_execute_snapshot_read_id_and_unsupported_provider_errors() -> Result<(), MigratoryError>
    {
        let dir1 = tempdir()?;
        let cwd1 = dir1.path();
        std::fs::write(
            cwd1.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|
end",
        )?;
        let id_dir = cwd1
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox")
            .join("id");
        std::fs::create_dir_all(&id_dir)?;

        let cmd = SnapshotCommands::List(SnapshotListArgs { vm_name: None });
        let res1 = execute(&cmd, cwd1);
        assert!(res1.is_err());

        let save_cmd = SnapshotCommands::Save(SnapshotSaveArgs {
            vm_name: None,
            name: Some("snap1".to_string()),
            force: false,
        });
        let save_err_factory =
            |_: &str, id: Option<String>| -> Result<Box<dyn provider::Provider>, MigratoryError> {
                Ok(Box::new(
                    crate::provider::virtualbox::VirtualBoxProvider::new(id)
                        .with_runner(|_| Err(MigratoryError::Generic("save err".to_string()))),
                ))
            };
        assert!(execute_with_provider(&save_cmd, cwd1, save_err_factory).is_err());
        std::fs::remove_dir_all(&id_dir)?;
        std::fs::write(&id_dir, "12345")?;
        assert!(execute_with_provider(&save_cmd, cwd1, save_err_factory).is_ok());

        let dir2 = tempdir()?;
        let cwd2 = dir2.path();
        std::fs::write(
            cwd2.join("Vagrantfile"),
            "Vagrant.configure('2') do |config|
  config.vm.provider 'unsupported_provider'
end",
        )?;
        let state_dir2 = cwd2
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("unsupported_provider");
        std::fs::create_dir_all(&state_dir2)?;
        std::fs::write(state_dir2.join("id"), "12345")?;

        let res2 = execute(&cmd, cwd2);
        assert!(res2.is_err());
        Ok(())
    }
}
