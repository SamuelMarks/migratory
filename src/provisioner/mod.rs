//! Provisioner module for running provisioning scripts.

use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::collections::HashMap;

/// Ansible provisioner implementation.
pub mod ansible;
/// Chef provisioner implementation.
pub mod chef;
/// Docker provisioner implementation.
pub mod docker;
/// File upload provisioner implementation.
pub mod file;
/// Puppet provisioner implementation.
pub mod puppet;
/// Salt provisioner implementation.
pub mod salt;
/// Shell script provisioner implementation.
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
