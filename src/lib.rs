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
#![deny(clippy::collapsible_if)]
#![deny(clippy::needless_raw_string_hashes)]
#![deny(clippy::missing_const_for_thread_local)]
#![deny(clippy::io_other_error)]
#![deny(clippy::empty_enums)]
#![deny(clippy::explicit_iter_loop)]
#![deny(clippy::needless_continue)]
#![deny(clippy::ptr_as_ptr)]
#![deny(clippy::unused_unit)]
#![deny(clippy::unnecessary_box_returns)]
#![deny(clippy::manual_string_new)]
#![deny(clippy::manual_clamp)]
#![deny(clippy::manual_instant_elapsed)]
#![deny(clippy::manual_is_power_of_two)]
#![deny(clippy::manual_ok_or)]
#![deny(clippy::trivially_copy_pass_by_ref)]
#![deny(clippy::case_sensitive_file_extension_comparisons)]
#![deny(clippy::stable_sort_primitive)]
#![deny(clippy::ref_binding_to_reference)]
#![deny(clippy::redundant_else)]
#![deny(clippy::match_wild_err_arm)]
#![deny(clippy::match_wildcard_for_single_variants)]
#![deny(clippy::unnested_or_patterns)]
#![deny(clippy::bool_to_int_with_if)]
#![deny(clippy::borrow_as_ptr)]
#![deny(clippy::clear_with_drain)]
#![deny(clippy::comparison_to_empty)]
#![deny(clippy::derive_partial_eq_without_eq)]
#![deny(clippy::doc_link_with_quotes)]
#![deny(clippy::double_comparisons)]
#![deny(clippy::equatable_if_let)]
#![deny(clippy::explicit_auto_deref)]
#![deny(clippy::get_first)]
#![deny(clippy::implicit_saturating_sub)]
#![deny(clippy::iter_filter_is_ok)]
#![deny(clippy::iter_filter_is_some)]
#![deny(clippy::iter_kv_map)]
#![deny(clippy::iter_on_empty_collections)]
#![deny(clippy::iter_on_single_items)]
#![deny(clippy::large_digit_groups)]
#![deny(clippy::large_futures)]
#![deny(clippy::manual_bits)]
#![deny(clippy::manual_filter)]
#![deny(clippy::manual_filter_map)]
#![deny(clippy::manual_find)]
#![deny(clippy::manual_find_map)]
#![deny(clippy::manual_flatten)]
#![deny(clippy::manual_main_separator_str)]
#![deny(clippy::manual_range_contains)]
#![deny(clippy::manual_rem_euclid)]
#![deny(clippy::manual_retain)]
#![deny(clippy::manual_slice_size_calculation)]
#![deny(clippy::manual_split_once)]
#![deny(clippy::manual_str_repeat)]
#![deny(clippy::manual_while_let_some)]
#![deny(clippy::match_as_ref)]
#![deny(clippy::match_like_matches_macro)]
#![deny(clippy::mut_mutex_lock)]
#![deny(clippy::needless_borrowed_reference)]
#![deny(clippy::needless_collect)]
#![deny(clippy::needless_late_init)]
#![deny(clippy::needless_match)]
#![deny(clippy::needless_option_as_deref)]
#![deny(clippy::needless_option_take)]
#![deny(clippy::needless_pub_self)]
#![deny(clippy::needless_raw_strings)]
#![deny(clippy::no_effect_underscore_binding)]
#![deny(clippy::non_ascii_literal)]
#![deny(clippy::non_canonical_clone_impl)]
#![deny(clippy::non_canonical_partial_ord_impl)]
#![deny(clippy::option_filter_map)]
#![deny(clippy::option_map_unit_fn)]
#![deny(clippy::option_option)]
#![deny(clippy::rc_buffer)]
#![deny(clippy::rc_mutex)]
#![deny(clippy::redundant_allocation)]
#![deny(clippy::redundant_clone)]
#![deny(clippy::redundant_feature_names)]
#![deny(clippy::ref_as_ptr)]
#![deny(clippy::seek_from_current)]
#![deny(clippy::seek_to_start_instead_of_rewind)]
#![deny(clippy::should_panic_without_expect)]
#![deny(clippy::significant_drop_in_scrutinee)]
#![deny(clippy::single_char_pattern)]
#![deny(clippy::single_match)]
#![deny(clippy::string_add)]
#![deny(clippy::string_extend_chars)]
#![deny(clippy::suspicious_operation_groupings)]
#![deny(clippy::trait_duplication_in_bounds)]
#![deny(clippy::type_repetition_in_bounds)]
#![deny(clippy::unnecessary_cast)]
#![deny(clippy::unnecessary_filter_map)]
#![deny(clippy::unnecessary_find_map)]
#![deny(clippy::unnecessary_fold)]
#![deny(clippy::unnecessary_lazy_evaluations)]
#![deny(clippy::unnecessary_mut_passed)]
#![deny(clippy::unnecessary_to_owned)]
#![deny(clippy::unneeded_field_pattern)]
#![deny(clippy::unused_peekable)]
#![deny(clippy::unused_rounding)]
#![deny(clippy::useless_let_if_seq)]
#![deny(clippy::while_let_on_iterator)]
#![deny(clippy::wildcard_dependencies)]
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
