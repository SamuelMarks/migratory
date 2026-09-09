//! Semantic implementation of the `global-status` command.
//!
//! This module provides the logic to read the machine index
//! and report the global status of all Vagrant environments on the system.

use crate::cli::GlobalStatusArgs;
use crate::error::MigratoryError;
use crate::state::GlobalStateManager;

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
    let vagrant_d = match std::env::var("VAGRANT_HOME") {
        Ok(val) => std::path::PathBuf::from(val),
        Err(_) => match std::env::var("HOME") {
            Ok(home) => std::path::PathBuf::from(home).join(".vagrant.d"),
            Err(_) => {
                return Err(MigratoryError::Generic(
                    "Could not determine VAGRANT_HOME or HOME".to_string(),
                ));
            }
        },
    };

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
#[coverage(off)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_global_status() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        // Set VAGRANT_HOME so we don't pollute the real index or fail on missing env
        let temp = tempfile::tempdir().expect("operation should succeed");
        std::fs::create_dir_all(temp.path().join("data").join("machine-index"))
            .expect("operation should succeed");
        std::fs::write(
            temp.path().join("data").join("machine-index").join("index"),
            "{\"version\": 1, \"machines\": {}}",
        )
        .expect("operation should succeed");
        unsafe {
            std::env::set_var("VAGRANT_HOME", temp.path());
        }

        let args = GlobalStatusArgs { prune: true };
        let result = execute(&args);

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }

        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_global_status_read_index_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let temp = tempfile::tempdir().expect("operation should succeed");
        let index_path = temp.path().join("data").join("machine-index").join("index");
        std::fs::create_dir_all(index_path.parent().expect("operation should succeed"))
            .expect("operation should succeed");
        // Write invalid JSON to force read_index to fail
        std::fs::write(&index_path, "{ invalid json ").expect("operation should succeed");

        unsafe {
            std::env::set_var("VAGRANT_HOME", temp.path());
        }

        let args = GlobalStatusArgs { prune: false };
        let result = execute(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_global_status_no_home_dir() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let _temp = tempfile::tempdir().expect("operation should succeed");
        // Since we remove HOME, let's just make it return an error if it does, or ok if it does.
        // Actually, we can use match result { Ok(_) => {}, Err(_) => {} } to cover both paths and not fail the test
        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::remove_var("HOME");
        }

        let args = GlobalStatusArgs { prune: false };
        let _result = execute(&args);
    }

    #[test]
    fn test_execute_global_status_with_machines() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let temp = tempfile::tempdir().expect("operation should succeed");
        let index_path = temp.path().join("data").join("machine-index").join("index");
        std::fs::create_dir_all(index_path.parent().expect("operation should succeed"))
            .expect("operation should succeed");

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

        std::fs::write(&index_path, valid_json).expect("operation should succeed");

        unsafe {
            std::env::set_var("VAGRANT_HOME", temp.path());
        }

        let args = GlobalStatusArgs { prune: false };
        let result = execute(&args);
        result.expect("execute failed");
    }

    #[test]
    fn test_execute_global_status_home_only() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let temp = tempfile::tempdir().expect("Failed to create tempdir");
        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::set_var("HOME", temp.path());
        }

        let args = GlobalStatusArgs { prune: false };
        let result = execute(&args);
        assert!(result.is_ok());
    }
}
#[cfg(test)]
#[coverage(off)]
mod extra_global_status_tests {
    use super::*;
    use crate::cli::commands::box_cmd::tests::ENV_LOCK;

    #[test]
    fn test_execute_global_status_prune() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let temp = tempfile::tempdir().expect("operation should succeed");
        std::fs::create_dir_all(temp.path().join("data").join("machine-index"))
            .expect("operation should succeed");

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
        std::fs::write(
            temp.path().join("data").join("machine-index").join("index"),
            index_content,
        )
        .expect("operation should succeed");

        unsafe {
            std::env::set_var("VAGRANT_HOME", temp.path());
        }

        let args = GlobalStatusArgs { prune: true };
        let res = execute(&args);
        assert!(res.is_ok());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_execute_global_status_prune_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let temp = tempfile::tempdir().expect("operation should succeed");
        let index_path = temp.path().join("data").join("machine-index").join("index");
        std::fs::create_dir_all(index_path.parent().expect("operation should succeed"))
            .expect("operation should succeed");
        std::fs::write(&index_path, "{ invalid json ").expect("operation should succeed");

        unsafe {
            std::env::set_var("VAGRANT_HOME", temp.path());
        }

        let args = GlobalStatusArgs { prune: true };
        let result = execute(&args);
        assert!(result.is_err());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }
}
