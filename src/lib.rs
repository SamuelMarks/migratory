#![feature(coverage_attribute)]
//! Migratory core library.
//!
//! An open-source, 100% compatible replica of Vagrant.

#![deny(missing_docs)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

pub mod action;
pub mod box_manager;
pub mod cli;
pub mod cloud;
pub mod communicator;
pub mod config;
pub mod error;
pub mod guest;
pub mod host;
pub mod network;
pub mod plugin;
pub mod provider;
pub mod provisioner;
pub mod state;
pub mod synced_folder;
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
