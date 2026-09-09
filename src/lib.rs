#![feature(coverage_attribute)]
//! Migratory core library.
//!
//! An open-source, 100% compatible replica of Vagrant.

#![deny(missing_docs)]
#![deny(clippy::all)]
#![deny(clippy::correctness)]
#![deny(clippy::suspicious)]
#![deny(clippy::complexity)]
#![deny(clippy::perf)]
#![deny(clippy::style)]
#![deny(clippy::cargo)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::clone_on_ref_ptr)]
#![deny(clippy::empty_line_after_outer_attr)]
#![deny(clippy::explicit_into_iter_loop)]
#![deny(clippy::flat_map_option)]
#![deny(clippy::manual_assert)]
#![deny(clippy::match_same_arms)]
#![deny(clippy::needless_borrow)]
#![deny(clippy::redundant_closure)]
#![deny(clippy::redundant_static_lifetimes)]
#![deny(clippy::semicolon_if_nothing_returned)]
#![deny(clippy::single_match_else)]
#![deny(clippy::unused_async)]
#![deny(clippy::cast_lossless)]
#![deny(clippy::checked_conversions)]
#![deny(clippy::cloned_instead_of_copied)]
#![deny(clippy::default_trait_access)]
#![deny(clippy::expl_impl_clone_on_copy)]
#![deny(clippy::filter_map_next)]
#![deny(clippy::fn_params_excessive_bools)]
#![deny(clippy::if_then_some_else_none)]
#![deny(clippy::inefficient_to_string)]
#![deny(clippy::macro_use_imports)]
#![deny(clippy::manual_is_ascii_check)]
#![deny(clippy::match_bool)]
#![deny(clippy::mut_mut)]
#![deny(clippy::naive_bytecount)]
#![deny(clippy::needless_bitwise_bool)]
#![deny(clippy::range_minus_one)]
#![deny(clippy::range_plus_one)]
#![deny(clippy::same_functions_in_if_condition)]
#![deny(clippy::str_split_at_newline)]
#![deny(clippy::string_add_assign)]
#![deny(clippy::unnecessary_join)]
#![deny(clippy::zero_sized_map_values)]
#![allow(clippy::multiple_crate_versions)]

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
