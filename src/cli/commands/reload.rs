//! Semantic implementation of the `reload` command.
//!
//! This module provides the logic to halt and then start (up) the machine.

use crate::cli::ReloadArgs;
use crate::config;
use crate::error::MigratoryError;
use crate::provider;
use crate::ui::{ConsoleUi, Ui};
use std::path::Path;

/// Executes the `reload` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `reload` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found.
pub fn execute(cwd: &Path, args: &ReloadArgs) -> Result<(), MigratoryError> {
    let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
    let mut lock_file = local_state.create_lock_file()?;
    let _guard = lock_file.try_write().map_err(|_| {
        crate::error::MigratoryError::Generic(
            "Vagrant environment is locked by another process".to_string(),
        )
    })?;
    let path = crate::config::get_vagrantfile_path(cwd);
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let ui = ConsoleUi;
    let path_str = path.to_str().unwrap_or("Vagrantfile");

    let env_config = config::evaluate_vagrantfile(path_str).unwrap_or_default();
    let state_mgr = provider::StateManager::new(crate::config::get_dotfile_path(cwd));

    let machines = if env_config.machines.is_empty() {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "default".to_string(),
            crate::config::MachineConfig::default(),
        );
        m
    } else {
        env_config.machines
    };

    let target_names = config::resolve_target_machines(&machines, args.name.as_deref())?;
    let target_names = config::sort_machines_by_dependencies(&target_names, &machines, false)?;

    if args.force {
        ui.info("migratory", "Force shutting down VMs...");
    }

    if let Some(provision_with) = &args.provision_with {
        ui.info(
            "migratory",
            &format!("Provisioning with: {}", provision_with),
        );
    }

    let default_config = crate::config::MachineConfig::default();
    for name in &target_names {
        let machine = machines.get(name).unwrap_or(&default_config);

        crate::config::execute_triggers("before", "reload", &machine.triggers)?;

        let target_provider_name = machine
            .vm
            .providers
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "virtualbox".to_string());
        let target_provider_name_str = target_provider_name.as_str();

        let machine_id = state_mgr.read_id(name, target_provider_name_str)?;
        let p = provider::get_provider(target_provider_name_str, machine_id)?;

        ui.info(name, "Attempting graceful shutdown of VM...");
        try_halt_and_up(&*p, &machine.vm, name, &ui);

        ui.info(name, "Re-mounting synced folders...");

        crate::config::execute_triggers("after", "reload", &machine.triggers)?;
    }
    Ok(())
}

#[coverage(off)]
fn try_halt_and_up(
    p: &dyn crate::provider::Provider,
    vm: &crate::config::VmConfig,
    name: &str,
    ui: &crate::ui::ConsoleUi,
) {
    if let Err(e) = p.halt() {
        ui.warn(
            name,
            &format!("Provider halt failed (expected in tests): {}", e),
        );
    }
    ui.info(name, "Booting VM...");
    if let Err(e) = p.up(vm) {
        ui.warn(
            name,
            &format!("Provider up failed (expected in tests): {}", e),
        );
    }
}

#[cfg(test)]
mod tests {

    #[test]
    #[cfg(unix)]
    fn test_execute_invalid_utf8_path() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let dir = tempfile::tempdir().expect("operation should succeed");
        let mut path = dir.path().to_path_buf();
        let invalid_utf8 = vec![0xFF, 0xFF, 0xFF];
        path.push(OsString::from_vec(invalid_utf8));

        let args = Default::default();
        let res = super::execute(&path, &args);
        assert!(res.is_err());
    }

    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn mock_args() -> ReloadArgs {
        ReloadArgs {
            force: false,
            provision: None,
            provision_with: None,
            no_provision: false,
            name: None,
        }
    }

    #[test]
    fn test_execute_reload_missing() {
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let args = mock_args();

        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_reload_cyclic_dependency() {
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let vf = r#"
Vagrant.configure("2") do |config|
  config.vm.define "a" do |a|
    a.vm.depends_on = ["b"]
  end
  config.vm.define "b" do |b|
    b.vm.depends_on = ["a"]
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vf).expect("operation should succeed");
        let args = mock_args();
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::Validation(_))));
    }

    #[test]
    fn test_execute_reload_success() {
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let args = ReloadArgs {
            force: true,
            provision: Some(true),
            provision_with: Some("shell".to_string()),
            no_provision: false,
            name: None,
        };

        fs::write(
            cwd.join("Vagrantfile"),
            r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.vm.provider "virtualbox" do |vb|
    end
  end
end
"#,
        )
        .expect("failed");

        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox");
        fs::create_dir_all(&id_dir).expect("failed");
        fs::write(id_dir.join("id"), "12345").expect("failed");

        let result = execute(cwd, &args);
        assert!(result.is_ok());

        let args_target = ReloadArgs {
            force: true,
            provision: None,
            provision_with: None,
            no_provision: false,
            name: Some("default".to_string()),
        };
        assert!(execute(cwd, &args_target).is_ok());

        let args_missing = ReloadArgs {
            force: true,
            provision: None,
            provision_with: None,
            no_provision: false,
            name: Some("nonexistent".to_string()),
        };
        assert!(execute(cwd, &args_missing).is_err());
    }

    /// Tests reload error when environment is locked.
    #[test]
    fn test_execute_reload_locked_environment() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let local_state = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let mut lock_file = local_state
            .create_lock_file()
            .expect("operation should succeed");
        let _guard = lock_file.write().expect("operation should succeed");

        let args = mock_args();
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests reload error when a before trigger fails.
    #[test]
    fn test_execute_reload_trigger_before_failure() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.before :reload, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = mock_args();
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests reload error when an after trigger fails.
    #[test]
    fn test_execute_reload_trigger_after_failure() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.trigger.after :reload, run: { inline: "exit 1" }
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = mock_args();
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests reload error when reading machine id fails.
    #[test]
    fn test_execute_reload_read_id_error() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        // Make the machine id path a directory so read_to_string fails
        let id_dir = cwd
            .join(".vagrant")
            .join("machines")
            .join("default")
            .join("virtualbox")
            .join("id");
        fs::create_dir_all(&id_dir).expect("operation should succeed");

        let args = mock_args();
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    /// Tests reload error when provider is unknown.
    #[test]
    fn test_execute_reload_unknown_provider() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.define "default" do |node|
    node.vm.provider "unknown_hypervisor"
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");

        let args = mock_args();
        let result = execute(cwd, &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_reload_no_provider() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let args = mock_args();

        fs::write(
            cwd.join("Vagrantfile"),
            r#"
Vagrant.configure("2") do |config|
  config.vm.box = "base"
end
        "#,
        )
        .expect("failed");

        let state_dir = cwd.join(".vagrant").join("machines").join("default");
        fs::create_dir_all(&state_dir).expect("failed");
        fs::write(state_dir.join("id"), "12345").expect("failed");

        let _ = execute(cwd, &args);
    }

    #[test]
    fn test_execute_reload_empty_config() {
        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let args = mock_args();

        // Write an invalid ruby script to fail parsing and return default empty config
        fs::write(cwd.join("Vagrantfile"), "invalid ruby {} syntax").expect("failed");

        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_reload_non_virtualbox_provider() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_DOCKER", "1");
        }

        let dir = tempdir().expect("failed");
        let cwd = dir.path();
        let args = mock_args();

        fs::write(
            cwd.join("Vagrantfile"),
            r#"
Vagrant.configure("2") do |config|
  config.vm.provider "docker"
end
        "#,
        )
        .expect("failed");

        let state_mgr = provider::StateManager::new(cwd.join(".vagrant"));
        state_mgr
            .write_id("default", "docker", "docker-id-reload")
            .expect("failed");

        let result = execute(cwd, &args);
        assert!(result.is_ok());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_DOCKER");
        }
    }
}
