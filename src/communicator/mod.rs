//! Communicator module for guest interactions (SSH, WinRM).

use crate::error::MigratoryError;
use std::path::Path;
use std::time::Duration;

pub mod docker;
pub mod ssh;
pub mod winrm;

pub use docker::DockerCommunicator;
pub use ssh::SshCommunicator;
pub use winrm::WinrmCommunicator;

/// Core interface for communicating with a guest VM.
pub trait Communicator {
    /// Executes a command on the guest.
    fn execute(&self, command: &str) -> Result<String, MigratoryError>;

    /// Uploads a file from the host to the guest.
    fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), MigratoryError>;

    /// Downloads a file from the guest to the host.
    fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), MigratoryError>;

    /// Starts an interactive session on the guest.
    fn execute_interactive(&self) -> Result<(), MigratoryError>;

    /// Blocks until the communicator is ready or the timeout is reached.
    fn wait_for_ready(&self, timeout: Duration) -> Result<(), MigratoryError>;
}
