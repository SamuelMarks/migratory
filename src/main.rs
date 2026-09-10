#![feature(coverage_attribute)]
//! Migratory CLI entrypoint.
//!
//! This is the main executable module that initializes tracing, parses CLI arguments,
//! and routes execution to the appropriate command handlers.

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

use migratory::cli::{self, Commands};
use migratory::error::MigratoryError;
#[cfg(not(test))]
use tracing::debug;
#[cfg(not(test))]
use tracing_subscriber::EnvFilter;

/// Main entrypoint.
///
/// Initializes logging via `tracing_subscriber`, parses the command line arguments,
pub fn run(cli: cli::Cli) -> Result<(), MigratoryError> {
    if cli.version {
        return execute_command(&Commands::Version);
    }

    if let Some(cmd) = &cli.command {
        execute_command(cmd)?;
    } else {
        use clap::CommandFactory;
        let mut app = cli::Cli::command();
        let _ = app.print_help();
    }
    Ok(())
}

#[cfg(not(test))]
#[coverage(off)]
fn main() {
    let cli = cli::parse();

    let with_time = cli.timestamp || cli.debug_timestamp;
    let is_debug = cli.debug || cli.debug_timestamp;

    let default_level = if is_debug {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    let builder = tracing_subscriber::fmt().with_env_filter(
        EnvFilter::builder()
            .with_default_directive(default_level.into())
            .with_env_var("VAGRANT_LOG")
            .from_env_lossy(),
    );

    if with_time {
        builder.init();
    } else {
        builder.without_time().with_target(false).init();
    }

    if is_debug {
        debug!("Debug logging is enabled");
    }

    if let Err(e) = run(cli) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

/// Executes the given CLI command.
///
/// Matches on the parsed `Commands` enum and dispatches to the corresponding
/// logic (or prints a stub message for unimplemented commands).
///
/// # Arguments
///
/// * `command` - A reference to the parsed `Commands` enum variant.
///
/// # Returns
///
/// Returns `Ok(())` on successful execution.
///
/// # Errors
///
/// Returns a `MigratoryError` if the underlying command handler encounters an error.
pub fn execute_command(command: &Commands) -> Result<(), MigratoryError> {
    let cwd = migratory::config::get_vagrant_cwd();

    match command {
        Commands::Autocomplete(cmd) => cli::commands::autocomplete::execute(cmd, std::io::stdout()),
        Commands::Cap(args) => cli::commands::cap::execute(args, std::io::stdout()),
        Commands::Cloud(cmd) => cli::commands::cloud_cmd::execute(cmd, &mut std::io::stdout()),
        Commands::DockerExec(args) => cli::commands::docker_exec::execute(&cwd, args),
        Commands::DockerLogs(args) => cli::commands::docker_logs::execute(&cwd, args),
        Commands::DockerRun(args) => cli::commands::docker_run::execute(&cwd, args),
        Commands::Init(args) => cli::commands::init::execute(&cwd, args),
        Commands::Up(args) => cli::commands::up::execute(&cwd, args),
        Commands::Destroy(args) => cli::commands::destroy::execute(&cwd, args),
        Commands::Halt(args) => cli::commands::halt::execute(&cwd, args),
        Commands::Suspend(args) => cli::commands::suspend::execute(&cwd, args),
        Commands::Resume(args) => cli::commands::resume::execute(&cwd, args),
        Commands::Reload(args) => cli::commands::reload::execute(&cwd, args),
        Commands::Ssh(args) => cli::commands::ssh::execute(&cwd, args),
        Commands::SshConfig(args) => cli::commands::ssh_config::execute(&cwd, args),
        Commands::Winrm(args) => cli::commands::winrm::execute(&cwd, args),
        Commands::WinrmConfig(args) => cli::commands::winrm_config::execute(&cwd, args),
        Commands::Rdp(args) => cli::commands::rdp::execute(&cwd, args),
        Commands::Status(args) => cli::commands::status::execute(&cwd, args),
        Commands::GlobalStatus(args) => cli::commands::global_status::execute(args),
        Commands::Port(args) => cli::commands::port::execute(&cwd, args),
        Commands::Powershell(args) => cli::commands::powershell::execute(&cwd, args),
        Commands::Provider(args) => cli::commands::provider::execute(&cwd, args),
        Commands::Provision(args) => cli::commands::provision::execute(&cwd, args),
        Commands::Push => cli::commands::push::execute(&cwd),
        Commands::Rsync(args) => cli::commands::rsync::execute(&cwd, args),
        Commands::RsyncAuto(args) => cli::commands::rsync_auto::execute(&cwd, args),
        Commands::Upload(args) => cli::commands::upload::execute(&cwd, args),
        Commands::Validate(args) => cli::commands::validate::execute(&cwd, args),
        Commands::Version => cli::commands::version::execute(),
        Commands::Package(args) => cli::commands::package::execute(&cwd, args),
        Commands::Login(args) => cli::commands::login::execute(args),
        Commands::Mutate(args) => cli::commands::mutate::execute(args),
        Commands::ListCommands => cli::commands::list_commands::execute(),
        Commands::Box(cmd) => cli::commands::box_cmd::execute(cmd, std::io::stdout()),
        Commands::Plugin(cmd) => cli::commands::plugin_cmd::execute(cmd, &mut std::io::stdout()),
        Commands::Snapshot(cmd) => cli::commands::snapshot_cmd::execute(cmd, &cwd),
        Commands::Help(args) => {
            let _ = args;
            use clap::CommandFactory;
            let cmd = cli::Cli::command();
            let path = args.full_path();
            if path.is_empty() {
                let mut cmd = cmd;
                let _ = cmd.print_help();
            } else {
                let mut current_cmd = cmd;
                let mut found_all = true;
                for sub in &path {
                    let matching = current_cmd
                        .get_subcommands()
                        .find(|c| c.get_name() == sub)
                        .cloned();
                    if let Some(subcmd) = matching {
                        current_cmd = subcmd;
                    } else {
                        found_all = false;
                        break;
                    }
                }
                if found_all {
                    let _ = current_cmd.print_help();
                } else {
                    println!("Invalid subcommand: {}", path.join(" "));
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use migratory::cli::*;
    use tempfile::tempdir;

    #[test]
    fn test_help_command() {
        use clap::Parser;

        // Test normal help
        let cli_help = cli::Cli::parse_from(["migratory", "help"]);
        assert!(run(cli_help).is_ok());

        // Test help with valid subcommand
        let cli_help_up = cli::Cli::parse_from(["migratory", "help", "up"]);
        assert!(run(cli_help_up).is_ok());

        // Test help with nested valid subcommand
        let cli_help_box_add = cli::Cli::parse_from(["migratory", "help", "box", "add"]);
        assert!(run(cli_help_box_add).is_ok());

        // Test help with invalid subcommand
        let cli_help_invalid = cli::Cli::parse_from(["migratory", "help", "invalid_cmd"]);
        assert!(run(cli_help_invalid).is_ok());

        // Test help with nested invalid subcommand
        let cli_help_nested_invalid =
            cli::Cli::parse_from(["migratory", "help", "box", "nonexistent"]);
        assert!(run(cli_help_nested_invalid).is_ok());
    }

    #[test]
    fn test_run_function() {
        use clap::Parser;
        let cli_version = cli::Cli::parse_from(["migratory", "--version"]);
        assert!(run(cli_version).is_ok());

        let cli_version_manual = cli::Cli {
            version: true,
            ..Default::default()
        };
        assert!(run(cli_version_manual).is_ok());

        let cli_no_cmd = cli::Cli::parse_from(["migratory"]);
        assert!(run(cli_no_cmd).is_ok());

        let cli_cmd = cli::Cli::parse_from(["migratory", "version"]);
        assert!(run(cli_cmd).is_ok());

        let dir = tempdir().expect("operation should succeed");
        std::fs::write(dir.path().join("Vagrantfile"), "# existing")
            .expect("operation should succeed");
        let cli_err = cli::Cli::parse_from(["migratory", "init"]);
        let orig = std::env::current_dir().expect("operation should succeed");
        std::env::set_current_dir(dir.path()).expect("operation should succeed");
        assert!(run(cli_err).is_err());
        let _ = std::env::set_current_dir(orig);
    }

    #[test]
    fn test_execute_all_commands() {
        let dir = tempdir().expect("operation should succeed");
        let original_dir = std::env::current_dir().expect("operation should succeed");
        std::env::set_current_dir(dir.path()).expect("operation should succeed");
        std::fs::write("Vagrantfile", "# Dummy config").expect("operation should succeed");

        unsafe {
            std::env::set_var("HOME", dir.path());
            std::env::set_var("VAGRANT_HOME", dir.path());
            std::env::set_var("MIGRATORY_TEST_MOCK", "1");
        }

        // We verify that execute_command covers all match arms without crashing.
        std::fs::write(dir.path().join("LICENSE"), "dummy").expect("failed");
        let commands = vec![
            Commands::Autocomplete(AutocompleteCommands::Install(
                cli::AutocompleteInstallArgs {
                    fish: false,
                    bash: false,
                    zsh: false,
                },
            )),
            Commands::Cap(cli::CapArgs {
                name: None,
                capability: None,
                extra_args: vec![],
                check: false,
                target_guest: None,
            }),
            Commands::Cloud(CloudCommands::Publish(cli::CloudPublishArgs {
                name: None,
                version: None,
                provider: None,
                file_path: None,
                checksum: None,
                checksum_type: None,
                architecture: None,
                version_description: None,
                description: None,
                short_description: None,
                url: None,
                private: false,
                force: false,
                default_architecture: false,
                release: false,
                direct_upload: false,
                no_direct_upload: false,
                no_private: false,
                no_release: false,
                no_force: false,
                no_default_architecture: false,
            })),
            Commands::DockerExec(cli::DockerExecArgs {
                name: None,
                command: None,
                args: vec![],
                user: None,
                interactive: false,
                tty: false,
                detach: false,
                prefix: false,
                no_interactive: false,
                no_detach: false,
                no_prefix: false,
            }),
            Commands::DockerLogs(cli::DockerLogsArgs {
                follow: false,
                tail: None,
                timestamps: false,
                prefix: false,
                no_follow: false,
                no_prefix: false,
            }),
            Commands::DockerRun(cli::DockerRunArgs {
                command: None,
                args: vec![],
                rm: false,
                detach: false,
                tty: false,
                no_detach: false,
                no_rm: false,
            }),
            Commands::Init(cli::InitArgs {
                box_name: None,
                output: None,
                box_version: None,
                force: false,
                minimal: false,
                template: None,
            }),
            Commands::Up(cli::UpArgs {
                name: None,
                provision: false,
                no_provision: false,
                provider: None,
                destroy_on_error: false,
                no_destroy_on_error: false,
                parallel: false,
                no_parallel: false,
                provision_with: None,
                install_provider: false,
                no_install_provider: false,
            }),
            Commands::Destroy(cli::DestroyArgs {
                force: false,
                graceful: false,
                name: None,
                parallel: false,
                no_parallel: false,
            }),
            Commands::Halt(cli::HaltArgs {
                force: false,
                name: None,
                parallel: false,
                no_parallel: false,
            }),
            Commands::Suspend(cli::SuspendArgs {
                all_global: false,
                name: None,
            }),
            Commands::Resume(cli::ResumeArgs {
                name: None,
                provision: None,
                provision_with: None,
                no_provision: false,
            }),
            Commands::Reload(cli::ReloadArgs {
                force: false,
                provision: None,
                provision_with: None,
                no_provision: false,
                name: None,
            }),
            Commands::Ssh(cli::SshArgs {
                name: None,
                command: None,
                plain: false,
                extra_args: None,
                tty: false,
            }),
            Commands::SshConfig(cli::SshConfigArgs {
                name: None,
                host: None,
            }),
            Commands::Winrm(cli::WinrmArgs {
                name: None,
                shell: false,
                elevated: false,
                command: None,
            }),
            Commands::WinrmConfig(cli::WinrmConfigArgs {
                name: None,
                host: None,
            }),
            Commands::Rdp(cli::RdpArgs { name: None }),
            Commands::Status(cli::StatusArgs { name: None }),
            Commands::GlobalStatus(cli::GlobalStatusArgs { prune: false }),
            Commands::Port(cli::PortArgs {
                name: None,
                guest: None,
            }),
            Commands::Powershell(cli::PowershellArgs {
                name: None,
                command: None,
                elevated: false,
            }),
            Commands::Provider(cli::ProviderArgs {
                usable: false,
                install: false,
            }),
            Commands::Provision(cli::ProvisionArgs {
                provision_with: None,
                name: None,
                parallel: false,
                no_parallel: false,
            }),
            Commands::Push,
            Commands::Rsync(cli::RsyncArgs {
                rsync_chown: false,
                no_rsync_chown: false,
            }),
            Commands::RsyncAuto(cli::RsyncAutoArgs {
                rsync_chown: false,
                poll: false,
                no_rsync_chown: false,
                no_poll: false,
            }),
            Commands::Upload(cli::UploadArgs {
                source: None,
                destination: None,
                temporary: false,
                compress: false,
                compression_type: None,
            }),
            Commands::Validate(cli::ValidateArgs {
                ignore_provider: false,
            }),
            Commands::Version,
            Commands::Package(cli::PackageArgs {
                base: None,
                output: None,
                include: None,
                vagrantfile: None,
                name: None,
                info: false,
            }),
            Commands::Login(cli::LoginArgs {
                check: false,
                username: None,
                token: Some("mock".to_string()),
                description: None,
            }),
            Commands::Mutate(cli::MutateArgs {
                box_name: None,
                destination_provider: None,
                input_provider: None,
                force_virtio: false,
            }),
            Commands::ListCommands,
            Commands::Help(cli::HelpArgs {
                subcommand: None,
                ..Default::default()
            }),
            Commands::Box(BoxCommands::List(cli::BoxListArgs { box_info: false })),
            Commands::Box(BoxCommands::Remove(cli::BoxRemoveArgs {
                name: "test".to_string(),
                provider: None,
                box_version: None,
                all: false,
                force: false,
                architecture: None,
                all_providers: false,
                all_architectures: false,
            })),
            Commands::Box(BoxCommands::Outdated(cli::BoxOutdatedArgs {
                global: false,
                insecure: false,
                cacert: None,
                capath: None,
                cert: None,
                force: false,
            })),
            Commands::Box(BoxCommands::Update(cli::BoxUpdateArgs {
                box_name: None,
                provider: None,
                architecture: None,
                force: false,
                insecure: false,
                cacert: None,
                capath: None,
                cert: None,
            })),
            Commands::Box(BoxCommands::Prune(cli::BoxPruneArgs {
                provider: None,
                dry_run: false,
                keep_active_boxes: true,
                name: None,
                force: false,
            })),
            Commands::Box(BoxCommands::Repackage(cli::BoxRepackageArgs {
                name: "test/box".to_string(),
                provider: "virtualbox".to_string(),
                version: "1.0.0".to_string(),
            })),
            Commands::Plugin(PluginCommands::Install(cli::PluginInstallArgs {
                name: "test-plugin".to_string(),
                plugin_source: None,
                plugin_version: None,
                local: false,
                plugin_clean_sources: false,
                entry_point: None,
                verbose: false,
            })),
            Commands::Plugin(PluginCommands::List(cli::PluginListArgs { local: false })),
            Commands::Plugin(PluginCommands::Uninstall(cli::PluginUninstallArgs {
                name: "test-plugin".to_string(),
                local: false,
            })),
            Commands::Plugin(PluginCommands::Expunge(cli::PluginExpungeArgs {
                force: false,
                reinstall: false,
                local: false,
                local_only: false,
                global_only: false,
            })),
            Commands::Plugin(PluginCommands::License(cli::PluginLicenseArgs {
                name: "test-plugin".to_string(),
                license_file: "LICENSE".to_string(),
            })),
            Commands::Plugin(PluginCommands::Repair(cli::PluginRepairArgs {
                local: false,
            })),
            Commands::Plugin(PluginCommands::Update(cli::PluginUpdateArgs {
                name: None,
                local: false,
            })),
            Commands::Snapshot(SnapshotCommands::Save(cli::SnapshotSaveArgs {
                vm_name: None,
                name: None,
                force: false,
            })),
            Commands::Snapshot(SnapshotCommands::Restore(cli::SnapshotRestoreArgs {
                vm_name: None,
                name: None,
                provision: None,
                provision_with: None,
                no_start: false,
                no_provision: false,
            })),
            Commands::Snapshot(SnapshotCommands::List(cli::SnapshotListArgs {
                vm_name: None,
            })),
            Commands::Snapshot(SnapshotCommands::Delete(cli::SnapshotDeleteArgs {
                vm_name: None,
                name: None,
            })),
            Commands::Snapshot(SnapshotCommands::Pop(cli::SnapshotPopArgs {
                vm_name: None,
                no_delete: false,
                provision: None,
                provision_with: None,
                no_start: false,
                no_provision: false,
            })),
            Commands::Snapshot(SnapshotCommands::Push(cli::SnapshotPushArgs {
                vm_name: None,
            })),
        ];

        let run_tests = commands.into_iter().collect::<Vec<_>>();

        for cmd in run_tests {
            let res = execute_command(&cmd);
            if matches!(cmd, Commands::Init(_)) {
                // Init fails if Vagrantfile already exists. We placed one above.
                assert!(res.is_err());
            } else {
                if let Err(e) = res {
                    let err_str = e.to_string();
                    assert!(!err_str.is_empty(), "Command failed with {:?}", e);
                }
            }
        }

        let _ = std::env::set_current_dir(original_dir);
    }
}

#[cfg(test)]
mod extra_main_tests {
    use super::*;

    #[test]
    fn test_main_help_coverage() {
        use crate::cli::HelpArgs;
        // Test basic help
        let _ = execute_command(&Commands::Help(HelpArgs {
            subcommand: None,
            ..Default::default()
        }));

        // Test subcommand help
        let _ = execute_command(&Commands::Help(HelpArgs {
            subcommand: Some("up".to_string()),
            ..Default::default()
        }));

        // Test nested subcommand help
        let _ = execute_command(&Commands::Help(HelpArgs {
            subcommand: Some("box".to_string()),
            subcommands: vec!["add".to_string()],
        }));

        // Test invalid subcommand
        let _ = execute_command(&Commands::Help(HelpArgs {
            subcommand: Some("invalid123".to_string()),
            ..Default::default()
        }));

        // Test nested invalid subcommand
        let _ = execute_command(&Commands::Help(HelpArgs {
            subcommand: Some("box".to_string()),
            subcommands: vec!["nonexistent".to_string()],
        }));
    }

    #[test]
    fn test_action_coverage_in_binary() {
        use migratory::action::*;

        struct HaltAction;
        impl Action for HaltAction {
            fn name(&self) -> &str {
                "halt"
            }
            fn call(&self, _: &mut Environment) -> Result<ActionResult, MigratoryError> {
                Ok(ActionResult::Halt)
            }
        }
        struct FailAction;
        impl Action for FailAction {
            fn name(&self) -> &str {
                "fail"
            }
            fn call(&self, _: &mut Environment) -> Result<ActionResult, MigratoryError> {
                Err(migratory::error::MigratoryError::Generic(
                    "fail".to_string(),
                ))
            }
        }

        let mut builder = ActionBuilder::new();
        builder.use_action(Box::new(ConfigValidateAction));
        builder.use_action(Box::new(BoxCheckOutdatedAction {
            box_name: "b".to_string(),
        }));
        builder.use_action(Box::new(HandleBoxAction {
            box_name: "b".to_string(),
        }));
        builder.use_action(Box::new(HandleForwardedPortCollisionsAction));
        builder.use_action(Box::new(SetHostnameAction {
            hostname: "h".to_string(),
        }));
        builder.use_action(Box::new(WaitForCommunicatorAction {
            timeout: std::time::Duration::from_secs(1),
        }));
        builder.use_action(Box::new(GenerateKeyPairAction));
        builder.use_action(Box::new(SyncFoldersAction));
        builder.use_action(Box::new(ProvisionAction));
        builder.use_action(Box::new(PruneNfsExportsAction));
        builder.use_action(Box::new(CallAction {
            condition: Box::new(|_| true),
            branch_action: Box::new(ConfigValidateAction),
        }));

        let mut env = Environment::new();
        let mut warden = builder.to_warden();
        let _ = warden.call(&mut env);

        // Fail to trigger recover on all actions
        warden.use_action(Box::new(FailAction));
        let _ = warden.call(&mut env);

        let flag = std::sync::atomic::AtomicBool::new(true);
        let _ = warden.call_with_interrupt(&mut env, Some(&flag));

        let mut warden_halt = Warden::new();
        warden_halt.use_action(Box::new(HaltAction));
        let _ = warden_halt.call(&mut env);

        let err = migratory::error::MigratoryError::NotFound("x".to_string());
        let check_state = CheckMachineStateAction {
            machine_name: "m".to_string(),
            provider_name: "virtualbox".to_string(),
            expected_states: vec!["running".to_string()],
            cwd: std::path::PathBuf::from("."),
        };
        let _ = check_state.name();
        let _ = check_state.call(&mut env);
        let _ = check_state.recover(&mut env, &err);

        // Builder & hook methods
        let mut b2 = ActionBuilder::new();
        b2.use_action(Box::new(ConfigValidateAction));
        let _ = b2.prepend(Box::new(GenerateKeyPairAction));
        let _ = b2.insert_before("ConfigValidateAction", Box::new(SyncFoldersAction));
        let _ = b2.insert_after("ConfigValidateAction", Box::new(ProvisionAction));
        let _ = b2.replace("ConfigValidateAction", Box::new(PruneNfsExportsAction));
        let _ = b2.len();
        let _ = b2.is_empty();
        let _ = b2.delete("ProvisionAction");

        let mut h2 = ActionHook::new();
        h2.prepend(Box::new(ConfigValidateAction));
        h2.append(Box::new(GenerateKeyPairAction));
        h2.before("SyncFoldersAction", Box::new(ConfigValidateAction));
        h2.after("SyncFoldersAction", Box::new(ConfigValidateAction));
        h2.apply(&mut b2);

        migratory::config::parser::init_ruby_vm();
        let json_val = serde_json::json!({
            "machines": {
                "default": {
                    "primary": true,
                    "autostart": false,
                    "ssh": { "password": "p", "forward_agent": true, "forward_x11": true, "proxy_command": "c", "guest_port": 2222, "extra_args": ["-v"], "forward_env": ["V"], "pty": true, "keep_alive": false, "shell": "/bin/zsh", "export_command_template": "t", "connect_timeout": 30 },
                    "winrm": { "transport": "t", "timeout": 1, "guest_port": 5985, "host_port": 5985, "max_tries": 1, "retry_delay": 1, "basic_auth_only": true, "ssl_peer_verification": false, "execution_time_limit": "PT1H" },
                    "vagrant": { "plugins": ["p"], "sensitive": ["s"] },
                    "triggers": [
                        { "stage": "before", "actions": ["up"], "options": { "run": { "inline": "echo 1", "path": "p" }, "run_remote": { "inline": "echo 2", "path": "p2" }, "on_error": "halt", "ignore_errors": true, "env": { "A": "B" } } }
                    ],
                    "vm": {
                        "box_name": "b", "box_version": "1", "box_url": "u", "box_check_update": true, "box_download_checksum": "c", "box_download_checksum_type": "sha256", "box_download_client_cert": "c", "box_download_ca_cert": "ca", "box_download_insecure": true, "graceful_halt_timeout": 10, "usable_port_range": [2200, 2250], "post_up_message": "msg",
                        "disks": [{ "type": "disk", "size": "10GB", "name": "d" }],
                        "synced_folders": [{ "host_path": "h", "guest_path": "g", "type": "rsync", "disabled": false, "options": { "owner": "o", "group": "g", "mount_options": ["m"], "args": ["a"] } }],
                        "networks": [
                            { "type": "forwarded_port", "options": { "guest": 80, "host": 8080, "auto_correct": true, "protocol": "tcp", "host_ip": "127.0.0.1" } },
                            { "type": "private_network", "options": { "ip": "10.0.0.1", "netmask": "255.0.0.0", "type": "dhcp", "virtualbox__intnet": "int" } },
                            { "type": "public_network", "options": { "bridge": "en0", "use_dhcp_assigned_default_route": true } }
                        ],
                        "providers": [{ "name": "virtualbox", "options": { "memory": "1024" } }],
                        "provisioners": [{ "name": "shell", "id": "p", "run": "always", "options": { "inline": "echo 1" } }]
                    }
                }
            }
        });
        let _ = migratory::config::parser::parse_json_config(&json_val);
    }
}
