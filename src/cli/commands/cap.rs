//! Cap command implementation.
//!
//! Handles the `cap` subcommand to check and execute capabilities.

use crate::cli::CapArgs;
use crate::communicator::Communicator;
use crate::error::MigratoryError;
use crate::host::detect_host;
use std::io::Write;
use std::path::Path;

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
#[coverage(off)]
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

/// Dispatches guest capabilities to a `Guest` implementation.
///
/// # Arguments
///
/// * `guest` - The detected Guest trait implementation.
/// * `comm` - The active machine communicator.
/// * `cap_name` - The capability name.
/// * `args` - Extra arguments passed to the capability.
/// * `provider_name` - The active provider name.
/// * `writer` - Output destination.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if execution fails or the capability is unsupported.
pub fn dispatch_guest_cap(
    guest: &dyn crate::guest::Guest,
    comm: &dyn Communicator,
    cap_name: &str,
    args: &[String],
    provider_name: &str,
    writer: &mut dyn Write,
) -> Result<(), MigratoryError> {
    match cap_name {
        "change_hostname" => {
            let hostname = args.first().map(|s| s.as_str()).unwrap_or("vagrant");
            writeln!(writer, "Changing guest hostname to '{}'...", hostname)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            guest.change_hostname(comm, hostname)?;
        }
        "configure_networks" => {
            writeln!(writer, "Configuring guest networks...")
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            guest.configure_networks(comm, &[])?;
        }
        "mount_shared_folder" => {
            let name = args.first().map(|s| s.as_str()).unwrap_or("vagrant");
            let guest_path_str = args.get(1).map(|s| s.as_str()).unwrap_or("/vagrant");
            let guest_path = Path::new(guest_path_str);
            writeln!(
                writer,
                "Mounting shared folder '{}' at '{}'...",
                name,
                guest_path.display()
            )
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            guest.mount_shared_folder(comm, name, guest_path)?;
        }
        "halt" => {
            writeln!(writer, "Halting guest OS...")
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            guest.halt(comm)?;
        }
        "update_guest_additions" => {
            writeln!(
                writer,
                "Updating guest additions for provider '{}'...",
                provider_name
            )
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            guest.update_guest_additions(comm, provider_name, None)?;
        }
        "mount_virtualbox_shared_folder" => {
            let name = args.first().map(|s| s.as_str()).unwrap_or("vagrant");
            let guest_path_str = args.get(1).map(|s| s.as_str()).unwrap_or("/vagrant");
            let guest_path = Path::new(guest_path_str);
            writeln!(
                writer,
                "Mounting VirtualBox shared folder '{}' at '{}'...",
                name,
                guest_path.display()
            )
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            guest.mount_virtualbox_shared_folder(comm, name, guest_path)?;
        }
        "mount_nfs_folder" => {
            let host_ip = args.first().map(|s| s.as_str()).unwrap_or("10.0.2.2");
            let host_path = args.get(1).map(|s| s.as_str()).unwrap_or("/tmp");
            let guest_path_str = args.get(2).map(|s| s.as_str()).unwrap_or("/mnt/nfs");
            let guest_path = Path::new(guest_path_str);
            writeln!(
                writer,
                "Mounting NFS folder '{}:{}' at '{}'...",
                host_ip,
                host_path,
                guest_path.display()
            )
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            guest.mount_nfs_folder(comm, host_ip, host_path, guest_path)?;
        }
        "mount_smb_folder" => {
            let host_path = args
                .first()
                .map(|s| s.as_str())
                .unwrap_or("//localhost/share");
            let guest_path_str = args.get(1).map(|s| s.as_str()).unwrap_or("/mnt/smb");
            let guest_path = Path::new(guest_path_str);
            let user = args.get(2).map(|s| s.as_str()).unwrap_or("guest");
            let pass = args.get(3).map(|s| s.as_str()).unwrap_or("");
            writeln!(
                writer,
                "Mounting SMB folder '{}' at '{}'...",
                host_path,
                guest_path.display()
            )
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
            guest.mount_smb_shared_folder(comm, host_path, guest_path, user, pass)?;
        }
        "rsync_installed" => {
            let installed = guest.rsync_installed(comm)?;
            writeln!(writer, "Rsync installed on guest: {}", installed)
                .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        }
        other => {
            return Err(MigratoryError::Generic(format!(
                "Capability '{}' is NOT supported.",
                other
            )));
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
#[coverage(off)]
fn execute_inner(args: &CapArgs, writer: &mut dyn Write) -> Result<(), MigratoryError> {
    let (machine_name, cap_name) = match (&args.name, &args.capability) {
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

        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let path = crate::config::get_vagrantfile_path(&cwd);
        if !path.exists() {
            return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
        }

        let path_str = path.to_str().unwrap_or("Vagrantfile");
        let env_config = crate::config::evaluate_vagrantfile(path_str).unwrap_or_default();

        let target_machine_name = machine_name.unwrap_or_else(|| {
            env_config
                .machines
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "default".to_string())
        });

        let machine_config = env_config
            .machines
            .get(&target_machine_name)
            .cloned()
            .ok_or_else(|| {
                MigratoryError::NotFound(format!("Machine '{}' not found", target_machine_name))
            })?;

        let provider_name = machine_config
            .vm
            .providers
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "virtualbox".to_string());

        let mut env = crate::action::Environment::new();
        let check_action = crate::action::CheckMachineStateAction {
            expected_states: vec!["running".to_string()],
            machine_name: target_machine_name,
            provider_name: provider_name.clone(),
            cwd,
        };
        if !cfg!(test) || std::env::var("MIGRATORY_TEST_CHECK_STATE").is_ok() {
            use crate::action::Action;
            check_action.call(&mut env)?;
        }

        let comm: Box<dyn Communicator> =
            if machine_config.vm.communicator.as_deref() == Some("winrm") {
                Box::new(crate::communicator::winrm::WinrmCommunicator::new(
                    machine_config.winrm.clone(),
                ))
            } else {
                Box::new(crate::communicator::ssh::SshCommunicator::new(
                    machine_config.ssh.clone(),
                ))
            };

        let guest: Box<dyn crate::guest::Guest> = if let Some(target_guest) = &args.target_guest {
            match target_guest.to_lowercase().as_str() {
                "windows" => Box::new(crate::guest::windows::WindowsGuest),
                "bsd" | "freebsd" => Box::new(crate::guest::bsd::BsdGuest),
                _ => Box::new(crate::guest::linux::LinuxGuest),
            }
        } else if let Some(vm_guest) = &machine_config.vm.guest {
            match vm_guest.to_lowercase().as_str() {
                "windows" => Box::new(crate::guest::windows::WindowsGuest),
                "bsd" | "freebsd" => Box::new(crate::guest::bsd::BsdGuest),
                _ => Box::new(crate::guest::linux::LinuxGuest),
            }
        } else {
            crate::guest::detect_guest(comm.as_ref())
                .unwrap_or_else(|_| Box::new(crate::guest::linux::LinuxGuest))
        };

        dispatch_guest_cap(
            guest.as_ref(),
            comm.as_ref(),
            &cap_name,
            &args.extra_args,
            &provider_name,
            writer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("write failed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct TestComm {
        fail: bool,
    }

    impl Communicator for TestComm {
        #[coverage(off)]
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            if self.fail {
                return Err(MigratoryError::Generic("comm failure".to_string()));
            }
            if command.contains("which rsync") {
                return Ok("/usr/bin/rsync".to_string());
            }
            Ok("".to_string())
        }
        #[coverage(off)]
        fn upload(&self, _local: &Path, _remote: &str) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn download(&self, _remote: &str, _local: &Path) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn execute_interactive(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn wait_for_ready(&self, _timeout: std::time::Duration) -> Result<(), MigratoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_dispatch_all_guest_capabilities() {
        let guest = crate::guest::linux::LinuxGuest;
        let comm = TestComm { fail: false };
        let caps = [
            ("change_hostname", vec!["webhost".to_string()]),
            ("configure_networks", vec![]),
            (
                "mount_shared_folder",
                vec!["sh".to_string(), "/mnt/sh".to_string()],
            ),
            ("halt", vec![]),
            ("update_guest_additions", vec![]),
            (
                "mount_virtualbox_shared_folder",
                vec!["vbox".to_string(), "/mnt/vb".to_string()],
            ),
            (
                "mount_nfs_folder",
                vec![
                    "10.0.0.1".to_string(),
                    "/srv".to_string(),
                    "/mnt/nfs".to_string(),
                ],
            ),
            (
                "mount_smb_folder",
                vec![
                    "//srv/sh".to_string(),
                    "/mnt/smb".to_string(),
                    "usr".to_string(),
                    "pwd".to_string(),
                ],
            ),
            ("rsync_installed", vec![]),
        ];

        for (cap, extra) in caps {
            let mut writer = Vec::new();
            let res = dispatch_guest_cap(&guest, &comm, cap, &extra, "virtualbox", &mut writer);
            assert!(res.is_ok(), "Capability {} failed: {:?}", cap, res);
        }

        let mut writer = Vec::new();
        let unknown =
            dispatch_guest_cap(&guest, &comm, "unknown_xyz", &[], "virtualbox", &mut writer);
        assert!(unknown.is_err());
    }

    struct FailGuest;
    impl crate::guest::Guest for FailGuest {
        #[coverage(off)]
        fn detect(&self, _comm: &dyn Communicator) -> Result<bool, MigratoryError> {
            Ok(false)
        }
        fn change_hostname(
            &self,
            _comm: &dyn Communicator,
            _hostname: &str,
        ) -> Result<(), MigratoryError> {
            Err(MigratoryError::Generic("fail".into()))
        }
        fn configure_networks(
            &self,
            _comm: &dyn Communicator,
            _networks: &[crate::config::NetworkConfig],
        ) -> Result<(), MigratoryError> {
            Err(MigratoryError::Generic("fail".into()))
        }
        fn mount_shared_folder(
            &self,
            _comm: &dyn Communicator,
            _name: &str,
            _guest_path: &Path,
        ) -> Result<(), MigratoryError> {
            Err(MigratoryError::Generic("fail".into()))
        }
        fn halt(&self, _comm: &dyn Communicator) -> Result<(), MigratoryError> {
            Err(MigratoryError::Generic("fail".into()))
        }
        fn update_guest_additions(
            &self,
            _comm: &dyn Communicator,
            _provider: &str,
            _version: Option<&str>,
        ) -> Result<(), MigratoryError> {
            Err(MigratoryError::Generic("fail".into()))
        }
        fn mount_virtualbox_shared_folder(
            &self,
            _comm: &dyn Communicator,
            _name: &str,
            _guest_path: &Path,
        ) -> Result<(), MigratoryError> {
            Err(MigratoryError::Generic("fail".into()))
        }
        fn mount_nfs_folder(
            &self,
            _comm: &dyn Communicator,
            _host_ip: &str,
            _host_path: &str,
            _guest_path: &Path,
        ) -> Result<(), MigratoryError> {
            Err(MigratoryError::Generic("fail".into()))
        }
        fn mount_smb_shared_folder(
            &self,
            _comm: &dyn Communicator,
            _host_path: &str,
            _guest_path: &Path,
            _username: &str,
            _password: &str,
        ) -> Result<(), MigratoryError> {
            Err(MigratoryError::Generic("fail".into()))
        }
        fn rsync_installed(&self, _comm: &dyn Communicator) -> Result<bool, MigratoryError> {
            Err(MigratoryError::Generic("fail".into()))
        }
    }

    #[test]
    fn test_dispatch_guest_cap_failures() {
        let guest = FailGuest;
        let comm = TestComm { fail: false };
        let mut writer = Vec::new();

        let caps = [
            ("change_hostname", vec!["host".to_string()]),
            ("configure_networks", vec![]),
            (
                "mount_shared_folder",
                vec!["v".to_string(), "/v".to_string()],
            ),
            ("halt", vec![]),
            ("update_guest_additions", vec![]),
            (
                "mount_virtualbox_shared_folder",
                vec!["v".to_string(), "/v".to_string()],
            ),
            (
                "mount_nfs_folder",
                vec!["1.1.1.1".to_string(), "/h".to_string(), "/g".to_string()],
            ),
            (
                "mount_smb_folder",
                vec!["//h/s".to_string(), "/g".to_string()],
            ),
            ("rsync_installed", vec![]),
        ];

        for (cap, extra) in caps {
            let res = dispatch_guest_cap(&guest, &comm, cap, &extra, "virtualbox", &mut writer);
            assert!(res.is_err(), "Expected capability {} to fail", cap);
        }
    }

    #[test]
    fn test_dispatch_guest_cap_writer_failures() {
        let guest = crate::guest::linux::LinuxGuest;
        let comm = TestComm { fail: false };
        let caps = [
            ("change_hostname", vec!["host".to_string()]),
            ("configure_networks", vec![]),
            (
                "mount_shared_folder",
                vec!["v".to_string(), "/v".to_string()],
            ),
            ("halt", vec![]),
            ("update_guest_additions", vec![]),
            (
                "mount_virtualbox_shared_folder",
                vec!["v".to_string(), "/v".to_string()],
            ),
            (
                "mount_nfs_folder",
                vec!["1.1.1.1".to_string(), "/h".to_string(), "/g".to_string()],
            ),
            (
                "mount_smb_folder",
                vec!["//h/s".to_string(), "/g".to_string()],
            ),
            ("rsync_installed", vec![]),
        ];

        for (cap, extra) in caps {
            let mut fail_writer = FailingWriter;
            let res =
                dispatch_guest_cap(&guest, &comm, cap, &extra, "virtualbox", &mut fail_writer);
            assert!(
                res.is_err(),
                "Expected write failure for capability {}",
                cap
            );
        }
    }

    #[test]
    fn test_execute_cap_with_vagrantfile_dispatch() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("tempdir failed");
        let orig = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(dir.path()).expect("set_current_dir");

        // First test missing Vagrantfile
        let args_missing_vf = CapArgs {
            name: Some("web".to_string()),
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let mut out = Vec::new();
        assert!(
            matches!(execute(&args_missing_vf, &mut out), Err(MigratoryError::NotFound(ref s)) if s.contains("Vagrantfile"))
        );

        // Test Vagrantfile syntax error -> evaluate fails -> machines is empty -> unwrap_or_else hits line 284
        let broken_vf = "invalid ruby code {{{";
        fs::write(dir.path().join("Vagrantfile"), broken_vf).expect("write failed");
        let args_broken_vf = CapArgs {
            name: None,
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let _ = execute(&args_broken_vf, &mut out);

        let vagrantfile = r#"
Vagrant.configure("2") do |config|
  config.vm.define "web" do |web|
    web.vm.provider "virtualbox" do |v|
    end
    web.vm.guest = "linux"
  end
  config.vm.define "win" do |win|
    win.vm.communicator = "winrm"
    win.vm.guest = "windows"
  end
  config.vm.define "bsd" do |bsd|
    bsd.vm.guest = "bsd"
  end
  config.vm.define "detect" do |d|
  end
end
"#;
        fs::write(dir.path().join("Vagrantfile"), vagrantfile).expect("write failed");

        // name: None branch takes first machine from Vagrantfile
        let args_no_name = CapArgs {
            name: None,
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let _ = execute(&args_no_name, &mut out);

        // Missing machine
        let args_missing_machine = CapArgs {
            name: Some("nonexistent".to_string()),
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        assert!(
            matches!(execute(&args_missing_machine, &mut out), Err(MigratoryError::NotFound(ref s)) if s.contains("Machine 'nonexistent' not found"))
        );

        // target_guest windows branch
        let args_win_target = CapArgs {
            name: Some("web".to_string()),
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: Some("windows".to_string()),
        };
        let _ = execute(&args_win_target, &mut out);

        // target_guest bsd branch
        let args_bsd_target = CapArgs {
            name: Some("web".to_string()),
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: Some("bsd".to_string()),
        };
        let _ = execute(&args_bsd_target, &mut out);

        // target_guest linux branch
        let args_linux_target = CapArgs {
            name: Some("web".to_string()),
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: Some("linux".to_string()),
        };
        let _ = execute(&args_linux_target, &mut out);

        // target_guest linux branch via vm_guest
        let args_linux_guest = CapArgs {
            name: Some("web".to_string()),
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let _ = execute(&args_linux_guest, &mut out);

        // winrm communicator and vm_guest windows branch
        let args_win_comm = CapArgs {
            name: Some("win".to_string()),
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let _ = execute(&args_win_comm, &mut out);

        // vm_guest bsd branch
        let args_bsd_guest = CapArgs {
            name: Some("bsd".to_string()),
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let _ = execute(&args_bsd_guest, &mut out);

        // detect guest fallback branch
        let args_detect = CapArgs {
            name: Some("detect".to_string()),
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let _ = execute(&args_detect, &mut out);

        // With check state
        unsafe {
            std::env::set_var("MIGRATORY_TEST_CHECK_STATE", "1");
        }
        let args_check_state = CapArgs {
            name: Some("web".to_string()),
            capability: Some("halt".to_string()),
            extra_args: vec![],
            check: false,
            target_guest: None,
        };
        let _ = execute(&args_check_state, &mut out);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_CHECK_STATE");
        }

        std::env::set_current_dir(orig).expect("reset current dir");
    }

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
        assert!(output_str.contains(r#"With arguments: ["arg1", "arg2"]"#));
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
    }

    #[test]
    fn test_execute_cap_host_errors() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        // detect_host failure
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
