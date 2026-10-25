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
pub trait Communicator: Send + Sync {
    /// Executes a command on the guest.
    fn execute(&self, command: &str) -> Result<String, MigratoryError>;

    /// Uploads a file from the host to the guest.
    fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Downloads a file from the guest to the host.
    fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Starts an interactive session on the guest.
    fn execute_interactive(&self) -> Result<(), MigratoryError> {
        Ok(())
    }

    /// Blocks until the communicator is ready or the timeout is reached.
    fn wait_for_ready(&self, _timeout: Duration) -> Result<(), MigratoryError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal communicator exercising default trait methods.
    struct MinimalComm;

    impl Communicator for MinimalComm {
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            Ok(command.to_string())
        }
    }

    #[test]
    fn test_communicator_default_methods() -> Result<(), MigratoryError> {
        let comm = MinimalComm;
        assert_eq!(comm.execute("echo hi")?, "echo hi");
        assert!(comm.upload(Path::new("a"), "b").is_ok());
        assert!(comm.download("b", Path::new("a")).is_ok());
        assert!(comm.execute_interactive().is_ok());
        assert!(comm.wait_for_ready(Duration::from_millis(1)).is_ok());
        Ok(())
    }
}
