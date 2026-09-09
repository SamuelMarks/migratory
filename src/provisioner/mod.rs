//! Provisioner module for running provisioning scripts.

use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::collections::HashMap;

pub mod ansible;
pub mod chef;
pub mod docker;
pub mod file;
pub mod puppet;
pub mod salt;
pub mod shell;

/// Interface for machine provisioners.
pub trait Provisioner {
    /// Name of the provisioner.
    fn name(&self) -> &str;

    /// Prepares the provisioner based on config.
    fn prepare(&mut self, config: &HashMap<String, String>) -> Result<(), MigratoryError>;

    /// Executes the provisioning logic.
    fn provision(&self, comm: &dyn Communicator) -> Result<(), MigratoryError>;

    /// Cleanup after provisioning.
    fn cleanup(&self) -> Result<(), MigratoryError>;
}
