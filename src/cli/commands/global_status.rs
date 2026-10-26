//! Semantic implementation of the `global-status` command.
//!
//! This module provides the logic to read the machine index
//! and report the global status of all Vagrant environments on the system.

use crate::cli::GlobalStatusArgs;
use crate::error::MigratoryError;
use crate::state::GlobalStateManager;

/// Resolves the Vagrant global home directory from optional `VAGRANT_HOME` and `HOME` strings.
///
/// # Arguments
///
/// * `vagrant_home` - Optional `VAGRANT_HOME` value.
/// * `home` - Optional `HOME` value.
///
/// # Returns
///
/// Returns the resolved `PathBuf` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if neither `VAGRANT_HOME` nor `HOME` is provided.
pub fn resolve_vagrant_home_from(
    vagrant_home: Option<&str>,
    home: Option<&str>,
) -> Result<std::path::PathBuf, MigratoryError> {
    if let Some(val) = vagrant_home {
        Ok(std::path::PathBuf::from(val))
    } else if let Some(h) = home {
        Ok(std::path::PathBuf::from(h).join(".vagrant.d"))
    } else {
        Err(MigratoryError::Generic(
            "Could not determine VAGRANT_HOME or HOME".to_string(),
        ))
    }
}

/// Executes the `global-status` command.
///
/// # Arguments
///
/// * `args` - The parsed arguments for the `global-status` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the global state file cannot be accessed or parsed.
pub fn execute(args: &GlobalStatusArgs) -> Result<(), MigratoryError> {
    let vh = std::env::var("VAGRANT_HOME").ok();
    let home = std::env::var("HOME").ok();
    let vagrant_d = resolve_vagrant_home_from(vh.as_deref(), home.as_deref())?;
    execute_with_home(args, vagrant_d)
}

/// Executes the `global-status` command with an explicit Vagrant home directory.
///
/// # Arguments
///
/// * `args` - The parsed arguments for the `global-status` command.
/// * `vagrant_d` - Path to the Vagrant global state directory.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the global state file cannot be accessed or parsed.
pub fn execute_with_home(
    args: &GlobalStatusArgs,
    vagrant_d: std::path::PathBuf,
) -> Result<(), MigratoryError> {
    let state_manager = GlobalStateManager::new(vagrant_d);

    if args.prune {
        println!("Pruning invalid entries from index...");
        let pruned = state_manager.prune()?;
        if pruned > 0 {
            println!("Pruned {} invalid entries.", pruned);
        }
    }

    let index = state_manager.read_index()?;

    println!("id       name    provider   state    directory");
    println!("-------------------------------------------------------------------------");

    if index.machines.is_empty() {
        println!("There are no active Vagrant environments on this computer! Or,");
        println!("you haven't destroyed and recreated vagrant environments that were");
        println!("started with an older version of Vagrant.");
    } else {
        for (id, machine) in index.machines {
            let short_id = if id.len() >= 7 { &id[0..7] } else { &id };
            println!(
                "{:<8} {:<7} {:<10} {:<8} {}",
                short_id, machine.name, machine.provider, machine.state, machine.local_data_path
            );
        }
        println!("\nThe above shows information about all known Vagrant environments");
        println!("on this machine. This data is cached and may not be completely");
        println!("up-to-date (use \"migratory global-status --prune\" to prune invalid");
        println!("entries). To send a command to a specific machine, use the 7-character");
        println!("ID that is shown in the first column of each entry:\n");
        println!("  migratory <command> <id>");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_global_status() {
        let temp = std::env::temp_dir().join(format!("mig_gs_1_{}", uuid::Uuid::new_v4()));
        let index_dir = temp.join("data").join("machine-index");
        let _ = std::fs::create_dir_all(&index_dir);
        let _ = std::fs::write(
            index_dir.join("index"),
            "{\"version\": 1, \"machines\": {}}",
        );

        let args = GlobalStatusArgs { prune: true };
        let result = execute_with_home(&args, temp.clone());
        let _ = std::fs::remove_dir_all(&temp);

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_global_status_read_index_error() {
        let temp = std::env::temp_dir().join(format!("mig_gs_2_{}", uuid::Uuid::new_v4()));
        let index_dir = temp.join("data").join("machine-index");
        let _ = std::fs::create_dir_all(&index_dir);
        let index_path = index_dir.join("index");
        let _ = std::fs::write(&index_path, "{ invalid json ");

        let args = GlobalStatusArgs { prune: false };
        let result = execute_with_home(&args, temp.clone());
        let _ = std::fs::remove_dir_all(&temp);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_global_status_no_home_dir() {
        assert!(resolve_vagrant_home_from(None, None).is_err());
        assert_eq!(
            resolve_vagrant_home_from(Some("/custom"), Some("/home")).ok(),
            Some(std::path::PathBuf::from("/custom"))
        );
        assert_eq!(
            resolve_vagrant_home_from(None, Some("/home/user")).ok(),
            Some(std::path::PathBuf::from("/home/user/.vagrant.d"))
        );
    }

    #[test]
    fn test_execute_global_status_with_machines() {
        let temp = std::env::temp_dir().join(format!("mig_gs_3_{}", uuid::Uuid::new_v4()));
        let index_dir = temp.join("data").join("machine-index");
        let _ = std::fs::create_dir_all(&index_dir);
        let index_path = index_dir.join("index");

        let valid_json = r#"{
            "version": 1,
            "machines": {
                "abcdefg12345": {
                    "local_data_path": "/some/path",
                    "name": "default",
                    "provider": "virtualbox",
                    "state": "running",
                    "vagrantfile_name": "",
                    "vagrantfile_path": "/some/path",
                    "updated_at": 1690000000,
                    "extra_data": {
                        "box_name": "ubuntu/focal64",
                        "box_provider": "virtualbox",
                        "box_version": "20.04"
                    }
                },
                "short": {
                    "local_data_path": "/other/path",
                    "name": "other",
                    "provider": "qemu",
                    "state": "shutoff",
                    "vagrantfile_name": "",
                    "vagrantfile_path": "/other/path",
                    "updated_at": 1690000001,
                    "extra_data": {}
                }
            }
        }"#;

        let _ = std::fs::write(&index_path, valid_json);

        let args = GlobalStatusArgs { prune: false };
        assert!(execute_with_home(&args, temp.clone()).is_ok());
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_execute_global_status_home_only() {
        let temp = std::env::temp_dir().join(format!("mig_gs_4_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&temp);
        let vagrant_d = temp.join(".vagrant.d");
        let args = GlobalStatusArgs { prune: false };
        let result = execute_with_home(&args, vagrant_d);
        let _ = std::fs::remove_dir_all(&temp);
        assert!(result.is_ok());
    }
}
#[cfg(test)]
mod extra_global_status_tests {
    use super::*;

    #[test]
    fn test_execute_global_status_prune() {
        let temp = std::env::temp_dir().join(format!("mig_gs_5_{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(temp.join("data").join("machine-index"));

        let index_content = r#"{
            "version": 1,
            "machines": {
                "invalid_machine_id": {
                    "local_data_path": "/path/that/does/not/exist",
                    "name": "default",
                    "provider": "virtualbox",
                    "state": "running",
                    "vagrantfile_name": "Vagrantfile",
                    "vagrantfile_path": "/path/that/does/not/exist",
                    "updated_at": 1672531200,
                    "extra_data": {}
                }
            }
        }"#;
        let _ = std::fs::write(
            temp.join("data").join("machine-index").join("index"),
            index_content,
        );

        let args = GlobalStatusArgs { prune: true };
        let res = execute_with_home(&args, temp.clone());
        let _ = std::fs::remove_dir_all(&temp);
        assert!(res.is_ok());
        let _ = execute(&GlobalStatusArgs { prune: false });
    }

    #[test]
    fn test_execute_global_status_prune_error() {
        let temp = std::env::temp_dir().join(format!("mig_gs_6_{}", uuid::Uuid::new_v4()));
        let index_dir = temp.join("data").join("machine-index");
        let _ = std::fs::create_dir_all(&index_dir);
        let index_path = index_dir.join("index");
        let _ = std::fs::write(&index_path, "{ invalid json ");

        let args = GlobalStatusArgs { prune: true };
        let result = execute_with_home(&args, temp.clone());
        let _ = std::fs::remove_dir_all(&temp);
        assert!(result.is_err());
    }
}
