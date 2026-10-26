//! Semantic implementation of the `rsync` command.
//!
//! This module provides the logic to sync rsync folders to the remote machine.

use crate::cli::RsyncArgs;
use crate::error::MigratoryError;
use std::path::Path;

/// Executes the `rsync` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The command line arguments.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found.
pub fn execute(cwd: &Path, args: &RsyncArgs) -> Result<(), MigratoryError> {
    execute_with_mount(cwd, args, |folder, opts, comm| {
        use crate::synced_folder::SyncedFolder;
        folder.mount(opts, comm)
    })
}

/// Dynamic callback type for performing an rsync synced-folder mount.
type RsyncMountFn<'a> = dyn FnMut(
        &crate::synced_folder::rsync::RsyncSyncedFolder,
        &crate::synced_folder::SyncedFolderOptions,
        &dyn crate::communicator::Communicator,
    ) -> Result<(), MigratoryError>
    + 'a;

/// Executes the `rsync` command with a customizable mount callback.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The command line arguments.
/// * `mount_fn` - Callback invoked to perform the rsync mount operation.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or if syncing fails.
pub fn execute_with_mount<F>(
    cwd: &Path,
    args: &RsyncArgs,
    mut mount_fn: F,
) -> Result<(), MigratoryError>
where
    F: FnMut(
        &crate::synced_folder::rsync::RsyncSyncedFolder,
        &crate::synced_folder::SyncedFolderOptions,
        &dyn crate::communicator::Communicator,
    ) -> Result<(), MigratoryError>,
{
    execute_with_mount_dyn(cwd, args, &mut mount_fn)
}

/// Non-generic implementation of `rsync` execution with a dynamic mount callback.
fn execute_with_mount_dyn(
    cwd: &Path,
    args: &RsyncArgs,
    mount_fn: &mut RsyncMountFn<'_>,
) -> Result<(), MigratoryError> {
    let env_config = crate::config::load_environment_config(cwd)?;

    println!("==> default: Rsyncing folder...");

    // Find configured rsync folders and sync them via the SyncedFolder abstraction
    for (machine_name, machine) in &env_config.machines {
        let mut comm_config = machine.ssh.clone();
        comm_config.insert_key = false;
        let communicator = crate::communicator::ssh::SshCommunicator::new(comm_config.clone());

        for sf_config in &machine.vm.synced_folders {
            if sf_config.disabled {
                continue;
            }
            if sf_config.folder_type == Some(crate::config::SyncedFolderType::Rsync) {
                let mut mount_options = sf_config.mount_options.clone().unwrap_or_default();
                if args.no_rsync_chown {
                    mount_options.push("rsync__chown=false".to_string());
                } else if args.rsync_chown {
                    mount_options.push("rsync__chown=true".to_string());
                }
                mount_options.push(format!("ssh_port={}", comm_config.port));
                mount_options.push(format!("ssh_host={}", comm_config.host));
                mount_options.push(format!("ssh_user={}", comm_config.username));
                if let Some(key) = &comm_config.private_key_path {
                    mount_options.push(format!("private_key_path={}", key.display()));
                }

                let opts = crate::synced_folder::SyncedFolderOptions {
                    guest_path: sf_config.guest_path.to_string_lossy().to_string(),
                    host_path: sf_config.host_path.to_string_lossy().to_string(),
                    mount_options,
                    ..Default::default()
                };

                let folder = crate::synced_folder::rsync::RsyncSyncedFolder;

                let hp = &opts.host_path;
                let gp = &opts.guest_path;
                println!("==> {machine_name}: Rsyncing {hp} to {gp}");
                use crate::synced_folder::SyncedFolder;
                let _ = folder.prepare(&opts);
                mount_fn(&folder, &opts, &communicator)?;
            }
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
    fn test_execute_rsync_missing_new() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let args = RsyncArgs {
            rsync_chown: false,
            no_rsync_chown: false,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_rsync_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        let args = RsyncArgs {
            rsync_chown: false,
            no_rsync_chown: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_rsync_with_folders() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vf = b"
Vagrant.configure('2') do |config|
  config.vm.synced_folder '.', '/vagrant', type: 'rsync'
  config.vm.synced_folder 'disabled', '/disabled', disabled: true
end
";
        std::fs::write(cwd.join("Vagrantfile"), vf)?;

        let args = RsyncArgs {
            rsync_chown: true,
            no_rsync_chown: false,
        };
        let result = execute_with_mount(cwd, &args, |_, _, _| Ok(()));
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_execute_rsync_with_folders_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vf = b"
Vagrant.configure('2') do |config|
  config.vm.synced_folder '.', '/vagrant', type: 'rsync'
end
";
        std::fs::write(cwd.join("Vagrantfile"), vf)?;

        let args = RsyncArgs {
            rsync_chown: true,
            no_rsync_chown: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_rsync_with_disabled_folder() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vf = b"
Vagrant.configure('2') do |config|
  config.vm.synced_folder '.', '/vagrant', disabled: true
end
";
        std::fs::write(cwd.join("Vagrantfile"), vf)?;

        let args = RsyncArgs {
            rsync_chown: false,
            no_rsync_chown: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_execute_rsync_with_other_folder() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vf = b"Vagrant.configure('2') do |config|\n  config.vm.synced_folder '.', '/vagrant', type: 'nfs'\nend";
        std::fs::write(cwd.join("Vagrantfile"), vf)?;
        let args = RsyncArgs {
            rsync_chown: false,
            no_rsync_chown: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_rsync_flags_and_private_key() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vf = b"Vagrant.configure('2') do |config|\n  config.ssh.private_key_path = '/tmp/key'\n  config.vm.synced_folder '.', '/vagrant', type: 'rsync'\nend";
        std::fs::write(cwd.join("Vagrantfile"), vf)?;

        // Test with no_rsync_chown: true
        let args_no_chown = RsyncArgs {
            rsync_chown: false,
            no_rsync_chown: true,
        };
        assert!(execute_with_mount(cwd, &args_no_chown, |_, _, _| Ok(())).is_ok());

        // Test with neither flag set
        let args_default = RsyncArgs {
            rsync_chown: false,
            no_rsync_chown: false,
        };
        assert!(execute_with_mount(cwd, &args_default, |_, _, _| Ok(())).is_ok());

        Ok(())
    }

    #[test]
    fn test_execute_rsync_prepare_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vf1 = b"Vagrant.configure('2') do |config|\n  config.vm.synced_folder '/nonexistent/path/for/rsync', '/vagrant', type: 'rsync'\nend";
        std::fs::write(cwd.join("Vagrantfile"), vf1)?;

        let args = RsyncArgs {
            rsync_chown: false,
            no_rsync_chown: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());

        // Test with private_key_path configured in Vagrantfile
        let vf2 = "Vagrant.configure('2') do |config|\n  config.ssh.private_key_path = '/tmp/id_rsa'\n  config.vm.synced_folder '.', '/vagrant', type: 'rsync'\nend\n";
        fs::write(cwd.join("Vagrantfile"), vf2)?;
        assert!(execute_with_mount(cwd, &args, |_, _, _| Ok(())).is_ok());
        Ok(())
    }
}
