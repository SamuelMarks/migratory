//! Provider module for hypervisor interactions.
//!
//! This module defines the core `Provider` trait which must be implemented
//! by all supported virtual machine hypervisors.

use crate::config::VmConfig;
use crate::error::MigratoryError;

/// Docker container provider implementation.
pub mod docker;
/// Microsoft Hyper-V provider implementation.
pub mod hyperv;
/// Libvirt / KVM provider implementation.
pub mod libvirt;
/// Parallels Desktop provider implementation.
pub mod parallels;
/// QEMU / libvirt provider implementation.
pub mod qemu;
/// UTM (macOS / Apple Silicon) provider implementation.
pub mod utm;
/// Oracle VirtualBox provider implementation.
pub mod virtualbox;
/// VMware Fusion / Workstation provider implementation.
pub mod vmware;

/// Abstraction over external process execution for hypervisor CLI tools.
pub trait CommandRunner: Send + Sync {
    /// Executes `program` with `args` and returns its captured standard output.
    ///
    /// # Arguments
    ///
    /// * `program` - Executable name or path.
    /// * `args` - Slice of command-line arguments.
    ///
    /// # Returns
    ///
    /// Returns captured stdout on zero exit status.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if spawning fails or the process exits non-zero.
    fn run(&self, program: &str, args: &[&str]) -> Result<String, MigratoryError>;

    /// Checks whether `program` can be spawned with `check_arg`.
    ///
    /// # Arguments
    ///
    /// * `program` - Executable name or path.
    /// * `check_arg` - Version or help flag (e.g. `"--version"`).
    ///
    /// # Returns
    ///
    /// Returns `true` if the command could be spawned, `false` otherwise.
    fn is_available(&self, program: &str, check_arg: &str) -> bool {
        std::process::Command::new(program)
            .arg(check_arg)
            .output()
            .is_ok()
    }
}

/// Default [`CommandRunner`] that spawns real operating system processes via [`std::process::Command`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, MigratoryError> {
        let output = std::process::Command::new(program).args(args).output();
        match output {
            Ok(out) => {
                if out.status.success() {
                    Ok(String::from_utf8_lossy(&out.stdout).to_string())
                } else {
                    Err(MigratoryError::Generic(
                        String::from_utf8_lossy(&out.stderr).to_string(),
                    ))
                }
            }
            Err(e) => Err(MigratoryError::Generic(e.to_string())),
        }
    }
}

/// Closure-backed [`CommandRunner`] for dependency injection in tests and custom runners.
pub struct FnCommandRunner<F>
where
    F: Fn(&str, &[&str]) -> Result<String, MigratoryError> + Send + Sync,
{
    /// Handler invoked on [`CommandRunner::run`].
    handler: F,
    /// Availability flag returned by [`CommandRunner::is_available`].
    available: bool,
}

impl<F> FnCommandRunner<F>
where
    F: Fn(&str, &[&str]) -> Result<String, MigratoryError> + Send + Sync,
{
    /// Creates a new [`FnCommandRunner`] with `available = true`.
    ///
    /// # Arguments
    ///
    /// * `handler` - Closure invoked for each command execution.
    ///
    /// # Returns
    ///
    /// Returns a new [`FnCommandRunner`].
    pub fn new(handler: F) -> Self {
        Self {
            handler,
            available: true,
        }
    }

    /// Creates a new [`FnCommandRunner`] with an explicit `available` flag.
    ///
    /// # Arguments
    ///
    /// * `available` - Value returned by `is_available`.
    /// * `handler` - Closure invoked for each command execution.
    ///
    /// # Returns
    ///
    /// Returns a new [`FnCommandRunner`].
    pub fn with_availability(available: bool, handler: F) -> Self {
        Self { handler, available }
    }
}

impl<F> CommandRunner for FnCommandRunner<F>
where
    F: Fn(&str, &[&str]) -> Result<String, MigratoryError> + Send + Sync,
{
    fn run(&self, program: &str, args: &[&str]) -> Result<String, MigratoryError> {
        (self.handler)(program, args)
    }

    fn is_available(&self, _program: &str, _check_arg: &str) -> bool {
        self.available
    }
}

/// Strongly-typed virtual machine runtime state returned by [`Provider::status`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum MachineState {
    /// Machine has not been created yet (`"not created"`).
    #[default]
    NotCreated,
    /// Machine is currently running (`"running"`).
    Running,
    /// Machine is powered off (`"poweroff"`).
    PowerOff,
    /// Machine state is saved to disk (`"saved"`).
    Saved,
    /// Machine is suspended or paused in memory (`"suspended"`).
    Suspended,
    /// Machine exited abnormally (`"aborted"`).
    Aborted,
    /// Machine is stopped (`"stopped"`).
    Stopped,
    /// Unrecognized or provider-specific raw state string.
    Unknown(String),
}

impl MachineState {
    /// Parses a raw status string into a [`MachineState`].
    ///
    /// # Arguments
    ///
    /// * `s` - Raw status string reported by a hypervisor CLI.
    ///
    /// # Returns
    ///
    /// Returns the corresponding [`MachineState`] variant.
    pub fn from_raw(s: &str) -> Self {
        let trimmed = s.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "not created" | "not_created" => Self::NotCreated,
            "running" | "started" => Self::Running,
            "poweroff" | "powered off" | "shut off" => Self::PowerOff,
            "saved" => Self::Saved,
            "suspended" => Self::Suspended,
            "aborted" => Self::Aborted,
            "stopped" | "off" => Self::Stopped,
            "" | "unknown" => Self::Unknown("unknown".to_string()),
            _ => Self::Unknown(trimmed.to_string()),
        }
    }

    /// Returns the human-readable state string.
    ///
    /// # Returns
    ///
    /// String slice representing the state (e.g., `"running"`, `"not created"`).
    pub fn as_str(&self) -> &str {
        match self {
            Self::NotCreated => "not created",
            Self::Running => "running",
            Self::PowerOff => "poweroff",
            Self::Saved => "saved",
            Self::Suspended => "suspended",
            Self::Aborted => "aborted",
            Self::Stopped => "stopped",
            Self::Unknown(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for MachineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl PartialEq<&str> for MachineState {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for MachineState {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

/// Core provider interface.
///
/// Any supported hypervisor (VirtualBox, QEMU, Hyper-V, VMware) must
/// implement this trait to manage VM lifecycles.
pub trait Provider {
    /// Name of the provider.
    ///
    /// # Returns
    ///
    /// A string slice containing the provider's canonical name.
    fn name(&self) -> &str;

    /// Brings the machine up.
    ///
    /// # Arguments
    ///
    /// * `config` - The virtual machine configuration settings.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if bringing up the VM fails.
    fn up(&self, _config: &VmConfig) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Halts the machine gracefully.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if halting the VM fails.
    fn halt(&self) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Imports a machine from a base box.
    ///
    /// # Arguments
    ///
    /// * `box_dir` - The directory containing the unpacked box.
    /// * `vm_name` - The desired name for the newly created VM.
    ///
    /// # Returns
    ///
    /// Returns `Ok(String)` containing the new machine ID on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if importing fails.
    fn import(&self, _box_dir: &std::path::Path, _vm_name: &str) -> Result<String, MigratoryError> {
        Ok(String::new())
    }

    /// Clones an existing machine (Linked Clone).
    ///
    /// # Arguments
    ///
    /// * `base_machine_id` - The ID of the machine to clone.
    /// * `vm_name` - The desired name for the newly created VM.
    ///
    /// # Returns
    ///
    /// Returns `Ok(String)` containing the new machine ID on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if cloning fails.
    fn clone_machine(
        &self,
        _base_machine_id: &str,
        _vm_name: &str,
    ) -> Result<String, MigratoryError> {
        Ok(String::new())
    }

    /// Destroys the machine, removing all associated resources.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if destroying the VM fails.
    fn destroy(&self) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Retrieves the current state of the machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(MachineState)` representing the state (e.g., [`MachineState::Running`], [`MachineState::PowerOff`], [`MachineState::NotCreated`]).
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if querying the state fails.
    fn status(&self) -> Result<MachineState, MigratoryError> {
        Ok(MachineState::NotCreated)
    }

    /// Suspends the machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if suspending the VM fails.
    fn suspend(&self) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Resumes the suspended machine.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if resuming the VM fails.
    fn resume(&self) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Mounts synced folders configured for the machine.
    ///
    /// # Arguments
    ///
    /// * `config` - The VM configuration.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if mounting fails.
    fn setup_synced_folders(&self, _config: &VmConfig) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Saves a snapshot of the machine.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the snapshot.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if saving fails.
    fn snapshot_save(&self, _name: &str) -> Result<(), MigratoryError> {
        Err(MigratoryError::Generic(format!(
            "Snapshot save not supported by provider: {}",
            self.name()
        )))
    }

    /// Restores a snapshot of the machine.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the snapshot to restore.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if restoring fails.
    fn snapshot_restore(&self, _name: &str) -> Result<(), MigratoryError> {
        Err(MigratoryError::Generic(format!(
            "Snapshot restore not supported by provider: {}",
            self.name()
        )))
    }

    /// Lists snapshots of the machine.
    ///
    /// # Returns
    ///
    /// Returns a list of snapshot names on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if listing fails.
    fn snapshot_list(&self) -> Result<Vec<String>, MigratoryError> {
        Err(MigratoryError::Generic(format!(
            "Snapshot list not supported by provider: {}",
            self.name()
        )))
    }

    /// Deletes a snapshot of the machine.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the snapshot to delete.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if deletion fails.
    fn snapshot_delete(&self, _name: &str) -> Result<(), MigratoryError> {
        Err(MigratoryError::Generic(format!(
            "Snapshot delete not supported by provider: {}",
            self.name()
        )))
    }

    /// Exports the machine to the specified directory.
    ///
    /// The export pipeline produces provider-specific virtual machine artifacts suitable
    /// for packaging into a `.box` archive:
    /// - VirtualBox: exports an OVF appliance (`box.ovf`) and disk images (`.vmdk`).
    /// - QEMU/Libvirt: dumps domain definition (`box.xml`) and disk images (`box.img`).
    /// - Hyper-V: exports virtual machine definitions (`Virtual Machines/`) and virtual hard disks (`.vhdx`).
    /// - VMware: exports virtual machine configuration (`box.vmx`) and disk files (`box.vmdk`).
    /// - Docker: commits container state and saves image tarball (`box.tar`).
    ///
    /// The packaging step subsequently writes `metadata.json` and optional embedded `Vagrantfile`
    /// into this directory before compressing the tree into the target box file.
    ///
    /// # Arguments
    ///
    /// * `output_dir` - The directory where the OVF/box files should be exported.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if exporting fails or is not supported.
    fn export(&self, _output_dir: &std::path::Path) -> Result<(), MigratoryError> {
        Err(MigratoryError::Generic(format!(
            "Exporting not supported by provider: {}",
            self.name()
        )))
    }
}

/// Returns an instantiated provider based on its name or [`crate::config::ProviderKind`].
///
/// # Arguments
///
/// * `name` - The name or [`crate::config::ProviderKind`] of the provider.
/// * `machine_id` - The stored ID of the machine, if any.
///
/// # Returns
///
/// Returns a boxed provider implementation.
///
/// # Errors
///
/// Returns a `MigratoryError` if the provider name is unknown.
pub fn get_provider(
    name: impl AsRef<str>,
    machine_id: Option<String>,
) -> Result<Box<dyn Provider>, MigratoryError> {
    let raw_name = name.as_ref();
    let reg = crate::plugin::global_registry()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(prov) = reg.get_provider(raw_name, machine_id.clone()) {
        return Ok(prov);
    }
    match crate::config::ProviderKind::parse_raw(raw_name) {
        crate::config::ProviderKind::VirtualBox => {
            Ok(Box::new(virtualbox::VirtualBoxProvider::new(machine_id)))
        }
        crate::config::ProviderKind::Qemu => Ok(Box::new(qemu::QemuProvider::new(machine_id))),
        crate::config::ProviderKind::Hyperv => {
            Ok(Box::new(hyperv::HypervProvider::new(machine_id)))
        }
        crate::config::ProviderKind::Vmware => {
            Ok(Box::new(vmware::VmwareProvider::new(machine_id)))
        }
        crate::config::ProviderKind::Docker => {
            Ok(Box::new(docker::DockerProvider::new(machine_id)))
        }
        crate::config::ProviderKind::Utm => Ok(Box::new(utm::UtmProvider::new(machine_id))),
        crate::config::ProviderKind::Libvirt => {
            Ok(Box::new(libvirt::LibvirtProvider::new(machine_id)))
        }
        crate::config::ProviderKind::Parallels => {
            Ok(Box::new(parallels::ParallelsProvider::new(machine_id)))
        }
        crate::config::ProviderKind::Custom(unknown) => Err(MigratoryError::Generic(format!(
            "Unknown provider: {}",
            unknown
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal provider implementation used to test default [`Provider`] trait methods.
    struct DefaultProvider;
    impl Provider for DefaultProvider {
        fn name(&self) -> &str {
            "default"
        }
    }

    #[test]
    fn test_provider_default_methods() {
        let p = DefaultProvider;

        let cfg = crate::config::VmConfig::default();
        assert!(p.up(&cfg).is_ok());
        assert!(p.halt().is_ok());
        assert_eq!(
            p.import(std::path::Path::new("."), "vm").ok(),
            Some(String::new())
        );
        assert_eq!(p.clone_machine("base", "vm").ok(), Some(String::new()));
        assert!(p.destroy().is_ok());
        assert_eq!(p.status().ok(), Some(MachineState::NotCreated));
        assert!(p.suspend().is_ok());
        assert!(p.resume().is_ok());
        assert!(p.setup_synced_folders(&cfg).is_ok());

        assert!(p.snapshot_save("test").is_err());
        assert!(p.snapshot_restore("test").is_err());
        assert!(p.snapshot_list().is_err());
        assert!(p.snapshot_delete("test").is_err());
        assert!(p.export(std::path::Path::new("/dummy")).is_err());
        assert_eq!(
            MachineState::from_raw("not_created"),
            MachineState::NotCreated
        );
        let running_str = "running".to_string();
        assert!(MachineState::Running == running_str);
    }

    #[test]
    fn test_get_provider() {
        assert!(get_provider("virtualbox", None).is_ok());
        assert!(get_provider("qemu", None).is_ok());
        assert!(get_provider("hyperv", None).is_ok());
        assert!(get_provider("vmware", None).is_ok());
        assert!(get_provider("vmware_desktop", None).is_ok());
        assert!(get_provider("docker", None).is_ok());
        assert!(get_provider("utm", None).is_ok());
        assert!(get_provider("libvirt", None).is_ok());
        assert!(get_provider("parallels", None).is_ok());
        assert!(get_provider("unknown_provider", None).is_err());
    }
}
