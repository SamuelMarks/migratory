//! CLI module for parsing and routing commands.
//!
//! This module defines the structure and parsing logic for the `migratory`
//! command-line interface using `clap`.

pub mod commands;

use clap::{Parser, Subcommand};

/// Migratory CLI entrypoint.
///
/// This struct defines the global options and the primary subcommands
/// available in the application.
#[derive(Parser)]
#[command(name = "migratory")]
#[command(about = "An open-source, 100% compatible replica of Vagrant.", long_about = None)]
#[command(disable_help_subcommand = true)]
#[command(
    after_help = "Additional subcommands are available, but are either more advanced\nor not commonly used. To see all subcommands, run the command\n`migratory list-commands`."
)]
pub struct Cli {
    /// Enable or disable color output.
    #[arg(long, default_value_t = true, global = true)]
    pub color: bool,

    /// Disable color output (alias for --color=false).
    /// Negates the flag.
    #[arg(long, overrides_with = "color", global = true)]
    pub no_color: bool,

    /// Enable debug output.
    #[arg(long, global = true)]
    pub debug: bool,

    /// Turn on machine readable output.
    #[arg(long, global = true)]
    pub machine_readable: bool,

    /// Enable timestamps on log output.
    #[arg(long, global = true)]
    pub timestamp: bool,

    /// Enable debug output with timestamps.
    #[arg(long, global = true)]
    pub debug_timestamp: bool,

    /// Enable non-interactive output.
    #[arg(long, global = true)]
    pub no_tty: bool,

    /// Display Vagrant version.
    #[arg(short, long, global = true)]
    pub version: bool,

    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            color: true,
            no_color: false,
            debug: false,
            machine_readable: false,
            timestamp: false,
            debug_timestamp: false,
            no_tty: false,
            version: false,
            command: None,
        }
    }
}

/// Available CLI subcommands.
#[derive(Subcommand)]
pub enum Commands {
    /// Manages autocomplete installation on host.
    #[command(subcommand)]
    Autocomplete(AutocompleteCommands),
    /// Checks and executes capability.
    Cap(CapArgs),
    /// Manages everything related to Vagrant Cloud.
    #[command(subcommand)]
    Cloud(CloudCommands),
    /// Attach to an already-running docker container.
    DockerExec(DockerExecArgs),
    /// Outputs the logs from the Docker container.
    DockerLogs(DockerLogsArgs),
    /// Run a one-off command in the context of a container.
    DockerRun(DockerRunArgs),
    /// Initializes a new Migratory environment by creating a Vagrantfile.
    Init(InitArgs),
    /// Starts and provisions the vagrant environment.
    Up(UpArgs),
    /// Stops and deletes all traces of the vagrant machine.
    Destroy(DestroyArgs),
    /// Stops the vagrant machine.
    Halt(HaltArgs),
    /// Suspends the machine.
    Suspend(SuspendArgs),
    /// Resumes a suspend machine.
    Resume(ResumeArgs),
    /// Restarts vagrant machine, loads new Vagrantfile configuration.
    Reload(ReloadArgs),
    /// Connects to machine via SSH.
    Ssh(SshArgs),
    /// Outputs OpenSSH valid configuration to connect to the machine.
    SshConfig(SshConfigArgs),
    /// Executes commands on a machine via WinRM.
    Winrm(WinrmArgs),
    /// Outputs WinRM configuration to connect to the machine.
    WinrmConfig(WinrmConfigArgs),
    /// Generates an RDP file for Windows guests.
    Rdp(RdpArgs),
    /// Outputs status of the vagrant machine.
    Status(StatusArgs),
    /// Outputs status of all vagrant machines on this system.
    GlobalStatus(GlobalStatusArgs),
    /// Displays information about guest port bindings.
    Port(PortArgs),
    /// Connects to machine via powershell remoting.
    Powershell(PowershellArgs),
    /// Show provider for this environment.
    Provider(ProviderArgs),
    /// Provisions the vagrant machine.
    Provision(ProvisionArgs),
    /// Deploys code in this environment to a configured destination.
    Push,
    /// Syncs rsync synced folders to remote machine.
    Rsync(RsyncArgs),
    /// Syncs rsync synced folders automatically when files change.
    RsyncAuto(RsyncAutoArgs),
    /// Upload to machine via communicator.
    Upload(UploadArgs),
    /// Validates the Vagrantfile.
    Validate(ValidateArgs),
    /// Prints current and latest Vagrant version.
    Version,
    /// Packages a running vagrant environment into a box.
    Package(PackageArgs),
    /// Manages boxes: installation, removal, etc.
    #[command(subcommand)]
    Box(BoxCommands),
    /// Authenticates with Vagrant Cloud.
    Login(LoginArgs),
    /// Mutates a box.
    Mutate(MutateArgs),
    /// Manages plugins.
    #[command(subcommand)]
    Plugin(PluginCommands),
    /// Manages snapshots.
    #[command(subcommand)]
    Snapshot(SnapshotCommands),
    /// Outputs all available Vagrant subcommands, even non-primary ones.
    ListCommands,
    /// Print help for a command.
    Help(HelpArgs),
}

/// Autocomplete subcommands.
#[derive(Subcommand)]
pub enum AutocompleteCommands {
    /// Installs autocomplete.
    Install(AutocompleteInstallArgs),
}

/// Arguments for `autocomplete install`.
#[derive(Default, clap::Args, Clone)]
pub struct AutocompleteInstallArgs {
    /// Install for Bash.
    #[arg(short = 'b', long)]
    pub bash: bool,

    /// Install for Zsh.
    #[arg(short = 'z', long)]
    pub zsh: bool,

    /// Install for Fish.
    #[arg(short = 'f', long)]
    pub fish: bool,
}

/// Cloud subcommands.
#[derive(Subcommand)]
pub enum CloudCommands {
    /// Cloud authentication.
    #[command(subcommand)]
    Auth(CloudAuthCommands),
    /// Cloud boxes.
    #[command(subcommand)]
    Box(CloudBoxCommands),
    /// Cloud providers.
    #[command(subcommand)]
    Provider(CloudProviderCommands),
    /// Publish a cloud box.
    Publish(CloudPublishArgs),
    /// Search cloud.
    Search(CloudSearchArgs),
    /// Cloud versions.
    #[command(subcommand)]
    Version(CloudVersionCommands),
}

/// Cloud auth subcommands.
#[derive(Subcommand)]
pub enum CloudAuthCommands {
    /// Login to Vagrant Cloud.
    Login(CloudAuthLoginArgs),
    /// Logout from Vagrant Cloud.
    Logout,
    /// Prints the current logged in user.
    Whoami,
}

/// Arguments for `cloud auth login`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudAuthLoginArgs {
    /// Token to use for login.
    #[arg(short, long)]
    pub token: Option<String>,

    /// Username.
    #[arg(short, long)]
    pub username: Option<String>,

    /// Description.
    #[arg(short, long)]
    pub description: Option<String>,

    /// Check login state.
    #[arg(short, long)]
    pub check: bool,
}

/// Cloud box subcommands.
#[derive(Subcommand)]
pub enum CloudBoxCommands {
    /// Creates a cloud box.
    Create(CloudBoxCreateArgs),
    /// Deletes a cloud box.
    Delete(CloudBoxDeleteArgs),
    /// Shows a cloud box.
    Show(CloudBoxShowArgs),
    /// Updates a cloud box.
    Update(CloudBoxUpdateArgs),
}

/// Arguments for `cloud box create`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudBoxCreateArgs {
    /// Name of the box (e.g., organization/box-name).
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Short description.
    #[arg(short, long)]
    pub short_description: Option<String>,

    /// Description.
    #[arg(short, long)]
    pub description: Option<String>,

    /// Make the box private.
    #[arg(short, long)]
    pub private: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "private")]
    pub no_private: bool,
}

/// Arguments for `cloud box delete`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudBoxDeleteArgs {
    /// Name of the box.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Force deletion without confirmation.
    #[arg(short, long)]
    pub force: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "force")]
    pub no_force: bool,
}

/// Arguments for `cloud box show`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudBoxShowArgs {
    /// Name of the box.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Architectures.
    #[arg(long)]
    pub architectures: bool,

    /// Versions.
    #[arg(long)]
    pub versions: bool,

    /// Providers.
    #[arg(long)]
    pub providers: bool,

    /// Authentication.
    #[arg(long)]
    pub auth: Option<String>,

    /// Negates the flag.
    #[arg(long, overrides_with = "auth")]
    pub no_auth: bool,
}

/// Arguments for `cloud box update`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudBoxUpdateArgs {
    /// Name of the box.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Short description.
    #[arg(short, long)]
    pub short_description: Option<String>,

    /// Description.
    #[arg(short, long)]
    pub description: Option<String>,

    /// Make the box private.
    #[arg(short, long)]
    pub private: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "private")]
    pub no_private: bool,
}

/// Cloud provider subcommands.
#[derive(Subcommand)]
pub enum CloudProviderCommands {
    /// Creates a cloud provider.
    Create(CloudProviderCreateArgs),
    /// Deletes a cloud provider.
    Delete(CloudProviderDeleteArgs),
    /// Updates a cloud provider.
    Update(CloudProviderUpdateArgs),
    /// Uploads a cloud provider.
    Upload(CloudProviderUploadArgs),
}

/// Arguments for `cloud provider create`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudProviderCreateArgs {
    /// Name of the box (e.g., organization/box-name).
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Provider name.
    #[arg(index = 2)]
    pub provider: Option<String>,

    /// Version.
    #[arg(index = 3)]
    pub version: Option<String>,

    /// URL (optional).
    #[arg(index = 4)]
    pub url: Option<String>,

    /// Checksum.
    #[arg(short = 'c', long)]
    pub checksum: Option<String>,

    /// Checksum type.
    #[arg(short = 'C', long)]
    pub checksum_type: Option<String>,

    /// Architecture.
    #[arg(short = 'a', long)]
    pub architecture: Option<String>,

    /// Default architecture.
    #[arg(long)]
    pub default_architecture: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "default_architecture")]
    pub no_default_architecture: bool,
}

/// Arguments for `cloud provider delete`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudProviderDeleteArgs {
    /// Name of the box.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Provider name.
    #[arg(index = 2)]
    pub provider: Option<String>,

    /// Version.
    #[arg(index = 3)]
    pub version: Option<String>,

    /// Force deletion without confirmation.
    #[arg(short, long)]
    pub force: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "force")]
    pub no_force: bool,
}

/// Arguments for `cloud provider update`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudProviderUpdateArgs {
    /// Name of the box.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Provider name.
    #[arg(index = 2)]
    pub provider: Option<String>,

    /// Version.
    #[arg(index = 3)]
    pub version: Option<String>,

    /// URL (optional).
    #[arg(index = 4)]
    pub url: Option<String>,

    /// Checksum.
    #[arg(short = 'c', long)]
    pub checksum: Option<String>,

    /// Checksum type.
    #[arg(short = 'C', long)]
    pub checksum_type: Option<String>,

    /// Architecture.
    #[arg(short = 'a', long)]
    pub architecture: Option<String>,

    /// Default architecture.
    #[arg(long)]
    pub default_architecture: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "default_architecture")]
    pub no_default_architecture: bool,
}

/// Arguments for `cloud provider upload`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudProviderUploadArgs {
    /// Name of the box.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Provider name.
    #[arg(index = 2)]
    pub provider: Option<String>,

    /// Version.
    #[arg(index = 3)]
    pub version: Option<String>,

    /// File path to upload.
    #[arg(index = 4)]
    pub file_path: Option<String>,

    /// Direct upload.
    #[arg(short = 'D', long)]
    pub direct: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "direct")]
    pub no_direct: bool,
}

/// Arguments for `cloud publish`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudPublishArgs {
    /// Name of the box to publish.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Version of the box.
    #[arg(index = 2)]
    pub version: Option<String>,

    /// Provider of the box.
    #[arg(index = 3)]
    pub provider: Option<String>,

    /// Path to the box file to publish.
    #[arg(index = 4)]
    pub file_path: Option<String>,

    /// Checksum of the box.
    #[arg(short = 'c', long)]
    pub checksum: Option<String>,

    /// Checksum type of the box.
    #[arg(short = 'C', long)]
    pub checksum_type: Option<String>,

    /// Architecture of the box.
    #[arg(short = 'a', long)]
    pub architecture: Option<String>,

    /// Description for the version.
    #[arg(long)]
    pub version_description: Option<String>,

    /// Description for the box.
    #[arg(short = 'd', long)]
    pub description: Option<String>,

    /// Short description for the box.
    #[arg(short = 's', long)]
    pub short_description: Option<String>,

    /// URL for the box.
    #[arg(long)]
    pub url: Option<String>,

    /// Make the box private.
    #[arg(short, long)]
    pub private: bool,

    /// Force deletion without confirmation.
    #[arg(short, long)]
    pub force: bool,

    /// Default architecture.
    #[arg(long)]
    pub default_architecture: bool,

    /// Release the version.
    #[arg(short, long)]
    pub release: bool,

    /// Direct upload.
    #[arg(long)]
    pub direct_upload: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "direct_upload")]
    pub no_direct_upload: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "private")]
    pub no_private: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "release")]
    pub no_release: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "force")]
    pub no_force: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "default_architecture")]
    pub no_default_architecture: bool,
}

/// Arguments for `cloud search`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudSearchArgs {
    /// The query to search for.
    #[arg(index = 1)]
    pub query: Option<String>,

    /// Output search results as JSON.
    #[arg(short = 'j', long)]
    pub json: bool,

    /// Short output format.
    #[arg(short = 's', long)]
    pub short: bool,

    /// Limit the number of search results.
    #[arg(short = 'l', long)]
    pub limit: Option<usize>,

    /// Sort results by field.
    #[arg(long)]
    pub sort_by: Option<String>,

    /// Order results.
    #[arg(short = 'o', long)]
    pub order: Option<String>,

    /// Page of results to fetch.
    #[arg(long)]
    pub page: Option<usize>,

    /// Filter by provider.
    #[arg(short = 'p', long)]
    pub provider: Option<String>,

    /// Filter by architecture.
    #[arg(short = 'a', long)]
    pub architecture: Option<String>,

    /// Authentication token.
    #[arg(long)]
    pub auth: Option<String>,

    /// Negates the flag.
    #[arg(long, overrides_with = "auth")]
    pub no_auth: bool,
}

/// Cloud version subcommands.
#[derive(Subcommand)]
pub enum CloudVersionCommands {
    /// Creates a cloud version.
    Create(CloudVersionCreateArgs),
    /// Deletes a cloud version.
    Delete(CloudVersionDeleteArgs),
    /// Releases a cloud version.
    Release(CloudVersionReleaseArgs),
    /// Revokes a cloud version.
    Revoke(CloudVersionRevokeArgs),
    /// Updates a cloud version.
    Update(CloudVersionUpdateArgs),
}

/// Arguments for `cloud version create`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudVersionCreateArgs {
    /// Name of the box.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Version string.
    #[arg(index = 2)]
    pub version: Option<String>,

    /// Description.
    #[arg(short, long)]
    pub description: Option<String>,
}

/// Arguments for `cloud version delete`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudVersionDeleteArgs {
    /// Name of the box.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Version string.
    #[arg(index = 2)]
    pub version: Option<String>,

    /// Force deletion without confirmation.
    #[arg(short, long)]
    pub force: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "force")]
    pub no_force: bool,
}

/// Arguments for `cloud version release`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudVersionReleaseArgs {
    /// Name of the box.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Version string.
    #[arg(index = 2)]
    pub version: Option<String>,

    /// Force release without confirmation.
    #[arg(short, long)]
    pub force: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "force")]
    pub no_force: bool,
}

/// Arguments for `cloud version revoke`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudVersionRevokeArgs {
    /// Name of the box.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Version string.
    #[arg(index = 2)]
    pub version: Option<String>,

    /// Force revoke without confirmation.
    #[arg(short, long)]
    pub force: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "force")]
    pub no_force: bool,
}

/// Arguments for `cloud version update`.
#[derive(Default, clap::Args, Clone)]
pub struct CloudVersionUpdateArgs {
    /// Name of the box.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Version string.
    #[arg(index = 2)]
    pub version: Option<String>,

    /// Description.
    #[arg(short, long)]
    pub description: Option<String>,
}

/// Arguments for `cap` command.
#[derive(Default, clap::Args, Clone)]
pub struct CapArgs {
    /// Name or ID of machine
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Name of capability
    #[arg(index = 2)]
    pub capability: Option<String>,

    /// Additional arguments
    #[arg(index = 3, num_args(0..))]
    pub extra_args: Vec<String>,

    /// Check if the capability is supported
    #[arg(long)]
    pub check: bool,

    /// Target guest
    #[arg(short = 't', long)]
    pub target_guest: Option<String>,
}

/// Arguments for `snapshot save`.
#[derive(Default, clap::Args, Clone)]
pub struct SnapshotSaveArgs {
    /// Name or ID of the environment (optional).
    #[arg(index = 1)]
    pub vm_name: Option<String>,

    /// Name of the snapshot.
    #[arg(index = 2)]
    pub name: Option<String>,

    /// Replace snapshot without confirmation.
    #[arg(short, long)]
    pub force: bool,
}

/// Arguments for `snapshot restore`.
#[derive(Default, clap::Args, Clone)]
pub struct SnapshotRestoreArgs {
    /// Name or ID of the environment (optional).
    #[arg(index = 1)]
    pub vm_name: Option<String>,

    /// Name of the snapshot to restore.
    #[arg(index = 2)]
    pub name: Option<String>,

    /// Enable or disable provisioning after restore.
    #[arg(long)]
    pub provision: Option<bool>,

    /// Enable only certain provisioners, by type or by name.
    #[arg(long)]
    pub provision_with: Option<String>,

    /// Don't start the machine after restore.
    #[arg(long)]
    pub no_start: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "provision")]
    pub no_provision: bool,
}

/// Arguments for `snapshot list`.
#[derive(Default, clap::Args, Clone)]
pub struct SnapshotListArgs {
    /// Name or ID of the environment (optional).
    #[arg(index = 1)]
    pub vm_name: Option<String>,
}

/// Arguments for `snapshot delete`.
#[derive(Default, clap::Args, Clone)]
pub struct SnapshotDeleteArgs {
    /// Name or ID of the environment (optional).
    #[arg(index = 1)]
    pub vm_name: Option<String>,

    /// Name of the snapshot to delete.
    #[arg(index = 2)]
    pub name: Option<String>,
}

/// Arguments for `snapshot pop`.
#[derive(Default, clap::Args, Clone)]
pub struct SnapshotPopArgs {
    /// Name or ID of the environment (optional).
    #[arg(index = 1)]
    pub vm_name: Option<String>,

    /// Do not delete the snapshot after popping.
    #[arg(long)]
    pub no_delete: bool,

    /// Enable or disable provisioning after pop.
    #[arg(long)]
    pub provision: Option<bool>,

    /// Enable only certain provisioners, by type or by name.
    #[arg(long)]
    pub provision_with: Option<String>,

    /// Don't start the machine after pop.
    #[arg(long)]
    pub no_start: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "provision")]
    pub no_provision: bool,
}

/// Arguments for `snapshot push`.
#[derive(Default, clap::Args, Clone)]
pub struct SnapshotPushArgs {
    /// Name or ID of the environment (optional).
    #[arg(index = 1)]
    pub vm_name: Option<String>,
}

/// Snapshot management subcommands.
#[derive(Subcommand)]
pub enum SnapshotCommands {
    /// Saves a snapshot.
    Save(SnapshotSaveArgs),
    /// Restores a snapshot.
    Restore(SnapshotRestoreArgs),
    /// Lists snapshots.
    List(SnapshotListArgs),
    /// Deletes a snapshot.
    Delete(SnapshotDeleteArgs),
    /// Pops a snapshot.
    Pop(SnapshotPopArgs),
    /// Pushes a snapshot.
    Push(SnapshotPushArgs),
}

/// Arguments for the `init` command.
#[derive(Default, clap::Args, Clone)]
pub struct InitArgs {
    /// Name of the box for the new environment.
    #[arg(index = 1)]
    pub box_name: Option<String>,

    /// Output path for the Vagrantfile.
    #[arg(long)]
    pub output: Option<String>,

    /// Version of the box to add.
    #[arg(long)]
    pub box_version: Option<String>,

    /// Force overwrite existing Vagrantfile.
    #[arg(short, long)]
    pub force: bool,

    /// Create a minimal Vagrantfile.
    #[arg(short, long)]
    pub minimal: bool,

    /// Template file to use.
    #[arg(short, long)]
    pub template: Option<String>,
}

/// Arguments for the `login` command.
#[derive(Default, clap::Args, Clone)]
pub struct LoginArgs {
    /// Check if currently logged in.
    #[arg(short, long)]
    pub check: bool,

    /// Username for login.
    #[arg(short, long)]
    pub username: Option<String>,

    /// Token for login.
    #[arg(short, long)]
    pub token: Option<String>,

    /// Description for the token.
    #[arg(short, long)]
    pub description: Option<String>,
}

/// Arguments for the `mutate` command.
#[derive(Default, clap::Args, Clone)]
pub struct MutateArgs {
    /// Name of the box to mutate.
    #[arg(index = 1)]
    pub box_name: Option<String>,

    /// Target provider.
    #[arg(index = 2)]
    pub destination_provider: Option<String>,

    /// Input provider.
    #[arg(long)]
    pub input_provider: Option<String>,

    /// Force virtio for the mutated box.
    #[arg(short = 'f', long)]
    pub force_virtio: bool,
}

/// Arguments for the `up` command.
#[derive(Default, clap::Args, Clone)]
pub struct UpArgs {
    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Enable provisioning.
    /// Negates the flag.
    #[arg(long, overrides_with = "no_provision")]
    pub provision: bool,

    /// Disable provisioning.
    /// Negates the flag.
    #[arg(long, overrides_with = "provision")]
    pub no_provision: bool,

    /// Enable only certain provisioners, by type or by name.
    #[arg(long)]
    pub provision_with: Option<String>,

    /// Back the machine with a specific provider.
    #[arg(long)]
    pub provider: Option<String>,

    /// Destroy machine if any fatal error happens.
    /// Negates the flag.
    #[arg(long, overrides_with = "no_destroy_on_error")]
    pub destroy_on_error: bool,

    /// Do not destroy machine if any fatal error happens.
    /// Negates the flag.
    #[arg(long, overrides_with = "destroy_on_error")]
    pub no_destroy_on_error: bool,

    /// Enable parallelism if provider supports it.
    /// Negates the flag.
    #[arg(long, overrides_with = "no_parallel")]
    pub parallel: bool,

    /// Disable parallelism.
    /// Negates the flag.
    #[arg(long, overrides_with = "parallel")]
    pub no_parallel: bool,

    /// If possible, install the provider if it isn't installed.
    /// Negates the flag.
    #[arg(long, overrides_with = "no_install_provider")]
    pub install_provider: bool,

    /// Do not install the provider.
    /// Negates the flag.
    #[arg(long, overrides_with = "install_provider")]
    pub no_install_provider: bool,
}

/// Arguments for the `destroy` command.
#[derive(Default, clap::Args, Clone)]
pub struct DestroyArgs {
    /// Destroy without confirmation.
    #[arg(short, long)]
    pub force: bool,

    /// Graceful shutdown.
    #[arg(short, long)]
    pub graceful: bool,

    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Enable parallelism if provider supports it.
    /// Negates the flag.
    #[arg(long, overrides_with = "no_parallel")]
    pub parallel: bool,

    /// Disable parallelism.
    /// Negates the flag.
    #[arg(long, overrides_with = "parallel")]
    pub no_parallel: bool,
}

/// Arguments for the `halt` command.
#[derive(Default, clap::Args, Clone)]
pub struct HaltArgs {
    /// Force shut down (equivalent to pulling power).
    #[arg(short, long)]
    pub force: bool,

    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Enable parallelism if provider supports it.
    #[arg(long, overrides_with = "no_parallel")]
    pub parallel: bool,

    /// Disable parallelism.
    #[arg(long)]
    pub no_parallel: bool,
}

/// Arguments for the `suspend` command.
#[derive(Default, clap::Args, Clone)]
pub struct SuspendArgs {
    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Suspend all global machines.
    #[arg(short = 'a', long = "all-global")]
    pub all_global: bool,
}

/// Arguments for the `upload` command.
#[derive(Default, clap::Args, Clone)]
pub struct UploadArgs {
    /// Source file or directory.
    #[arg(index = 1)]
    pub source: Option<String>,

    /// Destination file or directory.
    #[arg(index = 2)]
    pub destination: Option<String>,

    /// Use temporary file.
    #[arg(short, long)]
    pub temporary: bool,

    /// Compress file before upload.
    #[arg(short = 'c', long)]
    pub compress: bool,

    /// Compression type.
    #[arg(short = 'C', long)]
    pub compression_type: Option<String>,
}

/// Arguments for the `validate` command.
#[derive(Default, clap::Args, Clone)]
pub struct ValidateArgs {
    /// Ignore provider validation.
    #[arg(short = 'p', long)]
    pub ignore_provider: bool,
}

/// Arguments for the `winrm` command.
#[derive(Default, clap::Args, Clone)]
pub struct WinrmArgs {
    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Run an interactive shell.
    #[arg(short, long)]
    pub shell: bool,

    /// Execute command with elevated privileges.
    #[arg(short, long)]
    pub elevated: bool,

    /// Command to execute.
    #[arg(short, long)]
    pub command: Option<String>,
}

/// Arguments for the `winrm-config` command.
#[derive(Default, clap::Args, Clone)]
pub struct WinrmConfigArgs {
    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Host string.
    #[arg(long)]
    pub host: Option<String>,
}

/// Arguments for the `resume` command.
#[derive(Default, clap::Args, Clone)]
pub struct ResumeArgs {
    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Enable or disable provisioning.
    #[arg(long)]
    pub provision: Option<bool>,

    /// Enable only certain provisioners, by type or by name.
    #[arg(long)]
    pub provision_with: Option<String>,

    /// Negates the flag.
    #[arg(long, overrides_with = "provision")]
    pub no_provision: bool,
}

/// Arguments for the `reload` command.
#[derive(Default, clap::Args, Clone)]
pub struct ReloadArgs {
    /// Force shut down.
    #[arg(short, long)]
    pub force: bool,

    /// Enable or disable provisioning.
    #[arg(long)]
    pub provision: Option<bool>,

    /// Enable only certain provisioners, by type or by name.
    #[arg(long)]
    pub provision_with: Option<String>,

    /// Negates the flag.
    #[arg(long, overrides_with = "provision")]
    pub no_provision: bool,

    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,
}

/// Arguments for the `port` command.
#[derive(Default, clap::Args, Clone)]
pub struct PortArgs {
    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// The guest port to look up.
    #[arg(long)]
    pub guest: Option<u16>,
}

/// Arguments for the `rdp` command.
#[derive(Default, clap::Args, Clone)]
pub struct RdpArgs {
    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,
}

/// Arguments for the `powershell` command.
#[derive(Default, clap::Args, Clone)]
pub struct PowershellArgs {
    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Execute command.
    #[arg(short, long)]
    pub command: Option<String>,

    /// Execute command with elevated privileges.
    #[arg(short, long)]
    pub elevated: bool,
}

/// Arguments for the `provider` command.
#[derive(Default, clap::Args, Clone)]
pub struct ProviderArgs {
    /// Show only usable providers.
    #[arg(long)]
    pub usable: bool,

    /// Ensure provider is installed.
    #[arg(long)]
    pub install: bool,
}

/// Arguments for the `provision` command.
#[derive(Default, clap::Args, Clone)]
pub struct ProvisionArgs {
    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Enable only certain provisioners, by type or by name.
    #[arg(long)]
    pub provision_with: Option<String>,

    /// Enable parallelism if provider supports it.
    #[arg(long, overrides_with = "no_parallel")]
    pub parallel: bool,

    /// Disable parallelism.
    #[arg(long)]
    pub no_parallel: bool,
}

/// Arguments for the `rsync` command.
#[derive(Default, clap::Args, Clone)]
pub struct RsyncArgs {
    /// Change ownership of the uploaded files.
    #[arg(long)]
    pub rsync_chown: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "rsync_chown")]
    pub no_rsync_chown: bool,
}

/// Arguments for the `rsync-auto` command.
#[derive(Default, clap::Args, Clone)]
pub struct RsyncAutoArgs {
    /// Change ownership of the uploaded files.
    #[arg(long)]
    pub rsync_chown: bool,

    /// Poll for changes.
    #[arg(long)]
    pub poll: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "rsync_chown")]
    pub no_rsync_chown: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "poll")]
    pub no_poll: bool,
}

/// Arguments for the `help` command.
#[derive(Default, clap::Args, Clone)]
pub struct HelpArgs {
    /// The primary subcommand to print help for.
    #[arg(index = 1)]
    pub subcommand: Option<String>,

    /// Additional nested subcommands to print help for (e.g. `box add`).
    #[arg(index = 2, trailing_var_arg = true, num_args = 0..)]
    pub subcommands: Vec<String>,
}

impl HelpArgs {
    /// Returns the full list of subcommand path components requested for help.
    pub fn full_path(&self) -> Vec<String> {
        let mut path = Vec::new();
        if let Some(ref first) = self.subcommand {
            path.push(first.clone());
            path.extend(self.subcommands.clone());
        }
        path
    }
}

/// Arguments for the `ssh` command.
#[derive(Default, clap::Args, Clone)]
pub struct SshArgs {
    /// Machine name or ID.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Execute an SSH command directly.
    #[arg(short, long)]
    pub command: Option<String>,

    /// Plain mode, leaves authentication to user.
    #[arg(short, long)]
    pub plain: bool,

    /// Pass extra arguments to OpenSSH.
    #[arg(short = 'e', long = "extra-args")]
    pub extra_args: Option<String>,

    /// Allocate a pseudo-TTY.
    #[arg(short, long)]
    pub tty: bool,
}

/// Arguments for the `ssh-config` command.
#[derive(Default, clap::Args, Clone)]
pub struct SshConfigArgs {
    /// Name or ID of machine
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Host string
    #[arg(long)]
    pub host: Option<String>,
}

/// Arguments for the `docker-exec` command.
#[derive(Default, clap::Args, Clone)]
pub struct DockerExecArgs {
    /// Name or ID of the environment.
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Command to execute.
    #[arg(index = 2)]
    pub command: Option<String>,

    /// Additional arguments for the command.
    #[arg(index = 3, num_args(0..))]
    pub args: Vec<String>,

    /// Name or ID of the container user.
    #[arg(short, long)]
    pub user: Option<String>,

    /// Keep STDIN open even if not attached.
    #[arg(short, long)]
    pub interactive: bool,

    /// Allocate a pseudo-TTY.
    #[arg(short, long)]
    pub tty: bool,

    /// Detached mode: run command in the background.
    #[arg(short, long)]
    pub detach: bool,

    /// Prefix output with machine names.
    #[arg(short, long)]
    pub prefix: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "interactive")]
    pub no_interactive: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "detach")]
    pub no_detach: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "prefix")]
    pub no_prefix: bool,
}

/// Arguments for the `docker-logs` command.
#[derive(Default, clap::Args, Clone)]
pub struct DockerLogsArgs {
    /// Follow log output.
    #[arg(short, long)]
    pub follow: bool,

    /// Number of lines to show from the end of the logs.
    #[arg(short, long)]
    pub tail: Option<String>,

    /// Show timestamps.
    #[arg(long)]
    pub timestamps: bool,

    /// Prefix output with machine names.
    #[arg(short, long)]
    pub prefix: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "follow")]
    pub no_follow: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "prefix")]
    pub no_prefix: bool,
}

/// Arguments for the `docker-run` command.
#[derive(Default, clap::Args, Clone)]
pub struct DockerRunArgs {
    /// Command to run.
    #[arg(index = 1)]
    pub command: Option<String>,

    /// Command arguments.
    #[arg(index = 2, num_args(0..))]
    pub args: Vec<String>,

    /// Automatically remove the container when it exits.
    #[arg(short = 'r', long)]
    pub rm: bool,

    /// Detached mode: run command in the background.
    #[arg(short, long)]
    pub detach: bool,

    /// Allocate a pseudo-TTY.
    #[arg(short, long)]
    pub tty: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "detach")]
    pub no_detach: bool,

    /// Negates the flag.
    #[arg(long, overrides_with = "rm")]
    pub no_rm: bool,
}

/// Arguments for the `global-status` command.
#[derive(Default, clap::Args, Clone)]
pub struct GlobalStatusArgs {
    /// Prune invalid entries from the index.
    #[arg(short, long)]
    pub prune: bool,
}

/// Arguments for the `status` command.
#[derive(Default, clap::Args, Clone)]
pub struct StatusArgs {
    /// The name or ID of the machine to status.
    #[arg(index = 1)]
    pub name: Option<String>,
}

/// Arguments for the `package` command.
#[derive(Default, clap::Args, Clone)]
pub struct PackageArgs {
    /// Name of a VM in virtualbox to package as a base box (VirtualBox only).
    #[arg(long)]
    pub base: Option<String>,

    /// Name of the file to output.
    #[arg(long)]
    pub output: Option<String>,

    /// Additional files to package with the box.
    #[arg(long, value_delimiter = ',')]
    pub include: Option<Vec<String>>,

    /// Vagrantfile to package with the box.
    #[arg(long)]
    pub vagrantfile: Option<String>,

    /// Show packaging information.
    #[arg(long)]
    pub info: bool,

    /// Name or ID of the environment.
    #[arg(index = 1)]
    pub name: Option<String>,
}

/// Arguments for `box add`.
#[derive(Default, clap::Args, Clone)]
pub struct BoxAddArgs {
    /// Box name, url, or path.
    #[arg(index = 1)]
    pub name: String,

    /// Name for the added box.
    #[arg(long = "name")]
    pub box_name: Option<String>,

    /// Force overwrite an existing box.
    #[arg(short, long)]
    pub force: bool,

    /// Clean any temporary download files.
    #[arg(short, long)]
    pub clean: bool,

    /// Provider the box should satisfy.
    #[arg(long)]
    pub provider: Option<String>,

    /// Version of the box to add.
    #[arg(long)]
    pub box_version: Option<String>,

    /// Checksum for the box.
    #[arg(long)]
    pub checksum: Option<String>,

    /// Checksum type for the box.
    #[arg(long)]
    pub checksum_type: Option<String>,

    /// Allow insecure downloads.
    #[arg(long)]
    pub insecure: bool,

    /// Architecture of the box.
    #[arg(short = 'a', long)]
    pub architecture: Option<String>,

    /// CA cert for the download.
    #[arg(long)]
    pub cacert: Option<String>,

    /// CA path for the download.
    #[arg(long)]
    pub capath: Option<String>,

    /// Client cert for the download.
    #[arg(long)]
    pub cert: Option<String>,

    /// Trust the location for the download.
    #[arg(long)]
    pub location_trusted: bool,
}

/// Arguments for `box list`.
#[derive(Default, clap::Args, Clone)]
pub struct BoxListArgs {
    /// Display more information about the boxes.
    #[arg(short = 'i', long)]
    pub box_info: bool,
}

/// Arguments for `box remove`.
#[derive(Default, clap::Args, Clone)]
pub struct BoxRemoveArgs {
    /// Name of the box to remove.
    #[arg(index = 1)]
    pub name: String,

    /// Provider of the box to remove.
    #[arg(long)]
    pub provider: Option<String>,

    /// Version of the box to remove.
    #[arg(long)]
    pub box_version: Option<String>,

    /// Architecture of the box to remove.
    #[arg(short = 'a', long)]
    pub architecture: Option<String>,

    /// Remove all versions of the box.
    #[arg(long)]
    pub all: bool,

    /// Remove for all providers.
    #[arg(long)]
    pub all_providers: bool,

    /// Remove for all architectures.
    #[arg(long)]
    pub all_architectures: bool,

    /// Remove without confirmation.
    #[arg(short, long)]
    pub force: bool,
}

/// Arguments for `box outdated`.
#[derive(Default, clap::Args, Clone)]
pub struct BoxOutdatedArgs {
    /// Check for outdated boxes globally.
    #[arg(short = 'g', long)]
    pub global: bool,

    /// Allow insecure connections.
    #[arg(long)]
    pub insecure: bool,

    /// CA cert for the download.
    #[arg(long)]
    pub cacert: Option<String>,

    /// CA path for the download.
    #[arg(long)]
    pub capath: Option<String>,

    /// Client cert for the download.
    #[arg(long)]
    pub cert: Option<String>,

    /// Force check.
    #[arg(short, long)]
    pub force: bool,
}

/// Arguments for `box update`.
#[derive(Default, clap::Args, Clone)]
pub struct BoxUpdateArgs {
    /// Name of the box to update.
    #[arg(long = "box")]
    pub box_name: Option<String>,

    /// Provider of the box to update.
    #[arg(long)]
    pub provider: Option<String>,

    /// Architecture of the box.
    #[arg(long)]
    pub architecture: Option<String>,

    /// Force overwrite.
    #[arg(short, long)]
    pub force: bool,

    /// Allow insecure connections.
    #[arg(long)]
    pub insecure: bool,

    /// CA cert for the download.
    #[arg(long)]
    pub cacert: Option<String>,

    /// CA path for the download.
    #[arg(long)]
    pub capath: Option<String>,

    /// Client cert for the download.
    #[arg(long)]
    pub cert: Option<String>,
}

/// Arguments for `box prune`.
#[derive(Default, clap::Args, Clone)]
pub struct BoxPruneArgs {
    /// Name of the box.
    #[arg(long)]
    pub name: Option<String>,

    /// The specific provider to prune.
    #[arg(short, long)]
    pub provider: Option<String>,

    /// Force prune.
    #[arg(short, long)]
    pub force: bool,

    /// Only print the boxes that would be removed.
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Keep boxes that are currently in use.
    #[arg(short, long, default_value_t = true)]
    pub keep_active_boxes: bool,
}

/// Arguments for `box repackage`.
#[derive(Default, clap::Args, Clone)]
pub struct BoxRepackageArgs {
    /// Name of the box to repackage.
    #[arg(index = 1)]
    pub name: String,

    /// Provider of the box.
    #[arg(index = 2)]
    pub provider: String,

    /// Version of the box.
    #[arg(index = 3)]
    pub version: String,
}

/// Box management subcommands.
#[derive(Subcommand)]
pub enum BoxCommands {
    /// Adds a box.
    Add(BoxAddArgs),
    /// Lists all installed boxes.
    List(BoxListArgs),
    /// Removes a box.
    Remove(BoxRemoveArgs),
    /// Checks for updates for the box.
    Outdated(BoxOutdatedArgs),
    /// Updates the box.
    Update(BoxUpdateArgs),
    /// Prunes old versions of boxes.
    Prune(BoxPruneArgs),
    /// Repackages an installed box.
    Repackage(BoxRepackageArgs),
}

/// Arguments for `plugin install`.
#[derive(Default, clap::Args, Clone)]
pub struct PluginInstallArgs {
    /// Name of the plugin to install.
    #[arg(index = 1)]
    pub name: String,

    /// Add a plugin source.
    #[arg(short = 's', long)]
    pub plugin_source: Option<String>,

    /// Install a specific version.
    #[arg(long)]
    pub plugin_version: Option<String>,

    /// Clean plugin sources.
    #[arg(long)]
    pub plugin_clean_sources: bool,

    /// Entry point for the plugin.
    #[arg(short = 'e', long)]
    pub entry_point: Option<String>,

    /// Verbose output.
    #[arg(short = 'V', long)]
    pub verbose: bool,

    /// Install for local project only.
    #[arg(short = 'l', long)]
    pub local: bool,
}

/// Arguments for `plugin list`.
#[derive(Default, clap::Args, Clone)]
pub struct PluginListArgs {
    /// List local project plugins only.
    #[arg(long)]
    pub local: bool,
}

/// Arguments for `plugin uninstall`.
#[derive(Default, clap::Args, Clone)]
pub struct PluginUninstallArgs {
    /// Name of the plugin to uninstall.
    #[arg(index = 1)]
    pub name: String,

    /// Uninstall for local project only.
    #[arg(short = 'l', long)]
    pub local: bool,
}

/// Arguments for `plugin expunge`.
#[derive(Default, clap::Args, Clone)]
pub struct PluginExpungeArgs {
    /// Expunge without confirmation.
    #[arg(short, long)]
    pub force: bool,

    /// Reinstall global plugins after expunge.
    #[arg(short = 'r', long)]
    pub reinstall: bool,

    /// Expunge local project plugins only.
    #[arg(short = 'l', long)]
    pub local: bool,

    /// Alias for local.
    #[arg(long)]
    pub local_only: bool,

    /// Expunge global plugins only.
    #[arg(short = 'g', long)]
    pub global_only: bool,
}

/// Arguments for `plugin license`.
#[derive(Default, clap::Args, Clone)]
pub struct PluginLicenseArgs {
    /// Name of the plugin.
    #[arg(index = 1)]
    pub name: String,

    /// Path to the license file.
    #[arg(index = 2)]
    pub license_file: String,
}

/// Arguments for `plugin repair`.
#[derive(Default, clap::Args, Clone)]
pub struct PluginRepairArgs {
    /// Repair local project plugins only.
    #[arg(long)]
    pub local: bool,
}

/// Arguments for `plugin update`.
#[derive(Default, clap::Args, Clone)]
pub struct PluginUpdateArgs {
    /// Name of the plugin to update (optional).
    #[arg(index = 1)]
    pub name: Option<String>,

    /// Update for local project only.
    #[arg(long)]
    pub local: bool,
}

/// Plugin management subcommands.
#[derive(Subcommand)]
pub enum PluginCommands {
    /// Installs a plugin.
    Install(PluginInstallArgs),
    /// Lists installed plugins.
    List(PluginListArgs),
    /// Uninstalls a plugin.
    Uninstall(PluginUninstallArgs),
    /// Completely removes a plugin and its configuration.
    Expunge(PluginExpungeArgs),
    /// Manages plugin licenses.
    License(PluginLicenseArgs),
    /// Repairs a plugin.
    Repair(PluginRepairArgs),
    /// Updates a plugin.
    Update(PluginUpdateArgs),
}

/// Parses the CLI arguments.
///
/// # Returns
///
/// Returns the parsed `Cli` arguments.
#[coverage(off)]
pub fn parse() -> Cli {
    Cli::parse()
}

/// Parses the CLI arguments from an explicit iterator.
///
/// # Arguments
///
/// * `args` - An iterator of string arguments.
///
/// # Returns
///
/// Returns the parsed `Cli` arguments.
///
/// # Errors
///
/// Returns a `clap::Error` if parsing fails.
pub fn parse_from<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Cli::try_parse_from(args)
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_parse_from_success() {
        let args = vec!["migratory", "status"];
        let cli = parse_from(args).expect("Failed to parse args");
        assert!(matches!(cli.command, Some(Commands::Status(_))));
    }

    #[test]
    fn test_parse_from_global_flags() {
        let args = vec!["migratory", "--debug", "--no-tty", "status"];
        let cli = parse_from(args).expect("Failed to parse args");
        assert!(cli.debug);
        assert!(cli.no_tty);
        assert!(matches!(cli.command, Some(Commands::Status(_))));
    }

    #[test]
    fn test_parse_version_flag() {
        let args = vec!["migratory", "-v"];
        let cli = parse_from(args).expect("Failed to parse args");
        assert!(cli.version);
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_parse_from_failure() {
        let args = vec!["migratory", "nonexistent-command"];
        let result = parse_from(args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_coverage() {
        // We cannot call `parse()` because clap's `parse()` calls `std::process::exit()` on error,
        // which kills the test runner. We test `parse_from` instead, which is sufficient.
    }
}
