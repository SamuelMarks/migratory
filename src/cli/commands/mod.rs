//! Modules containing semantic implementations of the CLI subcommands.

/// `autocomplete` subcommand implementation.
pub mod autocomplete;
/// `box` subcommand implementation.
pub mod box_cmd;
/// `cap` subcommand implementation.
pub mod cap;
/// `cloud` subcommand implementation.
pub mod cloud_cmd;
/// `destroy` subcommand implementation.
pub mod destroy;
/// `docker-exec` subcommand implementation.
pub mod docker_exec;
/// `docker-logs` subcommand implementation.
pub mod docker_logs;
/// `docker-run` subcommand implementation.
pub mod docker_run;
/// `global-status` subcommand implementation.
pub mod global_status;
/// `halt` subcommand implementation.
pub mod halt;
/// `init` subcommand implementation.
pub mod init;
/// `list-commands` subcommand implementation.
pub mod list_commands;
/// `login` subcommand implementation.
pub mod login;
/// `mutate` subcommand implementation.
pub mod mutate;
/// `package` subcommand implementation.
pub mod package;
/// `plugin` subcommand implementation.
pub mod plugin_cmd;
/// `port` subcommand implementation.
pub mod port;
/// `powershell` subcommand implementation.
pub mod powershell;
/// `provider` subcommand implementation.
pub mod provider;
/// `provision` subcommand implementation.
pub mod provision;
/// `push` subcommand implementation.
pub mod push;
/// `rdp` subcommand implementation.
pub mod rdp;
/// `reload` subcommand implementation.
pub mod reload;
/// `resume` subcommand implementation.
pub mod resume;
/// `rsync` subcommand implementation.
pub mod rsync;
/// `rsync-auto` subcommand implementation.
pub mod rsync_auto;
/// `snapshot` subcommand implementation.
pub mod snapshot_cmd;
/// `ssh` subcommand implementation.
pub mod ssh;
/// `ssh-config` subcommand implementation.
pub mod ssh_config;
/// `status` subcommand implementation.
pub mod status;
/// `suspend` subcommand implementation.
pub mod suspend;
/// `up` subcommand implementation.
pub mod up;
/// `upload` subcommand implementation.
pub mod upload;
/// `validate` subcommand implementation.
pub mod validate;
/// `version` subcommand implementation.
pub mod version;
/// `winrm` subcommand implementation.
pub mod winrm;
/// `winrm-config` subcommand implementation.
pub mod winrm_config;
