//! Box command implementation.
//!
//! Handles the `box` subcommand and its various operations such as
//! `add`, `list`, `outdated`, `prune`, `remove`, `repackage`, and `update`.

use crate::box_manager::BoxManager;
use crate::cli::BoxCommands;
use crate::cloud::CloudClient;
use crate::error::MigratoryError;
use crate::ui::{ConsoleUi, Ui};
use std::io::Write;

/// Executes the `box` command.
///
/// # Arguments
///
/// * `cmd` - The parsed `BoxCommands` subcommand.
/// * `writer` - A writable destination for output (e.g. stdout).
///
/// # Returns
///
/// Returns `Ok(())` on successful execution, or a `MigratoryError` on failure.
///
/// # Errors
///
/// Returns a `MigratoryError` if writing to the output stream fails.
pub fn execute(cmd: &BoxCommands, mut writer: impl Write) -> Result<(), MigratoryError> {
    execute_inner(cmd, &mut writer)
}

/// Internal non-generic implementation of box command execution.
///
/// # Arguments
///
/// * `cmd` - The box subcommand to execute.
/// * `writer` - A mutable reference to a trait object writer.
///
/// # Returns
///
/// Returns `Ok(())` on successful execution, or a `MigratoryError` on failure.
///
/// # Errors
///
/// Returns a `MigratoryError` if writing to the output stream fails.
fn execute_inner(cmd: &BoxCommands, writer: &mut dyn Write) -> Result<(), MigratoryError> {
    let home_dir = std::env::var("VAGRANT_HOME").unwrap_or_else(|_| ".vagrant.d".to_string());
    let global_dir = std::path::Path::new(&home_dir);

    match cmd {
        BoxCommands::Add(args) => {
            let ui = ConsoleUi;
            ui.info("box", &format!("Adding box '{}'...", args.name));

            // Check if it's a URL or path, if not, treat as HashiCorp Cloud tag
            let is_url_or_path = args.name.contains("://") || args.name.ends_with(".box");

            let box_mgr = BoxManager::new(global_dir);

            let mut download_url = args.name.clone();
            let mut target_version = args.box_version.clone().unwrap_or_else(|| "0".to_string());
            let mut target_provider = args
                .provider
                .clone()
                .unwrap_or_else(|| "virtualbox".to_string());

            if !is_url_or_path {
                ui.detail("box", "Fetching metadata from Vagrant Cloud...");
                let cloud_client = CloudClient::new()?;
                let metadata = cloud_client.fetch_metadata(&args.name)?;

                // Find matching version/provider
                let mut found_url = None;
                for v in metadata.versions {
                    let version_match = args.box_version.is_none()
                        || args.box_version.as_deref() == Some(&v.version);
                    if version_match {
                        for p in &v.providers {
                            let provider_match = args.provider.is_none()
                                || args.provider.as_deref() == Some(&p.name);
                            if provider_match {
                                found_url = Some(p.url.clone());
                                target_version = v.version.clone();
                                target_provider = p.name.clone();
                                break;
                            }
                        }
                    }
                    if found_url.is_some() {
                        break;
                    }
                }

                download_url = found_url.ok_or_else(|| {
                    MigratoryError::Generic(format!(
                        "Could not find a matching provider '{}' for box '{}'",
                        target_provider, args.name
                    ))
                })?;
            }

            ui.info("box", &format!("Downloading box from: {}", download_url));

            let temp_box_file =
                std::env::temp_dir().join(format!("migratory-box-{}.box", uuid::Uuid::new_v4()));

            box_mgr.download_box(
                &download_url,
                &temp_box_file,
                args.checksum.as_deref(),
                args.checksum_type.as_deref(),
            )?;

            ui.info("box", "Extracting box...");
            box_mgr.add_box(
                &args.name,
                &target_version,
                &target_provider,
                &temp_box_file,
            )?;

            let _out = std::fs::remove_file(&temp_box_file);

            ui.info("box", "Box added successfully.");
        }
        BoxCommands::List(_args) => {
            let manager = BoxManager::new(global_dir);
            if !manager.global_boxes_dir.exists() {
                writeln!(
                    writer,
                    "There are no installed boxes! Use `migratory box add` to add some."
                )?;
                return Ok(());
            }

            let mut boxes = vec![];
            for box_entry in std::fs::read_dir(&manager.global_boxes_dir)
                .into_iter()
                .flatten()
                .flatten()
            {
                let box_name = box_entry
                    .file_name()
                    .to_string_lossy()
                    .replace("-VAGRANTSLASH-", "/");
                for ver_entry in std::fs::read_dir(box_entry.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    let version = ver_entry.file_name().to_string_lossy().to_string();
                    for prov_entry in std::fs::read_dir(ver_entry.path())
                        .into_iter()
                        .flatten()
                        .flatten()
                    {
                        let provider = prov_entry.file_name().to_string_lossy().to_string();
                        boxes.push(format!("{} ({}, {})", box_name, provider, version));
                    }
                }
            }

            if boxes.is_empty() {
                writeln!(
                    writer,
                    "There are no installed boxes! Use `migratory box add` to add some."
                )?;
            } else {
                for b in boxes {
                    writeln!(writer, "{}", b)?;
                }
            }
        }
        BoxCommands::Remove(args) => {
            let manager = BoxManager::new(global_dir);
            let safe_name = args.name.replace("/", "-VAGRANTSLASH-");
            let box_dir = manager.global_boxes_dir.join(safe_name);

            if !box_dir.exists() {
                return Err(MigratoryError::NotFound(format!("Box '{}'", args.name)));
            }

            std::fs::remove_dir_all(&box_dir)?;
            writeln!(writer, "Box '{}' removed.", args.name)?;
        }
        BoxCommands::Outdated(args) => {
            let manager = BoxManager::new(global_dir);
            let ui = ConsoleUi;

            if !manager.global_boxes_dir.exists() {
                writeln!(writer, "There are no installed boxes to check.")?;
                return Ok(());
            }

            let cloud_client = CloudClient::new()?;
            let mut any_outdated = false;

            if args.global {
                ui.info("box", "Checking for outdated boxes globally...");
                for box_entry in std::fs::read_dir(&manager.global_boxes_dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    let box_name = box_entry
                        .file_name()
                        .to_string_lossy()
                        .replace("-VAGRANTSLASH-", "/");

                    let max_local_version = std::fs::read_dir(box_entry.path())
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .max()
                        .unwrap_or_default();

                    if max_local_version.is_empty() {
                        continue;
                    }

                    ui.info(
                        "box",
                        &format!(
                            "Checking '{}' (latest local: v{})...",
                            box_name, max_local_version
                        ),
                    );
                    if let Ok(metadata) = cloud_client.fetch_metadata(&box_name) {
                        let max_remote_version = metadata
                            .versions
                            .into_iter()
                            .map(|v| v.version)
                            .max()
                            .unwrap_or_default();
                        if !max_remote_version.is_empty() && max_remote_version > max_local_version
                        {
                            writeln!(
                                writer,
                                "Box '{}' is outdated! Local: {}, Remote: {}",
                                box_name, max_local_version, max_remote_version
                            )?;
                            any_outdated = true;
                        } else {
                            writeln!(writer, "Box '{}' is up to date.", box_name)?;
                        }
                    }
                }

                writeln!(writer, "Box is outdated: {}", any_outdated)?;
            } else {
                writeln!(writer, "Box is outdated: false")?;
            }
        }
        BoxCommands::Update(args) => {
            let manager = BoxManager::new(global_dir);
            let ui = ConsoleUi;

            if !manager.global_boxes_dir.exists() {
                writeln!(writer, "There are no installed boxes to update.")?;
                return Ok(());
            }

            let cloud_client = CloudClient::new()?;
            let mut any_updated = false;

            let target_box = args.box_name.clone();

            for box_entry in std::fs::read_dir(&manager.global_boxes_dir)
                .into_iter()
                .flatten()
                .flatten()
            {
                let box_name = box_entry
                    .file_name()
                    .to_string_lossy()
                    .replace("-VAGRANTSLASH-", "/");

                if target_box.as_ref().is_some_and(|tb| &box_name != tb) {
                    continue;
                }

                let max_local_version = std::fs::read_dir(box_entry.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .max()
                    .unwrap_or_default();

                if max_local_version.is_empty() {
                    continue;
                }

                ui.info("box", &format!("Checking for updates to '{}'...", box_name));

                if let Ok(metadata) = cloud_client.fetch_metadata(&box_name) {
                    let mut max_remote_version = String::new();
                    let mut max_remote_url = String::new();
                    let mut max_remote_provider = String::new();

                    for v in metadata.versions {
                        if v.version > max_remote_version {
                            for p in &v.providers {
                                let provider_match = args.provider.is_none()
                                    || args.provider.as_deref() == Some(&p.name);
                                if provider_match {
                                    max_remote_version = v.version.clone();
                                    max_remote_url = p.url.clone();
                                    max_remote_provider = p.name.clone();
                                    break;
                                }
                            }
                        }
                    }

                    if !max_remote_version.is_empty() && max_remote_version > max_local_version {
                        ui.info(
                            "box",
                            &format!(
                                "Updating '{}' from version {} to version {}...",
                                box_name, max_local_version, max_remote_version
                            ),
                        );

                        let temp_box_file = std::env::temp_dir()
                            .join(format!("migratory-box-{}.box", uuid::Uuid::new_v4()));

                        manager.download_box(&max_remote_url, &temp_box_file, None, None)?;
                        manager.add_box(
                            &box_name,
                            &max_remote_version,
                            &max_remote_provider,
                            &temp_box_file,
                        )?;
                        let _ = std::fs::remove_file(&temp_box_file);

                        any_updated = true;
                    } else {
                        ui.info("box", &format!("Box '{}' is already up to date.", box_name));
                    }
                }
            }

            if !any_updated {
                writeln!(writer, "No boxes were updated.")?;
            }
        }
        BoxCommands::Prune(args) => {
            let manager = BoxManager::new(global_dir);
            let ui = ConsoleUi;
            manager.prune(
                args.provider.as_deref(),
                args.keep_active_boxes,
                args.dry_run,
                &ui,
            )?;
            writeln!(writer, "Box pruned successfully.")?;
        }
        BoxCommands::Repackage(args) => {
            let manager = BoxManager::new(global_dir);
            manager.repackage(&args.name, &args.provider)?;
            writeln!(writer, "Box repackaged successfully.")?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(missing_docs)]
pub mod tests {
    #[allow(missing_docs)]
    pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    use super::*;
    use crate::cli::*;

    pub(crate) struct FailingWriter;
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

    #[test]
    fn test_failing_writer_flush() {
        let mut writer = FailingWriter;
        assert!(writer.flush().is_ok());
    }

    pub(crate) fn mock_add_args() -> BoxAddArgs {
        BoxAddArgs {
            name: "test/box".to_string(),
            box_name: None,
            force: false,
            clean: false,
            provider: None,
            box_version: None,
            checksum: None,
            checksum_type: None,
            insecure: false,
            architecture: None,
            cacert: None,
            capath: None,
            cert: None,
            location_trusted: false,
        }
    }

    pub(crate) fn mock_list_args() -> BoxListArgs {
        BoxListArgs { box_info: false }
    }

    pub(crate) fn mock_remove_args() -> BoxRemoveArgs {
        BoxRemoveArgs {
            name: "test/box".to_string(),
            provider: None,
            box_version: None,
            all: false,
            force: false,
            architecture: None,
            all_providers: false,
            all_architectures: false,
        }
    }

    pub(crate) fn mock_outdated_args() -> BoxOutdatedArgs {
        BoxOutdatedArgs {
            global: false,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
            force: false,
        }
    }

    pub(crate) fn mock_update_args() -> BoxUpdateArgs {
        BoxUpdateArgs {
            box_name: None,
            provider: None,
            architecture: None,
            force: false,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
        }
    }

    pub(crate) fn mock_prune_args() -> BoxPruneArgs {
        BoxPruneArgs {
            provider: None,
            dry_run: false,
            keep_active_boxes: true,
            name: None,
            force: false,
        }
    }

    pub(crate) fn mock_repackage_args() -> BoxRepackageArgs {
        BoxRepackageArgs {
            name: "test/box".to_string(),
            provider: "virtualbox".to_string(),
            version: "1.0.0".to_string(),
        }
    }

    #[test]
    fn test_execute_box_add() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let dir = tempfile::tempdir().expect("operation should succeed");

        unsafe {
            std::env::set_var("VAGRANT_HOME", dir.path());
        }

        let _cmd = BoxCommands::Add(mock_add_args());
        let mut out = Vec::new();
        let mut args = mock_add_args();
        let box_path = dir.path().join("dummy.box");
        std::fs::write(&box_path, b"dummy content").expect("operation should succeed");

        args.name = box_path
            .to_str()
            .expect("operation should succeed")
            .to_string();
        let cmd = BoxCommands::Add(args);

        let result = execute(&cmd, &mut out);
        assert!(result.is_err());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_execute_box_add_write_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        use httpmock::prelude::*;

        let server = MockServer::start();
        let metadata_json = serde_json::json!({
            "name": "test/write_error",
            "description_markdown": "test box",
            "short_description": "test box",
            "versions": []
        });

        let _mock_call = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/write_error");
            then.status(200).json_body(metadata_json);
        });

        let temp = tempfile::tempdir().expect("operation should succeed");
        std::fs::create_dir_all(temp.path().join("boxes")).expect("operation should succeed");
        std::fs::create_dir_all(temp.path().join("tmp")).expect("operation should succeed");
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
            std::env::set_var("VAGRANT_HOME", temp.path());
        }

        let mut args = mock_add_args();
        args.name = "test/write_error".to_string();
        let cmd = BoxCommands::Add(args);
        let mut out = FailingWriter;
        let result = execute(&cmd, &mut out);
        assert!(result.is_err());

        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_execute_box_list() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::List(mock_list_args());
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("There are no installed boxes"));
    }

    #[test]
    fn test_execute_box_list_write_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::List(mock_list_args());
        let mut out = FailingWriter;
        let result = execute(&cmd, &mut out);
        let _err = result.expect_err("operation should fail");
    }

    #[test]
    fn test_execute_box_remove() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::Remove(mock_remove_args());
        let mut out = Vec::new();
        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_box_remove_write_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::Remove(mock_remove_args());
        let mut out = FailingWriter;
        let result = execute(&cmd, &mut out);
        let _err = result.expect_err("operation should fail");
    }

    #[test]
    fn test_execute_box_outdated() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::Outdated(mock_outdated_args());
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("There are no installed boxes to check."));
    }

    #[test]
    fn test_execute_box_outdated_write_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::Outdated(mock_outdated_args());
        let mut out = FailingWriter;
        let result = execute(&cmd, &mut out);
        let _err = result.expect_err("operation should fail");
    }

    #[test]
    fn test_execute_box_update() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::Update(mock_update_args());
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("There are no installed boxes to update"));
    }

    #[test]
    fn test_execute_box_update_write_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::Update(mock_update_args());
        let mut out = FailingWriter;
        let result = execute(&cmd, &mut out);
        let _err = result.expect_err("operation should fail");
    }

    #[test]
    fn test_execute_box_prune() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::Prune(mock_prune_args());
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Box pruned successfully.");
    }

    #[test]
    fn test_execute_box_prune_write_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::Prune(mock_prune_args());
        let mut out = FailingWriter;
        let result = execute(&cmd, &mut out);
        let _err = result.expect_err("operation should fail");
    }

    #[test]
    fn test_execute_box_repackage() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::Repackage(mock_repackage_args());
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_err());
    }

    #[test]
    fn test_execute_box_repackage_write_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let cmd = BoxCommands::Repackage(mock_repackage_args());
        let mut out = FailingWriter;
        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_box_add_cloud_failure() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let mut args = mock_add_args();
        args.name = "invalid/tag/for/cloud".to_string(); // not ending with .box or ://
        let cmd = BoxCommands::Add(args);
        let mut out = Vec::new();
        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_box_prune_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let dir = tempfile::tempdir().expect("operation should succeed");
        let file_path = dir.path().join(".vagrant.d");
        std::fs::create_dir_all(&file_path).expect("operation should succeed");
        let boxes_file = file_path.join("boxes");
        std::fs::write(&boxes_file, "not a dir").expect("operation should succeed");
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                file_path.to_str().expect("operation should succeed"),
            );
        }

        let cmd = BoxCommands::Prune(mock_prune_args());
        let mut out = Vec::new();
        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_execute_box_repackage_error() {
        let mut args = mock_repackage_args();
        args.name = "".to_string(); // Will trigger error in repackage
        let cmd = BoxCommands::Repackage(args);
        let mut out = Vec::new();
        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_box_add_cloud_paths() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use httpmock::prelude::*;

        let server = MockServer::start();

        let mut tar_gz = Vec::new();
        {
            let enc = GzEncoder::new(&mut tar_gz, Compression::default());
            let mut tar = tar::Builder::new(enc);
            tar.finish().expect("operation should succeed");
        }

        let box_mock = server.mock(|when, then| {
            when.method(GET).path("/box.box");
            then.status(200).body(tar_gz.clone());
        });

        let metadata_json_success = serde_json::json!({
            "name": "test/box",
            "description_markdown": "test box",
            "short_description": "test box",
            "versions": [
                {
                    "version": "1.0.0",
                    "providers": [
                        {
                            "name": "virtualbox",
                            "url": server.url("/box.box")
                        }
                    ]
                }
            ]
        });

        let metadata_mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json_success);
        });

        let temp = tempfile::tempdir().expect("operation should succeed");
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
            std::env::set_var("VAGRANT_HOME", temp.path());
        }

        let mut args_success = mock_add_args();
        args_success.name = "test/box".to_string();
        args_success.clean = true;
        let cmd_success = BoxCommands::Add(args_success);

        let mut out = Vec::new();
        let result_success = execute(&cmd_success, &mut out);
        assert!(result_success.is_ok(), "{:?}", result_success);
        metadata_mock.assert_calls(1);
        box_mock.assert_calls(1);

        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("VAGRANT_HOME");
        }
    }
}

#[cfg(test)]
mod extra_tests {
    use super::*;
    use crate::cli::*;
    use httpmock::prelude::*;
    use std::fs;
    use tempfile::tempdir;

    pub(crate) fn setup_fake_boxes(dir: &std::path::Path) {
        let boxes_dir = dir.join("boxes");
        fs::create_dir_all(&boxes_dir).expect("operation should succeed");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        let v1_dir = box_path.join("1.0.0").join("virtualbox");
        let v2_dir = box_path.join("2.0.0").join("virtualbox");
        fs::create_dir_all(&v1_dir).expect("operation should succeed");
        fs::create_dir_all(&v2_dir).expect("operation should succeed");
    }

    #[test]
    fn test_list_with_boxes() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        setup_fake_boxes(dir.path());
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let cmd = BoxCommands::List(BoxListArgs { box_info: false });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());
        let output = String::from_utf8(out).expect("operation should succeed");
        assert!(output.contains("test/box (virtualbox, 1.0.0)"));
        assert!(output.contains("test/box (virtualbox, 2.0.0)"));

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_list_write_error_with_boxes() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        setup_fake_boxes(dir.path());
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let cmd = BoxCommands::List(BoxListArgs { box_info: false });
        struct FailingWriter;
        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "err"))
            }
            #[coverage(off)]
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut out = FailingWriter;
        assert!(execute(&cmd, &mut out).is_err());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_remove_success() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        setup_fake_boxes(dir.path());
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let cmd = BoxCommands::Remove(BoxRemoveArgs {
            name: "test/box".to_string(),
            provider: None,
            box_version: None,
            all: false,
            force: false,
            architecture: None,
            all_providers: false,
            all_architectures: false,
        });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_remove_write_error() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        setup_fake_boxes(dir.path());
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let cmd = BoxCommands::Remove(BoxRemoveArgs {
            name: "test/box".to_string(),
            provider: None,
            box_version: None,
            all: false,
            force: false,
            architecture: None,
            all_providers: false,
            all_architectures: false,
        });
        struct FailingWriter;
        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "err"))
            }
            #[coverage(off)]
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut out = FailingWriter;
        assert!(execute(&cmd, &mut out).is_err());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_outdated_global_false() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        setup_fake_boxes(dir.path());
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let cmd = BoxCommands::Outdated(BoxOutdatedArgs {
            global: false,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
            force: false,
        });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());
        let output = String::from_utf8(out).expect("operation should succeed");
        assert!(output.contains("Box is outdated: false"));

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_outdated_global_true_with_updates() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        setup_fake_boxes(dir.path());
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
        }
        let metadata_json = serde_json::json!({
            "name": "test/box",
            "description_markdown": "test box",
            "short_description": "test box",
            "versions": [
                {
                    "version": "3.0.0",
                    "providers": [{"name": "virtualbox", "url": "some_url"}]
                }
            ]
        });
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let cmd = BoxCommands::Outdated(BoxOutdatedArgs {
            global: true,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
            force: false,
        });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());
        let output = String::from_utf8(out).expect("operation should succeed");
        assert!(output.contains("Box 'test/box' is outdated! Local: 2.0.0, Remote: 3.0.0"));
        assert!(output.contains("Box is outdated: true"));

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::remove_var("VAGRANT_CLOUD_URL");
        }
    }

    #[test]
    fn test_outdated_global_true_no_updates() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        setup_fake_boxes(dir.path());
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
        }
        let metadata_json = serde_json::json!({
            "name": "test/box",
            "description_markdown": "test box",
            "short_description": "test box",
            "versions": [
                {
                    "version": "1.0.0",
                    "providers": [{"name": "virtualbox", "url": "some_url"}]
                }
            ]
        });
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let cmd = BoxCommands::Outdated(BoxOutdatedArgs {
            global: true,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
            force: false,
        });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());
        let output = String::from_utf8(out).expect("operation should succeed");
        assert!(output.contains("Box 'test/box' is up to date."));

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::remove_var("VAGRANT_CLOUD_URL");
        }
    }

    #[test]
    fn test_update_with_updates() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        setup_fake_boxes(dir.path());
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
        }

        let mut tar_gz = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tar_gz, flate2::Compression::default());
            let mut tar = tar::Builder::new(enc);
            tar.finish().expect("operation should succeed");
        }
        let _box_mock = server.mock(|when, then| {
            when.method(GET).path("/box.box");
            then.status(200).body(tar_gz);
        });

        let metadata_json = serde_json::json!({
            "name": "test/box",
            "description_markdown": "test box",
            "short_description": "test box",
            "versions": [
                {
                    "version": "3.0.0",
                    "providers": [{"name": "virtualbox", "url": server.url("/box.box")}]
                }
            ]
        });
        let _metadata_mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let cmd = BoxCommands::Update(BoxUpdateArgs {
            box_name: None,
            provider: None,
            architecture: None,
            force: false,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
        });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        // Check if v3.0.0 was downloaded
        let v3_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("3.0.0")
            .join("virtualbox");
        assert!(v3_dir.exists());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::remove_var("VAGRANT_CLOUD_URL");
        }
    }

    #[test]
    fn test_update_no_updates() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        setup_fake_boxes(dir.path());
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
        }

        let metadata_json = serde_json::json!({
            "name": "test/box",
            "description_markdown": "test box",
            "short_description": "test box",
            "versions": [
                {
                    "version": "1.0.0",
                    "providers": [{"name": "virtualbox", "url": "some_url"}]
                }
            ]
        });
        let _metadata_mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let cmd = BoxCommands::Update(BoxUpdateArgs {
            box_name: None,
            provider: None,
            architecture: None,
            force: false,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
        });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());
        let output = String::from_utf8(out).expect("operation should succeed");
        assert!(output.contains("No boxes were updated."));

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::remove_var("VAGRANT_CLOUD_URL");
        }
    }

    #[test]
    fn test_repackage_success_cmd() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        setup_fake_boxes(dir.path());
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        // Add a fake file to be repackaged so tar doesn't fail if it requires content, wait actually BoxManager handles it.

        let cmd = BoxCommands::Repackage(BoxRepackageArgs {
            name: "test/box".to_string(),
            provider: "virtualbox".to_string(),
            version: "2.0.0".to_string(),
        });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        let package_name = "package-test-VAGRANTSLASH-box-2.0.0-virtualbox.box";
        let dest_file = std::env::current_dir()
            .unwrap_or(std::path::PathBuf::from("."))
            .join(package_name);
        assert!(dest_file.exists());
        let _ = fs::remove_file(dest_file);

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }
}

#[cfg(test)]
mod extra_tests2 {
    use super::*;
    use crate::cli::*;
    use httpmock::prelude::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_add_with_version_and_provider_match() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();

        let mut tar_gz = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tar_gz, flate2::Compression::default());
            let mut tar = tar::Builder::new(enc);
            tar.finish().expect("operation should succeed");
        }
        let _box_mock = server.mock(|when, then| {
            when.method(GET).path("/box.box");
            then.status(200).body(tar_gz);
        });

        let metadata_json = serde_json::json!({
            "name": "test/box",
            "description_markdown": "test box",
            "short_description": "test box",
            "versions": [
                {
                    "version": "1.0.0",
                    "providers": [{"name": "virtualbox", "url": server.url("/box.box")}]
                }
            ]
        });
        let _metadata_mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let temp = tempdir().expect("operation should succeed");
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
            std::env::set_var(
                "VAGRANT_HOME",
                temp.path().to_str().expect("operation should succeed"),
            );
        }

        let mut args = super::tests::mock_add_args();
        args.name = "test/box".to_string();
        args.box_version = Some("1.0.0".to_string());
        args.provider = Some("virtualbox".to_string());
        let cmd = BoxCommands::Add(args);

        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_list_empty_boxes_dir() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let boxes_dir = dir.path().join("boxes");
        fs::create_dir_all(&boxes_dir).expect("operation should succeed"); // Exists but is empty!

        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let cmd = BoxCommands::List(BoxListArgs { box_info: false });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        struct FailingWriter;
        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "err"))
            }
            #[coverage(off)]
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut fail_out = FailingWriter;
        assert!(execute(&cmd, &mut fail_out).is_err());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_outdated_empty_version_dir() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let boxes_dir = dir.path().join("boxes");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        fs::create_dir_all(&box_path).expect("operation should succeed"); // Box dir exists but has no versions

        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let cmd = BoxCommands::Outdated(BoxOutdatedArgs {
            global: true,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
            force: false,
        });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_outdated_write_error_with_updates() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let boxes_dir = dir.path().join("boxes");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        let v1_dir = box_path.join("1.0.0").join("virtualbox");
        fs::create_dir_all(&v1_dir).expect("operation should succeed");

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let metadata_json = serde_json::json!({
            "name": "test/box",
            "description_markdown": "test box",
            "short_description": "test box",
            "versions": [
                {
                    "version": "3.0.0",
                    "providers": [{"name": "virtualbox", "url": "some_url"}]
                }
            ]
        });
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let cmd = BoxCommands::Outdated(BoxOutdatedArgs {
            global: true,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
            force: false,
        });
        struct FailingWriter;
        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "err"))
            }
            #[coverage(off)]
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut fail_out = FailingWriter;
        assert!(execute(&cmd, &mut fail_out).is_err());

        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_update_target_box_mismatch() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let boxes_dir = dir.path().join("boxes");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        let v1_dir = box_path.join("1.0.0").join("virtualbox");
        fs::create_dir_all(&v1_dir).expect("operation should succeed");

        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let cmd = BoxCommands::Update(BoxUpdateArgs {
            box_name: Some("other/box".to_string()),
            provider: None,
            architecture: None,
            force: false,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
        });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_update_empty_version_dir() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let boxes_dir = dir.path().join("boxes");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        fs::create_dir_all(&box_path).expect("operation should succeed"); // Box dir exists but has no versions

        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let cmd = BoxCommands::Update(BoxUpdateArgs {
            box_name: None,
            provider: None,
            architecture: None,
            force: false,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
        });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_update_with_provider_filter() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let boxes_dir = dir.path().join("boxes");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        let v1_dir = box_path.join("1.0.0").join("virtualbox");
        fs::create_dir_all(&v1_dir).expect("operation should succeed");

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let mut tar_gz = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tar_gz, flate2::Compression::default());
            let mut tar = tar::Builder::new(enc);
            tar.finish().expect("operation should succeed");
        }
        let _box_mock = server.mock(|when, then| {
            when.method(GET).path("/box.box");
            then.status(200).body(tar_gz);
        });

        let metadata_json = serde_json::json!({
            "name": "test/box",
            "description_markdown": "test box",
            "short_description": "test box",
            "versions": [
                {
                    "version": "3.0.0",
                    "providers": [{"name": "virtualbox", "url": server.url("/box.box")}]
                }
            ]
        });
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let cmd = BoxCommands::Update(BoxUpdateArgs {
            box_name: None,
            provider: Some("virtualbox".to_string()),
            architecture: None,
            force: false,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
        });
        let mut out = Vec::new();
        assert!(execute(&cmd, &mut out).is_ok());

        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("VAGRANT_HOME");
        }
    }
}

#[cfg(test)]
mod extra_tests3 {
    use super::*;
    use crate::cli::*;
    use httpmock::prelude::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_outdated_global_true_write_error() {
        let _guard = super::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let boxes_dir = dir.path().join("boxes");
        fs::create_dir_all(&boxes_dir).expect("operation should succeed");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        let v1_dir = box_path.join("1.0.0").join("virtualbox");
        fs::create_dir_all(&v1_dir).expect("operation should succeed");

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
            std::env::set_var(
                "VAGRANT_HOME",
                dir.path().to_str().expect("operation should succeed"),
            );
        }

        let metadata_json = serde_json::json!({
            "name": "test/box",
            "description_markdown": "test box",
            "short_description": "test box",
            "versions": [
                {
                    "version": "3.0.0",
                    "providers": [{"name": "virtualbox", "url": "some_url"}]
                }
            ]
        });
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let cmd = BoxCommands::Outdated(BoxOutdatedArgs {
            global: true,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
            force: false,
        });

        struct FailingWriter;
        impl Write for FailingWriter {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "err"))
            }
            #[coverage(off)]
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut fail_out = FailingWriter;
        assert!(execute(&cmd, &mut fail_out).is_err());

        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("VAGRANT_HOME");
        }
    }
}
#[cfg(test)]
mod extra_box_cmd_tests {
    use super::*;
    use crate::cli::commands::box_cmd::tests::ENV_LOCK;
    use httpmock::MockServer;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_box_outdated_with_outdated_box() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");

        let temp = tempdir().unwrap();
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        fs::create_dir_all(&boxes_dir).unwrap();

        // Create local box "hashicorp/bionic64" with version "1.0.0"
        let box_dir = boxes_dir.join("hashicorp-VAGRANTSLASH-bionic64");
        fs::create_dir_all(box_dir.join("1.0.0")).unwrap();

        unsafe { std::env::set_var("VAGRANT_HOME", &vagrant_home) };

        let server = MockServer::start();
        unsafe { std::env::set_var("VAGRANT_CLOUD_URL", server.base_url()) };

        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/boxes/hashicorp/bionic64");
            then.status(200).json_body(serde_json::json!({
                "description_markdown": "Test Box",
                "short_description": "Short",
                "name": "hashicorp/bionic64",
                "versions": [
                    {
                        "version": "1.1.0",
                        "providers": [
                            {
                                "name": "virtualbox",
                                "url": "http://example.com/box.box"
                            }
                        ]
                    }
                ]
            }));
        });

        let mut args = crate::cli::commands::box_cmd::tests::mock_outdated_args();
        args.global = true;
        let cmd = BoxCommands::Outdated(args);

        let mut out = Vec::new();
        let result = execute(&cmd, &mut out);

        let output_str = String::from_utf8(out).unwrap_or_default();
        println!("OUTPUT: {}", output_str);

        assert!(result.is_ok());
        assert!(output_str.contains("is outdated!"));

        mock.assert();
    }
}
#[cfg(test)]
mod extra_box_cmd_tests_2 {
    use super::*;
    use crate::cli::commands::box_cmd::tests::{ENV_LOCK, FailingWriter};
    use httpmock::MockServer;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_box_outdated_with_outdated_box_write_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");

        let temp = tempdir().unwrap();
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        fs::create_dir_all(&boxes_dir).unwrap();

        let box_dir = boxes_dir.join("hashicorp-VAGRANTSLASH-bionic64");
        fs::create_dir_all(box_dir.join("1.0.0")).unwrap();

        unsafe { std::env::set_var("VAGRANT_HOME", &vagrant_home) };

        let server = MockServer::start();
        unsafe { std::env::set_var("VAGRANT_CLOUD_URL", server.base_url()) };

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/boxes/hashicorp/bionic64");
            then.status(200).json_body(serde_json::json!({
                "description_markdown": "Test Box",
                "short_description": "Short",
                "name": "hashicorp/bionic64",
                "versions": [
                    {
                        "version": "1.1.0",
                        "providers": [
                            {
                                "name": "virtualbox",
                                "url": "http://example.com/box.box"
                            }
                        ]
                    }
                ]
            }));
        });

        let mut args = crate::cli::commands::box_cmd::tests::mock_outdated_args();
        args.global = true;
        let cmd = BoxCommands::Outdated(args);

        let mut out = FailingWriter;
        let result = execute(&cmd, &mut out);

        assert!(result.is_err());
    }
}

#[cfg(test)]
mod missing_coverage_tests {
    use super::tests::*;
    use super::*;
    use crate::cli::*;
    use httpmock::Method::GET;
    use httpmock::MockServer;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_add_download_box_fails() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
        }

        let metadata_json = serde_json::json!({
            "name": "test/box", "description_markdown": "Test", "short_description": "Test",
            "versions": [{ "version": "1.0.0", "providers": [
                { "name": "wrong_provider", "url": "http://invalid" },
                { "name": "virtualbox", "url": server.url("/fail_download.box") }
            ]}]
        });

        let metadata_mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let box_mock = server.mock(|when, then| {
            when.method(GET).path("/fail_download.box");
            then.status(404);
        });

        let mut args = mock_add_args();
        args.name = "test/box".to_string();
        args.provider = Some("virtualbox".to_string());

        let res = execute(&BoxCommands::Add(args), &mut Vec::new());
        assert!(res.is_err());
        metadata_mock.assert_calls(1);
        box_mock.assert_calls(1);
        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
        }
    }

    #[test]
    fn test_execute_update_add_box_fails_and_loop_miss() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let boxes_dir = dir.path().join("boxes");
        fs::create_dir_all(&boxes_dir).expect("operation should succeed");
        fs::create_dir_all(
            boxes_dir
                .join("test-VAGRANTSLASH-box")
                .join("1.0.0")
                .join("virtualbox"),
        )
        .unwrap();

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
            std::env::set_var("VAGRANT_HOME", dir.path());
        }

        let metadata_json = serde_json::json!({
            "name": "test/box", "description_markdown": "Test", "short_description": "Test",
            "versions": [{ "version": "2.0.0", "providers": [
                { "name": "wrong_provider", "url": "http://invalid" },
                { "name": "virtualbox", "url": server.url("/invalid.box") }
            ]}]
        });

        let metadata_mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let box_mock = server.mock(|when, then| {
            when.method(GET).path("/invalid.box");
            then.status(200).body("not a real box");
        });

        let mut args = mock_update_args();
        args.box_name = Some("test/box".to_string());
        args.provider = Some("virtualbox".to_string());

        let res = execute(&BoxCommands::Update(args), &mut Vec::new());
        assert!(res.is_err());
        metadata_mock.assert_calls(1);
        box_mock.assert_calls(1);
        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_execute_add_provider_not_found() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
        }

        let metadata_json = serde_json::json!({
            "name": "test/box", "description_markdown": "Test", "short_description": "Test",
            "versions": [{ "version": "1.0.0", "providers": [{ "name": "virtualbox", "url": "http://someurl" }] }]
        });

        let metadata_mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let mut args = mock_add_args();
        args.name = "test/box".to_string();
        args.provider = Some("vmware".to_string());
        let res = execute(&BoxCommands::Add(args), &mut Vec::new());
        assert!(res.is_err());
        metadata_mock.assert_calls(1);
        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
        }
    }

    #[test]
    fn test_execute_update_provider_not_found() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let boxes_dir = dir.path().join("boxes");
        fs::create_dir_all(&boxes_dir).expect("operation should succeed");
        fs::create_dir_all(
            boxes_dir
                .join("test-VAGRANTSLASH-box")
                .join("1.0.0")
                .join("virtualbox"),
        )
        .unwrap();

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
            std::env::set_var("VAGRANT_HOME", dir.path());
        }

        let metadata_json = serde_json::json!({
            "name": "test/box", "description_markdown": "Test", "short_description": "Test",
            "versions": [{ "version": "2.0.0", "providers": [{ "name": "virtualbox", "url": "http://someurl" }] }]
        });

        let metadata_mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let mut args = mock_update_args();
        args.box_name = Some("test/box".to_string());
        args.provider = Some("vmware".to_string());
        let res = execute(&BoxCommands::Update(args), &mut Vec::new());
        assert!(res.is_ok());
        metadata_mock.assert_calls(1);
        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_outdated_global_true_uptodate_write_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let dir = tempdir().expect("operation should succeed");
        let boxes_dir = dir.path().join("boxes");
        fs::create_dir_all(&boxes_dir).expect("operation should succeed");
        fs::create_dir_all(
            boxes_dir
                .join("test-VAGRANTSLASH-box")
                .join("1.0.0")
                .join("virtualbox"),
        )
        .unwrap();

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url(""));
            std::env::set_var("VAGRANT_HOME", dir.path());
        }

        let metadata_json = serde_json::json!({
            "name": "test/box", "description_markdown": "Test", "short_description": "Test",
            "versions": [{ "version": "1.0.0", "providers": [{"name": "virtualbox", "url": "some_url"}] }]
        });
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let mut fail_out = FailingWriter;
        assert!(
            execute(
                &BoxCommands::Outdated(BoxOutdatedArgs {
                    global: true,
                    insecure: false,
                    cacert: None,
                    capath: None,
                    cert: None,
                    force: false,
                }),
                &mut fail_out
            )
            .is_err()
        );

        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_execute_box_outdated_with_up_to_date_box() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");

        let temp = tempdir().unwrap();
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        fs::create_dir_all(&boxes_dir).unwrap();

        let box_dir = boxes_dir.join("hashicorp-VAGRANTSLASH-bionic64");
        fs::create_dir_all(box_dir.join("2.0.0")).unwrap();

        unsafe { std::env::set_var("VAGRANT_HOME", &vagrant_home) };

        let server = MockServer::start();
        unsafe { std::env::set_var("VAGRANT_CLOUD_URL", server.base_url()) };

        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/boxes/hashicorp/bionic64");
            then.status(200).json_body(serde_json::json!({
                "description_markdown": "Test Box",
                "short_description": "Test Box",
                "name": "hashicorp/bionic64",
                "versions": [
                    {
                        "version": "1.0.0",
                        "status": "active",
                        "description_html": "Test",
                        "description_markdown": "Test",
                        "providers": []
                    }
                ]
            }));
        });

        let mut out = Vec::new();
        let args = BoxOutdatedArgs {
            global: true,
            force: false,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
        };
        let cmd = BoxCommands::Outdated(args);

        assert!(execute(&cmd, &mut out).is_ok());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::remove_var("VAGRANT_CLOUD_URL");
        }

        mock.assert();
        let output_str = String::from_utf8(out).unwrap();
        assert!(output_str.contains("is up to date."));
    }
}

#[cfg(test)]
mod final_box_coverage_tests {
    use super::*;
    use crate::cli::commands::box_cmd::tests::{
        ENV_LOCK, mock_add_args, mock_outdated_args, mock_update_args,
    };
    use httpmock::prelude::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_client_new_errors() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let temp = tempdir().expect("operation should succeed");
        let boxes_dir = temp.path().join("boxes");
        fs::create_dir_all(&boxes_dir).expect("operation should succeed");
        unsafe {
            std::env::set_var("VAGRANT_HOME", temp.path());
            std::env::set_var("MIGRATORY_TEST_MOCK_CLIENT_ERROR", "1");
        }
        let mut out = Vec::new();
        assert!(execute(&BoxCommands::Add(mock_add_args()), &mut out).is_err());
        assert!(execute(&BoxCommands::Outdated(mock_outdated_args()), &mut out).is_err());
        assert!(execute(&BoxCommands::Update(mock_update_args()), &mut out).is_err());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_CLIENT_ERROR");
        }
    }

    #[test]
    fn test_box_add_version_loop_miss() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.base_url());
        }

        server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(serde_json::json!({
                "name": "test/box",
                "description_markdown": "",
                "short_description": "",
                "versions": [
                    {
                        "version": "1.0.0",
                        "providers": [{ "name": "virtualbox", "url": server.url("/box.box") }]
                    },
                    {
                        "version": "2.0.0",
                        "providers": [{ "name": "virtualbox", "url": server.url("/box.box") }]
                    }
                ]
            }));
        });
        server.mock(|when, then| {
            when.method(GET).path("/box.box");
            then.status(200).body(b"dummy");
        });

        let mut args = mock_add_args();
        args.name = "test/box".to_string();
        args.box_version = Some("2.0.0".to_string());
        args.provider = Some("virtualbox".to_string());

        let temp = tempdir().expect("operation should succeed");
        unsafe {
            std::env::set_var("VAGRANT_HOME", temp.path());
        }
        let mut out = Vec::new();
        let _ = execute(&BoxCommands::Add(args), &mut out);
    }

    #[test]
    fn test_outdated_version_comparisons_and_metadata_error() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let temp = tempdir().expect("operation should succeed");
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        let box_dir = boxes_dir.join("test-VAGRANTSLASH-box");
        fs::create_dir_all(box_dir.join("2.0.0")).expect("operation should succeed");
        fs::create_dir_all(box_dir.join("1.0.0")).expect("operation should succeed");
        unsafe {
            std::env::set_var("VAGRANT_HOME", &vagrant_home);
        }

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.base_url());
        }

        server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(serde_json::json!({
                "name": "test/box",
                "description_markdown": "",
                "short_description": "",
                "versions": [
                    { "version": "3.0.0", "providers": [] },
                    { "version": "2.5.0", "providers": [] }
                ]
            }));
        });

        let mut args = mock_outdated_args();
        args.global = true;
        let mut out = Vec::new();
        assert!(execute(&BoxCommands::Outdated(args.clone()), &mut out).is_ok());

        let server_err = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server_err.base_url());
        }
        server_err.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(404);
        });
        assert!(execute(&BoxCommands::Outdated(args), &mut out).is_ok());
    }

    #[test]
    fn test_update_version_comparisons_and_download_fail() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let temp = tempdir().expect("operation should succeed");
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        let box_dir = boxes_dir.join("test-VAGRANTSLASH-box");
        fs::create_dir_all(box_dir.join("2.0.0")).expect("operation should succeed");
        fs::create_dir_all(box_dir.join("1.0.0")).expect("operation should succeed");
        unsafe {
            std::env::set_var("VAGRANT_HOME", &vagrant_home);
        }

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.base_url());
        }

        server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(serde_json::json!({
                "name": "test/box",
                "description_markdown": "",
                "short_description": "",
                "versions": [
                    {
                        "version": "3.0.0",
                        "providers": [{ "name": "virtualbox", "url": server.url("/fail.box") }]
                    },
                    {
                        "version": "2.5.0",
                        "providers": [{ "name": "virtualbox", "url": server.url("/fail.box") }]
                    }
                ]
            }));
        });
        server.mock(|when, then| {
            when.method(GET).path("/fail.box");
            then.status(404);
        });

        let args = mock_update_args();
        let mut out = Vec::new();
        assert!(execute(&BoxCommands::Update(args.clone()), &mut out).is_err());

        let server_err = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server_err.base_url());
        }
        server_err.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(404);
        });
        assert!(execute(&BoxCommands::Update(args), &mut out).is_ok());
    }

    #[test]
    fn test_writeln_failures_in_outdated_update_repackage() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let temp = tempdir().expect("operation should succeed");
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        fs::create_dir_all(&boxes_dir).expect("operation should succeed");
        unsafe {
            std::env::set_var("VAGRANT_HOME", &vagrant_home);
        }

        let mut out = tests::FailingWriter;
        let mut args = mock_outdated_args();
        args.global = false;
        assert!(execute(&BoxCommands::Outdated(args), &mut out).is_err());

        let box_dir = boxes_dir.join("test-VAGRANTSLASH-box");
        fs::create_dir_all(box_dir.join("1.0.0")).expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.base_url());
        }
        server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(404);
        });
        let mut out2 = tests::FailingWriter;
        let mut args2 = mock_outdated_args();
        args2.global = true;
        assert!(execute(&BoxCommands::Outdated(args2), &mut out2).is_err());

        let mut out3 = tests::FailingWriter;
        assert!(execute(&BoxCommands::Update(mock_update_args()), &mut out3).is_err());

        let temp_rep = tempdir().expect("operation should succeed");
        super::extra_tests::setup_fake_boxes(temp_rep.path());
        unsafe {
            std::env::set_var("VAGRANT_HOME", temp_rep.path());
        }
        let mut out4 = tests::FailingWriter;
        let rep_args = crate::cli::BoxRepackageArgs {
            name: "test/box".to_string(),
            provider: "virtualbox".to_string(),
            version: "1.0.0".to_string(),
        };
        assert!(execute(&BoxCommands::Repackage(rep_args), &mut out4).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_permission_denied_branches() {
        use std::os::unix::fs::PermissionsExt;
        let _guard = ENV_LOCK.lock().expect("operation should succeed");
        let temp = tempdir().expect("operation should succeed");
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        let box_dir = boxes_dir.join("test-VAGRANTSLASH-box");
        let ver_dir = box_dir.join("1.0.0");
        let prov_dir = ver_dir.join("virtualbox");
        fs::create_dir_all(&prov_dir).expect("operation should succeed");
        unsafe {
            std::env::set_var("VAGRANT_HOME", &vagrant_home);
        }

        fs::set_permissions(&box_dir, fs::Permissions::from_mode(0o444))
            .expect("operation should succeed");
        let mut out = Vec::new();
        let remove_args = crate::cli::BoxRemoveArgs {
            name: "test/box".to_string(),
            force: true,
            ..Default::default()
        };
        let res_remove = execute(&BoxCommands::Remove(remove_args), &mut out);
        fs::set_permissions(&box_dir, fs::Permissions::from_mode(0o755))
            .expect("operation should succeed");
        assert!(res_remove.is_err());

        fs::set_permissions(&ver_dir, fs::Permissions::from_mode(0o000))
            .expect("operation should succeed");
        let mut out = Vec::new();
        let _ = execute(&BoxCommands::List(Default::default()), &mut out);
        let _ = execute(&BoxCommands::Outdated(mock_outdated_args()), &mut out);
        let _ = execute(&BoxCommands::Update(mock_update_args()), &mut out);
        fs::set_permissions(&ver_dir, fs::Permissions::from_mode(0o755))
            .expect("operation should succeed");
    }

    #[test]
    fn test_execute_box_outdated_with_empty_remote_versions() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");

        let temp = tempdir().expect("operation should succeed");
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        fs::create_dir_all(&boxes_dir).expect("operation should succeed");

        let box_dir = boxes_dir.join("hashicorp-VAGRANTSLASH-bionic64");
        fs::create_dir_all(box_dir.join("1.0.0")).expect("operation should succeed");

        unsafe { std::env::set_var("VAGRANT_HOME", &vagrant_home) };

        let server = MockServer::start();
        unsafe { std::env::set_var("VAGRANT_CLOUD_URL", server.base_url()) };

        let _mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/hashicorp/bionic64");
            then.status(200).json_body(serde_json::json!({
                "description_markdown": "Test Box",
                "short_description": "Short",
                "name": "hashicorp/bionic64",
                "versions": []
            }));
        });

        let mut args = mock_outdated_args();
        args.global = true;
        let cmd = BoxCommands::Outdated(args);

        let mut out = Vec::new();
        let result = execute(&cmd, &mut out);
        assert!(result.is_ok());
        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("is up to date"));
    }

    #[test]
    fn test_execute_box_add_local_box_file_suffix() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");

        let temp = tempdir().expect("operation should succeed");
        let vagrant_home = temp.path().join(".vagrant.d");
        fs::create_dir_all(&vagrant_home).expect("operation should succeed");

        let box_file = temp.path().join("local_package.box");
        fs::write(&box_file, "dummy content").expect("operation should succeed");

        unsafe { std::env::set_var("VAGRANT_HOME", &vagrant_home) };

        let mut args = mock_add_args();
        args.name = box_file.to_string_lossy().to_string();
        let cmd = BoxCommands::Add(args);

        let mut out = Vec::new();
        let _ = execute(&cmd, &mut out);
    }

    #[test]
    fn test_execute_box_add_http_url() {
        let _guard = ENV_LOCK.lock().expect("operation should succeed");

        let temp = tempdir().expect("operation should succeed");
        let vagrant_home = temp.path().join(".vagrant.d");
        fs::create_dir_all(&vagrant_home).expect("operation should succeed");
        unsafe { std::env::set_var("VAGRANT_HOME", &vagrant_home) };

        let mut args = mock_add_args();
        args.name = "http://127.0.0.1:9/box.box".to_string();
        let cmd = BoxCommands::Add(args);

        let mut out = Vec::new();
        let _ = execute(&cmd, &mut out);
    }
}
