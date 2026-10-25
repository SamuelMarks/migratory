//! Migratory core library.
//!
//! An open-source, 100% compatible replica of Vagrant.

#![allow(clippy::multiple_crate_versions)]

/// Middleware and action runner framework.
pub mod action;
/// Local Vagrant box management and metadata parsing.
pub mod box_manager;
/// Command-line interface definitions and subcommand handlers.
pub mod cli;
/// Vagrant Cloud / HCP Vagrant Registry API client.
pub mod cloud;
/// Guest communication transports (SSH, WinRM, Docker).
pub mod communicator;
/// `Vagrantfile` configuration data structures and evaluators.
pub mod config;
/// Unified error enum (`MigratoryError`) and result helper traits.
pub mod error;
/// Guest OS detection and capability implementations.
pub mod guest;
/// Host OS detection and capability implementations.
pub mod host;
/// Virtual machine networking, port collision detection, and interface management.
pub mod network;
/// Plugin management and WebAssembly plugin runtime.
pub mod plugin;
/// Hypervisor provider trait and driver implementations.
pub mod provider;
/// Machine provisioner trait and implementations.
pub mod provisioner;
/// Global and local machine state persistence and file locking.
pub mod state;
/// Host-to-guest synced folder trait and driver implementations.
pub mod synced_folder;
/// Terminal user interface and machine-readable output formatting.
pub mod ui;

pub use error::MigratoryError;

/// Performs a constant-time comparison between two strings.
///
/// This prevents timing side-channel attacks when comparing sensitive tokens
/// or cryptographic hashes.
///
/// # Arguments
///
/// * `a` - The first string to compare.
/// * `b` - The second string to compare.
///
/// # Returns
///
/// Returns `true` if `a` and `b` are identical, `false` otherwise.
pub fn constant_time_compare(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a_bytes.iter().zip(b_bytes.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare("token123", "token123"));
        assert!(!constant_time_compare("token123", "token124"));
        assert!(!constant_time_compare("token123", "token12"));
        assert!(!constant_time_compare("token12", "token123"));
        assert!(constant_time_compare("", ""));
    }
}
