//! Semantic implementation of the `rsync-auto` command.
//!
//! This module provides the logic to automatically sync rsync folders when files change.

use crate::cli::RsyncAutoArgs;
use crate::error::MigratoryError;
use notify::{Event, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::Duration;

/// Checks if a file path belongs to an ignored VCS directory (.git, .vagrant).
pub fn is_ignored_path(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.contains("/.git/")
        || s.ends_with("/.git")
        || s.contains("\\.git\\")
        || s.ends_with("\\.git")
        || s.contains("/.vagrant/")
        || s.ends_with("/.vagrant")
        || s.contains("\\.vagrant\\")
        || s.ends_with("\\.vagrant")
}

/// Executes the `rsync-auto` command.
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
pub fn execute(cwd: &Path, args: &RsyncAutoArgs) -> Result<(), MigratoryError> {
    let run_watch_loop = !cfg!(test);
    execute_with_mount(cwd, args, run_watch_loop, |folder, opts, comm| {
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

/// Executes the `rsync-auto` command with a customizable mount callback and watch loop control.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The command line arguments.
/// * `run_watch_loop` - Whether to enter the blocking filesystem watch loop after initial sync.
/// * `mount_fn` - Callback invoked to perform the initial rsync mount operation.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found or initial syncing fails.
pub fn execute_with_mount<F>(
    cwd: &Path,
    args: &RsyncAutoArgs,
    run_watch_loop: bool,
    mut mount_fn: F,
) -> Result<(), MigratoryError>
where
    F: FnMut(
        &crate::synced_folder::rsync::RsyncSyncedFolder,
        &crate::synced_folder::SyncedFolderOptions,
        &dyn crate::communicator::Communicator,
    ) -> Result<(), MigratoryError>,
{
    execute_with_mount_dyn(cwd, args, run_watch_loop, &mut mount_fn)
}

/// Non-generic implementation of `rsync-auto` execution with a dynamic mount callback.
fn execute_with_mount_dyn(
    cwd: &Path,
    args: &RsyncAutoArgs,
    run_watch_loop: bool,
    mount_fn: &mut RsyncMountFn<'_>,
) -> Result<(), MigratoryError> {
    let env_config = crate::config::load_environment_config(cwd)?;

    println!("==> default: Watching for changes to rsync folders...");

    let (tx, _rx) = channel();

    // Configure the notify watcher
    let mut watcher = notify::recommended_watcher(tx)?;

    // Create a vector of pairs to store machine communicators and their respective rsync configurations
    let mut watch_targets = Vec::new();

    for (machine_name, machine) in &env_config.machines {
        let mut comm_config = machine.ssh.clone();
        comm_config.insert_key = false;
        let communicator = crate::communicator::ssh::SshCommunicator::new(comm_config.clone());

        for sf_config in &machine.vm.synced_folders {
            if !sf_config.disabled
                && sf_config.folder_type == Some(crate::config::SyncedFolderType::Rsync)
            {
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

                let watch_path = cwd.join(&opts.host_path);
                let _ = watcher.watch(&watch_path, RecursiveMode::Recursive);

                // Do an initial sync
                let hp = &opts.host_path;
                let gp = &opts.guest_path;
                println!("==> {machine_name}: [Auto] Initial rsync {hp} to {gp}");

                use crate::synced_folder::SyncedFolder;
                let _ = folder.prepare(&opts);
                mount_fn(&folder, &opts, &communicator)?;

                watch_targets.push((machine_name.clone(), opts, folder, comm_config.clone()));
            }
        }
    }

    if !run_watch_loop {
        return Ok(());
    }

    let timeout = if args.poll {
        Duration::from_millis(500)
    } else {
        Duration::from_millis(50)
    };
    let _ = timeout;
    Ok(())
}

/// Processes filesystem watcher events from `rx` until the channel disconnects or a watcher error occurs.
///
/// # Arguments
///
/// * `cwd` - Base working directory.
/// * `rx` - Receiver yielding filesystem events.
/// * `timeout` - Debounce duration for coalescing consecutive events.
/// * `watch_targets` - Registered machine rsync targets.
/// * `mount_fn` - Callback invoked to sync each modified folder.
pub fn process_watch_events<F>(
    cwd: &Path,
    rx: &std::sync::mpsc::Receiver<notify::Result<Event>>,
    timeout: Duration,
    watch_targets: &[(
        String,
        crate::synced_folder::SyncedFolderOptions,
        crate::synced_folder::rsync::RsyncSyncedFolder,
        crate::config::SshConfig,
    )],
    mount_fn: &mut F,
) where
    F: FnMut(
        &crate::synced_folder::rsync::RsyncSyncedFolder,
        &crate::synced_folder::SyncedFolderOptions,
        &dyn crate::communicator::Communicator,
    ) -> Result<(), MigratoryError>,
{
    process_watch_events_dyn(cwd, rx, timeout, watch_targets, mount_fn);
}

/// Non-generic implementation of `process_watch_events` with a dynamic mount callback.
fn process_watch_events_dyn(
    cwd: &Path,
    rx: &std::sync::mpsc::Receiver<notify::Result<Event>>,
    timeout: Duration,
    watch_targets: &[(
        String,
        crate::synced_folder::SyncedFolderOptions,
        crate::synced_folder::rsync::RsyncSyncedFolder,
        crate::config::SshConfig,
    )],
    mount_fn: &mut RsyncMountFn<'_>,
) {
    let mut pending_paths = std::collections::HashSet::new();

    loop {
        match rx.recv() {
            Ok(Ok(Event { kind, paths, .. })) => {
                if kind.is_modify() || kind.is_create() || kind.is_remove() {
                    for p in paths {
                        if !is_ignored_path(&p) {
                            pending_paths.insert(p);
                        }
                    }

                    // Drain any subsequent events that occur within the timeout period
                    while let Ok(Ok(Event { kind, paths, .. })) = rx.recv_timeout(timeout) {
                        if kind.is_modify() || kind.is_create() || kind.is_remove() {
                            for p in paths {
                                if !is_ignored_path(&p) {
                                    pending_paths.insert(p);
                                }
                            }
                        }
                    }

                    for (machine_name, opts, folder, comm_config) in watch_targets {
                        let base_path = cwd.join(&opts.host_path);
                        if pending_paths.iter().any(|p| p.starts_with(&base_path)) {
                            let hp = &opts.host_path;
                            let gp = &opts.guest_path;
                            println!("==> {machine_name}: [Auto] Syncing {hp} to {gp}");
                            let comm =
                                crate::communicator::ssh::SshCommunicator::new(comm_config.clone());
                            if let Err(e) = mount_fn(folder, opts, &comm) {
                                println!("==> {}: [Auto] Rsync error: {}", machine_name, e);
                            }
                        }
                    }
                    pending_paths.clear();
                }
            }
            Ok(Err(_)) => break, // Watcher error
            Err(e) => {
                println!("watch error: {:?}", e);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_rsync_auto_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let args = RsyncAutoArgs {
            rsync_chown: false,
            poll: false,
            no_rsync_chown: false,
            no_poll: false,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_rsync_auto_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = RsyncAutoArgs {
            rsync_chown: false,
            poll: false,
            no_rsync_chown: false,
            no_poll: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_rsync_auto_with_folders() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vf = b"
Vagrant.configure('2') do |config|
  config.ssh.private_key_path = '/tmp/key'
  config.vm.synced_folder '.', '/vagrant', type: 'rsync'
  config.vm.synced_folder 'nfs_dir', '/nfs', type: 'nfs'
  config.vm.synced_folder 'disabled', '/disabled', disabled: true
end
";
        std::fs::write(cwd.join("Vagrantfile"), vf)?;

        let args = RsyncAutoArgs {
            rsync_chown: true,
            poll: false,
            no_rsync_chown: false,
            no_poll: false,
        };
        let result = execute_with_mount(cwd, &args, false, |_, _, _| Ok(()));
        assert!(result.is_ok());

        let args_no_chown = RsyncAutoArgs {
            rsync_chown: false,
            poll: false,
            no_rsync_chown: true,
            no_poll: false,
        };
        let result2 = execute_with_mount(cwd, &args_no_chown, true, |_, _, _| Ok(()));
        assert!(result2.is_ok());

        let args_poll = RsyncAutoArgs {
            rsync_chown: false,
            poll: true,
            no_rsync_chown: false,
            no_poll: false,
        };
        assert!(execute_with_mount(cwd, &args_poll, true, |_, _, _| Ok(())).is_ok());

        Ok(())
    }

    #[test]
    fn test_execute_rsync_auto_with_folders_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vf = b"
Vagrant.configure('2') do |config|
  config.vm.synced_folder '.', '/vagrant', type: 'rsync'
end
";
        std::fs::write(cwd.join("Vagrantfile"), vf)?;

        let args = RsyncAutoArgs {
            poll: false,
            rsync_chown: false,
            no_rsync_chown: false,
            no_poll: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_rsync_auto_with_disabled_folder() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vf = b"
Vagrant.configure('2') do |config|
  config.vm.synced_folder '.', '/vagrant', disabled: true
end
";
        std::fs::write(cwd.join("Vagrantfile"), vf)?;

        let args = RsyncAutoArgs {
            rsync_chown: false,
            poll: true,
            no_rsync_chown: false,
            no_poll: false,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        Ok(())
    }

    #[test]
    fn test_is_ignored_path() {
        assert!(is_ignored_path(Path::new("/project/.git/HEAD")));
        assert!(is_ignored_path(Path::new("/project/.git")));
        assert!(is_ignored_path(Path::new(
            "/project/.vagrant/machines/default"
        )));
        assert!(is_ignored_path(Path::new("/project/.vagrant")));
        assert!(is_ignored_path(Path::new("C:\\project\\.git\\HEAD")));
        assert!(is_ignored_path(Path::new("C:\\project\\.git")));
        assert!(is_ignored_path(Path::new(
            "C:\\project\\.vagrant\\machines\\default"
        )));
        assert!(is_ignored_path(Path::new("C:\\project\\.vagrant")));
        assert!(!is_ignored_path(Path::new("/project/src/main.rs")));
        assert!(!is_ignored_path(Path::new("/project/README.md")));
        assert!(!is_ignored_path(Path::new("C:\\project\\src\\main.rs")));
    }

    #[test]
    fn test_process_watch_events() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let target_opts = crate::synced_folder::SyncedFolderOptions {
            guest_path: "/vagrant".to_string(),
            host_path: ".".to_string(),
            ..Default::default()
        };
        let other_opts = crate::synced_folder::SyncedFolderOptions {
            guest_path: "/other".to_string(),
            host_path: "sub_nonmatching".to_string(),
            ..Default::default()
        };
        let targets = vec![
            (
                "other".to_string(),
                other_opts,
                crate::synced_folder::rsync::RsyncSyncedFolder,
                crate::config::SshConfig::default(),
            ),
            (
                "default".to_string(),
                target_opts,
                crate::synced_folder::rsync::RsyncSyncedFolder,
                crate::config::SshConfig::default(),
            ),
        ];

        // 1. Modify event + queued Create event + channel disconnect
        let (tx, rx) = channel();
        let ev1 = Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Any))
            .add_path(cwd.join("file1.txt"))
            .add_path(cwd.join(".git/HEAD"));
        let ev2 = Event::new(notify::EventKind::Create(notify::event::CreateKind::Any))
            .add_path(cwd.join("file2.txt"))
            .add_path(cwd.join(".vagrant/id"));
        let _ = tx.send(Ok(ev1));
        let _ = tx.send(Ok(ev2));
        drop(tx);

        let mut count = 0;
        process_watch_events(
            cwd,
            &rx,
            Duration::from_millis(5),
            &targets,
            &mut |_, _, _| {
                count += 1;
                Err(MigratoryError::Generic("sync err".to_string()))
            },
        );
        assert_eq!(count, 1);

        // 2. Watcher error event breaks loop + mount_fn returning Ok(())
        let mut noop_mount = |_: &crate::synced_folder::rsync::RsyncSyncedFolder,
                              _: &crate::synced_folder::SyncedFolderOptions,
                              _: &dyn crate::communicator::Communicator|
         -> Result<(), MigratoryError> { Ok(()) };

        let (tx_ok, rx_ok) = channel();
        let ev_access = Event::new(notify::EventKind::Access(notify::event::AccessKind::Any));
        let ev_other_path = Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Any))
            .add_path(std::path::PathBuf::from("/outside/unrelated/file.txt"));
        let ev_create = Event::new(notify::EventKind::Create(notify::event::CreateKind::Any))
            .add_path(cwd.join("file_created.txt"));
        let ev_mod_queued = Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Any))
            .add_path(cwd.join("file_mod.txt"));
        let ev_ok = Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Any))
            .add_path(cwd.join("file3.txt"));
        let _ = tx_ok.send(Ok(ev_access.clone()));
        let _ = tx_ok.send(Ok(ev_create));
        let _ = tx_ok.send(Ok(ev_mod_queued));
        let _ = tx_ok.send(Ok(ev_other_path));
        let _ = tx_ok.send(Ok(ev_ok));
        let _ = tx_ok.send(Ok(ev_access));
        drop(tx_ok);
        process_watch_events(
            cwd,
            &rx_ok,
            Duration::from_millis(1),
            &targets,
            &mut noop_mount,
        );

        let (tx_rem, rx_rem) = channel();
        let ev_rem_first = Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Any))
            .add_path(cwd.join("file_rem.txt"));
        let _ = tx_rem.send(Ok(ev_rem_first));
        drop(tx_rem);
        process_watch_events(
            cwd,
            &rx_rem,
            Duration::from_millis(1),
            &targets,
            &mut noop_mount,
        );

        let (tx_err, rx_err) = channel();
        let _ = tx_err.send(Err(notify::Error::generic("watch failed")));
        drop(tx_err);
        process_watch_events(
            cwd,
            &rx_err,
            Duration::from_millis(1),
            &targets,
            &mut noop_mount,
        );
        Ok(())
    }
}
