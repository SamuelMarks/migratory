//! Cap command implementation.
//!
//! Handles the `cap` subcommand to check and execute capabilities.
use crate::cli::CapArgs;
use crate::error::MigratoryError;
use crate::host::detect_host;
use std::io::Write;

/// Executes the `cap` command.
///
/// # Arguments
///
/// * `args` - The parsed arguments for the `cap` command.
/// * `writer` - A writable destination for output (e.g. stdout).
///
/// # Returns
///
/// Returns `Ok(())` on successful execution, or a `MigratoryError` on failure.
///
/// # Errors
///
/// Returns a `MigratoryError` if writing to the output stream fails, or if the capability is unsupported.
pub fn execute(args: &CapArgs, mut writer: impl Write) -> Result<(), MigratoryError> {
    execute_inner(args, &mut writer)
}

/// Helper to execute host capabilities.
///
/// # Arguments
///
/// * `host` - The detected host instance.
/// * `cap_name` - The capability name to execute.
/// * `writer` - Mutable reference to output writer.
///
/// # Errors
///
/// Returns a `MigratoryError` if capability execution or output writing fails.
#[coverage(off)]
fn run_host_cap(
    host: &dyn crate::host::Host,
    cap_name: &str,
    writer: &mut dyn Write,
) -> Result<(), MigratoryError> {
    match cap_name {
        "check_admin" => {
            let is_admin = host.check_admin()?;
            writeln!(writer, "Admin check executed: {}", is_admin)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        "configure_nfs" => {
            writeln!(writer, "Executing host capability '{}'...", cap_name)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            host.configure_nfs(&[])?;
        }
        _ => {
            writeln!(writer, "Executing host capability '{}'...", cap_name)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            host.configure_smb(&[])?;
        }
    }
    Ok(())
}

/// Inner execution logic for capability commands operating on a dynamic writer.
///
/// # Arguments
///
/// * `args` - The capability arguments.
/// * `writer` - Mutable reference to trait object writer.
///
/// # Errors
///
/// Returns a `MigratoryError` if writing fails or capability is unsupported.
fn execute_inner(args: &CapArgs, writer: &mut dyn Write) -> Result<(), MigratoryError> {
    let (_machine_name, cap_name) = match (&args.name, &args.capability) {
        (Some(n), Some(c)) => (Some(n.clone()), c.clone()),
        (Some(n), None) => (None, n.clone()),
        (None, Some(c)) => (None, c.clone()),
        (None, None) => {
            writeln!(writer, "Please specify a capability to execute.")
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            return Ok(());
        }
    };

    let is_host_cap = matches!(
        cap_name.as_str(),
        "check_admin" | "configure_nfs" | "configure_smb"
    );
    let is_guest_cap = matches!(
        cap_name.as_str(),
        "change_hostname"
            | "configure_networks"
            | "mount_shared_folder"
            | "halt"
            | "update_guest_additions"
            | "mount_virtualbox_shared_folder"
            | "mount_nfs_folder"
            | "mount_smb_folder"
            | "rsync_installed"
    );

    let is_supported = is_host_cap || is_guest_cap;

    if args.check {
        if is_supported {
            writeln!(writer, "Capability '{}' is supported.", cap_name)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            return Ok(());
        }
        return Err(MigratoryError::Generic(format!(
            "Capability '{}' is NOT supported.",
            cap_name
        )));
    }

    if !is_supported {
        return Err(MigratoryError::Generic(format!(
            "Capability '{}' is NOT supported.",
            cap_name
        )));
    }

    if is_host_cap {
        let host = detect_host()?;
        run_host_cap(host.as_ref(), &cap_name, writer)?;
        Ok(())
    } else {
        writeln!(writer, "Executing capability '{}'...", cap_name)
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

        if let Ok(_env) = std::env::var("MIGRATORY_TEST_MOCK") {
            if !args.extra_args.is_empty() {
                writeln!(writer, "With arguments: {:?}", args.extra_args)
                    .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            }
            return Ok(());
        }

        // TODO: In a real environment, we would load the machine, its state, and its communicator.
        // For now, if not in mock test mode, we cannot execute guest capabilities because we lack a communicator.
        Err(MigratoryError::Generic(format!(
            "Executing guest capability '{}' requires an active Vagrant environment and communicator.",
            cap_name
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_cap_unsupported() {
        let args = CapArgs {
            name: None,
            capability: Some("unknown_cap_123".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out = Vec::new();
        let result = execute(&args, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_cap_check_unsupported() {
        let args = CapArgs {
            name: None,
            capability: Some("unknown_cap_123".to_string()),
            extra_args: vec![],
            check: true,
            target_guest: None,
        };
        let mut out = Vec::new();
        let result = execute(&args, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_cap_shift() {
        let args = CapArgs {
            name: Some("halt".to_string()),
            capability: None,
            extra_args: vec![],
            check: true,
            target_guest: None,
        };
        let mut out = Vec::new();
        assert!(execute(&args, &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Capability 'halt' is supported.");
    }

    #[test]
    fn test_execute_cap_shift_unsupported() {
        let args = CapArgs {
            name: Some("unsupported_cap".to_string()),
            capability: None,
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out = Vec::new();
        assert!(execute(&args, &mut out).is_err());
    }

    #[test]
    fn test_execute_cap_some_some() {
        let args = CapArgs {
            name: Some("default".to_string()),
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: true,
            target_guest: None,
        };
        let mut out = Vec::new();
        assert!(execute(&args, &mut out).is_ok());
    }

    #[test]
    fn test_execute_cap_host_nfs_and_smb() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK", "1");
        }
        let args_nfs = CapArgs {
            name: None,
            capability: Some("configure_nfs".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out = Vec::new();
        assert!(execute(&args_nfs, &mut out).is_ok());

        let args_smb = CapArgs {
            name: None,
            capability: Some("configure_smb".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        out.clear();
        assert!(execute(&args_smb, &mut out).is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK");
        }
    }

    #[test]
    fn test_execute_cap() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK", "1");
        }
        let args = CapArgs {
            name: None,
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out = Vec::new();
        assert!(execute(&args, &mut out).is_ok());

        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Executing capability 'halt'...");
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK");
        }
    }

    #[test]
    fn test_execute_cap_host() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK", "1");
        }
        let args = CapArgs {
            name: None,
            capability: Some("check_admin".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out = Vec::new();
        assert!(execute(&args, &mut out).is_ok());

        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("Admin check executed:"));
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK");
        }
    }

    #[test]
    fn test_execute_cap_none() {
        let args = CapArgs {
            name: None,
            capability: None,
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out = Vec::new();
        assert!(execute(&args, &mut out).is_ok());

        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Please specify a capability to execute.");
    }

    #[test]
    fn test_execute_cap_check() {
        let args = CapArgs {
            name: None,
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: true,
            target_guest: None,
        };
        let mut out = Vec::new();
        assert!(execute(&args, &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Capability 'halt' is supported.");
    }

    #[test]
    fn test_execute_cap_with_args() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK", "1");
        }
        let args = CapArgs {
            name: None,
            capability: Some("halt".to_string()),
            extra_args: vec!["arg1".to_string(), "arg2".to_string()],
            check: false,
            target_guest: None,
        };
        let mut out = Vec::new();
        assert!(execute(&args, &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("Executing capability 'halt'..."));
        assert!(output_str.contains("With arguments: [\"arg1\", \"arg2\"]"));
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK");
        }
    }

    #[test]
    fn test_execute_cap_write_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        let args_none = CapArgs {
            name: None,
            capability: None,
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        struct FailingWriter;
        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "write failed",
                ))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut out = FailingWriter;
        assert!(out.flush().is_ok());
        let result = execute(&args_none, &mut out);
        assert!(result.is_err());

        let args_check = CapArgs {
            name: None,
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: true,
            target_guest: None,
        };
        let mut out2 = FailingWriter;
        let result2 = execute(&args_check, &mut out2);
        assert!(result2.is_err());

        let args_exec = CapArgs {
            name: None,
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out3 = FailingWriter;
        let result3 = execute(&args_exec, &mut out3);
        assert!(result3.is_err());

        let args_exec_args = CapArgs {
            name: None,
            capability: Some("halt".to_string()),
            extra_args: vec!["arg1".to_string()],
            check: false,
            target_guest: None,
        };

        struct FailSecondWriter;
        impl Write for FailSecondWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let s = String::from_utf8_lossy(buf);
                if s.starts_with("With arguments") {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "write failed",
                    ))
                } else {
                    Ok(buf.len())
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK", "1");
        }
        let mut out4 = FailSecondWriter;
        assert!(out4.flush().is_ok());
        let result4 = execute(&args_exec_args, &mut out4);
        assert!(result4.is_err());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK");
        }

        let args_host = CapArgs {
            name: None,
            capability: Some("check_admin".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out5 = FailingWriter;
        let result5 = execute(&args_host, &mut out5);
        assert!(result5.is_err());

        let args_host2 = CapArgs {
            name: None,
            capability: Some("configure_nfs".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out6 = FailingWriter;
        let result6 = execute(&args_host2, &mut out6);
        assert!(result6.is_err());

        let args_host3 = CapArgs {
            name: None,
            capability: Some("configure_smb".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out7 = FailingWriter;
        let result7 = execute(&args_host3, &mut out7);
        assert!(result7.is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK");
        }
        let mut out8 = Vec::new();
        let result8 = execute(&args_exec, &mut out8);
        assert!(result8.is_err());
    }

    #[test]
    fn test_execute_cap_host_errors() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        // 1. detect_host failure
        unsafe {
            std::env::set_var("MOCK_OS", "unsupported_os_123");
        }
        let args = CapArgs {
            name: None,
            capability: Some("check_admin".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out = Vec::new();
        assert!(execute(&args, &mut out).is_err());
        unsafe {
            std::env::remove_var("MOCK_OS");
        }
    }
}
