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
    let home_dir = std::env::var("VAGRANT_HOME").unwrap_or_else(|_| ".vagrant.d".to_string());
    let global_dir = std::path::Path::new(&home_dir);
    execute_with_config(cmd, &mut writer, global_dir, CloudClient::new)
}

/// Executes the `box` command with an explicit global state directory and CloudClient factory.
///
/// # Arguments
///
/// * `cmd` - The box subcommand to execute.
/// * `writer` - A mutable reference to a trait object writer.
/// * `global_dir` - Base directory for Vagrant global state.
/// * `cloud_factory` - Factory function creating a [`CloudClient`].
///
/// # Returns
///
/// Returns `Ok(())` on successful execution, or a `MigratoryError` on failure.
///
/// # Errors
///
/// Returns a `MigratoryError` if writing to the output stream or executing the box operation fails.
pub fn execute_with_config<C>(
    cmd: &BoxCommands,
    writer: &mut dyn Write,
    global_dir: &std::path::Path,
    mut cloud_factory: C,
) -> Result<(), MigratoryError>
where
    C: FnMut() -> Result<CloudClient, MigratoryError>,
{
    match cmd {
        BoxCommands::Add(args) => {
            let ui = ConsoleUi;
            ui.info("box", &format!("Adding box '{}'...", args.name));

            // Check if it's a URL or path, if not, treat as HashiCorp Cloud tag
            let is_url_or_path = args.name.contains("://")
                || std::path::Path::new(&args.name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("box"));

            let box_mgr = BoxManager::new(global_dir);

            let mut download_url = args.name.clone();
            let mut target_version = args.box_version.clone().unwrap_or_else(|| "0".to_string());
            let mut target_provider = args
                .provider
                .clone()
                .unwrap_or_else(|| "virtualbox".to_string());

            if !is_url_or_path {
                ui.detail("box", "Fetching metadata from Vagrant Cloud...");
                let cloud_client = cloud_factory()?;
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
            let safe_name = args.name.replace('/', "-VAGRANTSLASH-");
            let box_dir = manager.global_boxes_dir.join(safe_name);

            if !box_dir.exists() {
                return Err(MigratoryError::NotFound(format!("Box '{}'", args.name)));
            }

            if args.all || (args.provider.is_none() && args.box_version.is_none()) {
                std::fs::remove_dir_all(&box_dir)?;
                writeln!(writer, "Box '{}' removed.", args.name)?;
            } else {
                let mut removed_any = false;
                for ver_entry in std::fs::read_dir(&box_dir).into_iter().flatten().flatten() {
                    let version_name = ver_entry.file_name().to_string_lossy().to_string();
                    if let Some(target_ver) = &args.box_version
                        && &version_name != target_ver
                    {
                        continue;
                    }
                    if let Some(target_prov) = &args.provider {
                        let prov_dir = ver_entry.path().join(target_prov);
                        if prov_dir.exists() {
                            let _ = std::fs::remove_dir_all(&prov_dir);
                            removed_any = true;
                        }
                    } else {
                        let _ = std::fs::remove_dir_all(ver_entry.path());
                        removed_any = true;
                    }

                    if ver_entry.path().exists()
                        && std::fs::read_dir(ver_entry.path())
                            .into_iter()
                            .flatten()
                            .next()
                            .is_none()
                    {
                        let _ = std::fs::remove_dir(ver_entry.path());
                    }
                }

                if std::fs::read_dir(&box_dir)
                    .into_iter()
                    .flatten()
                    .next()
                    .is_none()
                {
                    let _ = std::fs::remove_dir(&box_dir);
                }

                if !removed_any {
                    return Err(MigratoryError::NotFound(format!(
                        "Box '{}' with matching provider/version",
                        args.name
                    )));
                }

                writeln!(writer, "Box '{}' removed.", args.name)?;
            }
        }
        BoxCommands::Outdated(args) => {
            let manager = BoxManager::new(global_dir);
            let ui = ConsoleUi;

            if !manager.global_boxes_dir.exists() {
                writeln!(writer, "There are no installed boxes to check.")?;
                return Ok(());
            }

            let cloud_client = cloud_factory()?;
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
                                "Box '{box_name}' is outdated! Local: {max_local_version}, Remote: {max_remote_version}"
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

            let cloud_client = cloud_factory()?;
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

/// Test helpers and synchronization locks for box commands.
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::cli::*;

    /// Writer implementation that unconditionally returns an I/O error on write.
    pub(crate) struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("write failed"))
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

    /// Creates default `BoxAddArgs` for tests.
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

    /// Creates default `BoxListArgs` for tests.
    pub(crate) fn mock_list_args() -> BoxListArgs {
        BoxListArgs { box_info: false }
    }

    /// Creates default `BoxRemoveArgs` for tests.
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

    /// Creates default `BoxOutdatedArgs` for tests.
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

    /// Creates default `BoxUpdateArgs` for tests.
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

    /// Creates default `BoxPruneArgs` for tests.
    pub(crate) fn mock_prune_args() -> BoxPruneArgs {
        BoxPruneArgs {
            provider: None,
            dry_run: false,
            keep_active_boxes: true,
            name: None,
            force: false,
        }
    }

    /// Creates default `BoxRepackageArgs` for tests.
    pub(crate) fn mock_repackage_args() -> BoxRepackageArgs {
        BoxRepackageArgs {
            name: "test/box".to_string(),
            provider: "virtualbox".to_string(),
            version: "1.0.0".to_string(),
        }
    }

    #[test]
    fn test_execute_box_add() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;

        let _cmd = BoxCommands::Add(mock_add_args());
        let mut out = Vec::new();
        let mut args = mock_add_args();
        let box_path = dir.path().join("dummy.box");
        std::fs::write(&box_path, b"dummy content")?;

        args.name = box_path.to_string_lossy().to_string();
        let cmd = BoxCommands::Add(args);

        let result = execute_with_config(&cmd, &mut out, dir.path(), CloudClient::new);
        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_execute_box_add_write_error() -> Result<(), MigratoryError> {
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

        let temp = tempfile::tempdir()?;
        std::fs::create_dir_all(temp.path().join("boxes"))?;
        std::fs::create_dir_all(temp.path().join("tmp"))?;

        let mut args = mock_add_args();
        args.name = "test/write_error".to_string();
        let cmd = BoxCommands::Add(args);
        let mut out = FailingWriter;
        let url = server.url("");
        let result = execute_with_config(&cmd, &mut out, temp.path(), || {
            CloudClient::with_config(crate::cloud::CloudClientConfig {
                base_url: url.clone(),
                token: None,
            })
        });
        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_execute_box_list() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let cmd = BoxCommands::List(mock_list_args());
        let mut out = Vec::new();
        assert!(execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("There are no installed boxes"));
        Ok(())
    }

    #[test]
    fn test_execute_box_list_write_error() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let cmd = BoxCommands::List(mock_list_args());
        let mut out = FailingWriter;
        let result = execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_box_remove() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let cmd = BoxCommands::Remove(mock_remove_args());
        let mut out = Vec::new();
        let result = execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_box_remove_write_error() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let box_dir = temp.path().join("boxes").join("test-VAGRANTSLASH-box");
        std::fs::create_dir_all(&box_dir)?;

        let cmd = BoxCommands::Remove(mock_remove_args());
        let mut out = FailingWriter;
        let result = execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new);
        assert!(result.is_err());

        let box_ver_dir = temp
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box2")
            .join("1.0.0");
        std::fs::create_dir_all(box_ver_dir.join("virtualbox"))?;
        let cmd2 = BoxCommands::Remove(BoxRemoveArgs {
            name: "test/box2".to_string(),
            provider: Some("virtualbox".to_string()),
            box_version: None,
            all: false,
            force: true,
            architecture: None,
            all_providers: false,
            all_architectures: false,
        });
        let mut out2 = FailingWriter;
        let result2 = execute_with_config(&cmd2, &mut out2, temp.path(), CloudClient::new);
        assert!(result2.is_err());

        Ok(())
    }

    #[test]
    fn test_execute_box_remove_by_provider_and_all() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;

        let box_dir = temp
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0");
        std::fs::create_dir_all(box_dir.join("virtualbox"))?;
        std::fs::create_dir_all(box_dir.join("qemu"))?;

        // 1. Remove non-existent provider returns error
        let cmd_nonexistent_prov = BoxCommands::Remove(BoxRemoveArgs {
            name: "test/box".to_string(),
            provider: Some("hyperv".to_string()),
            box_version: None,
            all: false,
            force: true,
            architecture: None,
            all_providers: false,
            all_architectures: false,
        });
        let mut out1 = Vec::new();
        assert!(
            execute_with_config(
                &cmd_nonexistent_prov,
                &mut out1,
                temp.path(),
                CloudClient::new
            )
            .is_err()
        );

        // 2. Remove with version mismatch
        let cmd_version_mismatch = BoxCommands::Remove(BoxRemoveArgs {
            name: "test/box".to_string(),
            provider: Some("virtualbox".to_string()),
            box_version: Some("9.9.9".to_string()),
            all: false,
            force: true,
            architecture: None,
            all_providers: false,
            all_architectures: false,
        });
        let mut out2 = Vec::new();
        assert!(
            execute_with_config(
                &cmd_version_mismatch,
                &mut out2,
                temp.path(),
                CloudClient::new
            )
            .is_err()
        );

        // 3. Remove specific provider virtualbox succeeds
        let cmd_remove_vbox = BoxCommands::Remove(BoxRemoveArgs {
            name: "test/box".to_string(),
            provider: Some("virtualbox".to_string()),
            box_version: Some("1.0.0".to_string()),
            all: false,
            force: true,
            architecture: None,
            all_providers: false,
            all_architectures: false,
        });
        let mut out3 = Vec::new();
        assert!(
            execute_with_config(&cmd_remove_vbox, &mut out3, temp.path(), CloudClient::new).is_ok()
        );
        assert!(!box_dir.join("virtualbox").exists());
        assert!(box_dir.join("qemu").exists());

        // 3b. Remove remaining provider qemu, emptying version dir and box dir
        let cmd_remove_qemu = BoxCommands::Remove(BoxRemoveArgs {
            name: "test/box".to_string(),
            provider: Some("qemu".to_string()),
            box_version: Some("1.0.0".to_string()),
            all: false,
            force: true,
            architecture: None,
            all_providers: false,
            all_architectures: false,
        });
        let mut out3b = Vec::new();
        assert!(
            execute_with_config(&cmd_remove_qemu, &mut out3b, temp.path(), CloudClient::new)
                .is_ok()
        );
        assert!(
            !temp
                .path()
                .join("boxes")
                .join("test-VAGRANTSLASH-box")
                .exists()
        );

        // 3c. Recreate with version 2.0.0 and test remove by box_version without provider
        let box_dir_v2 = temp
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("2.0.0");
        std::fs::create_dir_all(box_dir_v2.join("hyperv"))?;
        let cmd_remove_version_only = BoxCommands::Remove(BoxRemoveArgs {
            name: "test/box".to_string(),
            provider: None,
            box_version: Some("2.0.0".to_string()),
            all: false,
            force: true,
            architecture: None,
            all_providers: false,
            all_architectures: false,
        });
        let mut out3c = Vec::new();
        assert!(
            execute_with_config(
                &cmd_remove_version_only,
                &mut out3c,
                temp.path(),
                CloudClient::new
            )
            .is_ok()
        );
        assert!(
            !temp
                .path()
                .join("boxes")
                .join("test-VAGRANTSLASH-box")
                .exists()
        );

        // 4. Remove all remaining providers and versions
        let box_dir_v3 = temp
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("3.0.0");
        std::fs::create_dir_all(box_dir_v3.join("utm"))?;
        let cmd_remove_all = BoxCommands::Remove(BoxRemoveArgs {
            name: "test/box".to_string(),
            provider: None,
            box_version: None,
            all: true,
            force: true,
            architecture: None,
            all_providers: false,
            all_architectures: false,
        });
        let mut out4 = Vec::new();
        assert!(
            execute_with_config(&cmd_remove_all, &mut out4, temp.path(), CloudClient::new).is_ok()
        );
        assert!(
            !temp
                .path()
                .join("boxes")
                .join("test-VAGRANTSLASH-box")
                .exists()
        );

        Ok(())
    }

    #[test]
    fn test_execute_box_outdated() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let cmd = BoxCommands::Outdated(mock_outdated_args());
        let mut out = Vec::new();
        assert!(execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("There are no installed boxes to check."));
        Ok(())
    }

    #[test]
    fn test_execute_box_outdated_write_error() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let cmd = BoxCommands::Outdated(mock_outdated_args());
        let mut out = FailingWriter;
        let result = execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_box_update() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let cmd = BoxCommands::Update(mock_update_args());
        let mut out = Vec::new();
        assert!(execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("There are no installed boxes to update"));
        Ok(())
    }

    #[test]
    fn test_execute_box_update_write_error() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let cmd = BoxCommands::Update(mock_update_args());
        let mut out = FailingWriter;
        let result = execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_box_prune() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let cmd = BoxCommands::Prune(mock_prune_args());
        let mut out = Vec::new();
        assert!(execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Box pruned successfully.");
        Ok(())
    }

    #[test]
    fn test_execute_box_prune_write_error() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let cmd = BoxCommands::Prune(mock_prune_args());
        let mut out = FailingWriter;
        let result = execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_box_repackage() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let cmd = BoxCommands::Repackage(mock_repackage_args());
        let mut out = Vec::new();
        assert!(execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new).is_err());
        Ok(())
    }

    #[test]
    fn test_execute_box_repackage_write_error() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let cmd = BoxCommands::Repackage(mock_repackage_args());
        let mut out = FailingWriter;
        let result = execute_with_config(&cmd, &mut out, temp.path(), CloudClient::new);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_box_add_cloud_failure() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let mut args = mock_add_args();
        args.name = "invalid/tag/for/cloud".to_string(); // not ending with .box or ://
        let cmd = BoxCommands::Add(args);
        let mut out = Vec::new();
        let result = execute_with_config(&cmd, &mut out, temp.path(), || {
            CloudClient::with_config(crate::cloud::CloudClientConfig {
                base_url: "http://127.0.0.1:1".to_string(),
                token: None,
            })
        });
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_box_prune_error() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join(".vagrant.d");
        std::fs::create_dir_all(&file_path)?;
        let boxes_file = file_path.join("boxes");
        std::fs::write(&boxes_file, "not a dir")?;

        let cmd = BoxCommands::Prune(mock_prune_args());
        let mut out = Vec::new();
        let result = execute_with_config(&cmd, &mut out, &file_path, CloudClient::new);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_box_repackage_error() {
        let mut args = mock_repackage_args();
        args.name = String::new(); // Will trigger error in repackage
        let cmd = BoxCommands::Repackage(args);
        let mut out = Vec::new();
        let result = execute(&cmd, &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_box_add_cloud_paths() -> Result<(), MigratoryError> {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use httpmock::prelude::*;

        let server = MockServer::start();

        let mut tar_gz = Vec::new();
        {
            let enc = GzEncoder::new(&mut tar_gz, Compression::default());
            let mut tar = tar::Builder::new(enc);
            tar.finish()?;
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

        let temp = tempfile::tempdir()?;

        let mut args_success = mock_add_args();
        args_success.name = "test/box".to_string();
        args_success.clean = true;
        let cmd_success = BoxCommands::Add(args_success);

        let mut out = Vec::new();
        let url = server.url("");
        let result_success = execute_with_config(&cmd_success, &mut out, temp.path(), || {
            CloudClient::with_config(crate::cloud::CloudClientConfig {
                base_url: url.clone(),
                token: None,
            })
        });
        assert!(result_success.is_ok(), "{:?}", result_success);
        metadata_mock.assert_calls(1);
        box_mock.assert_calls(1);

        Ok(())
    }
}

#[cfg(test)]
mod extra_tests {
    use super::*;
    use crate::cli::*;
    use httpmock::prelude::*;
    use std::fs;
    use tempfile::tempdir;

    /// Populates a temporary directory with fake box versions (`1.0.0` and `2.0.0`).
    pub(crate) fn setup_fake_boxes(dir: &std::path::Path) -> Result<(), MigratoryError> {
        let boxes_dir = dir.join("boxes");
        fs::create_dir_all(&boxes_dir)?;
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        let v1_dir = box_path.join("1.0.0").join("virtualbox");
        let v2_dir = box_path.join("2.0.0").join("virtualbox");
        fs::create_dir_all(&v1_dir)?;
        fs::create_dir_all(&v2_dir)?;
        Ok(())
    }

    #[test]
    fn test_list_with_boxes() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        setup_fake_boxes(dir.path())?;

        let cmd = BoxCommands::List(BoxListArgs { box_info: false });
        let mut out = Vec::new();
        assert!(execute_with_config(&cmd, &mut out, dir.path(), CloudClient::new).is_ok());
        let output = String::from_utf8(out)?;
        assert!(output.contains("test/box (virtualbox, 1.0.0)"));
        assert!(output.contains("test/box (virtualbox, 2.0.0)"));

        Ok(())
    }

    #[test]
    fn test_list_write_error_with_boxes() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        setup_fake_boxes(dir.path())?;

        let cmd = BoxCommands::List(BoxListArgs { box_info: false });
        let mut out = super::tests::FailingWriter;
        assert!(execute_with_config(&cmd, &mut out, dir.path(), CloudClient::new).is_err());

        Ok(())
    }

    #[test]
    fn test_remove_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        setup_fake_boxes(dir.path())?;

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
        assert!(execute_with_config(&cmd, &mut out, dir.path(), CloudClient::new).is_ok());

        Ok(())
    }

    #[test]
    fn test_remove_write_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        setup_fake_boxes(dir.path())?;

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
        let mut out = super::tests::FailingWriter;
        assert!(execute_with_config(&cmd, &mut out, dir.path(), CloudClient::new).is_err());

        Ok(())
    }

    #[test]
    fn test_outdated_global_false() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        setup_fake_boxes(dir.path())?;

        let cmd = BoxCommands::Outdated(BoxOutdatedArgs {
            global: false,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
            force: false,
        });
        let mut out = Vec::new();
        assert!(execute_with_config(&cmd, &mut out, dir.path(), CloudClient::new).is_ok());
        let output = String::from_utf8(out)?;
        assert!(output.contains("Box is outdated: false"));

        Ok(())
    }

    #[test]
    fn test_outdated_global_true_with_updates() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        setup_fake_boxes(dir.path())?;

        let server = MockServer::start();
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
        let url = server.url("");
        assert!(
            execute_with_config(&cmd, &mut out, dir.path(), || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            })
            .is_ok()
        );
        let output = String::from_utf8(out)?;
        assert!(output.contains("Box 'test/box' is outdated! Local: 2.0.0, Remote: 3.0.0"));
        assert!(output.contains("Box is outdated: true"));

        Ok(())
    }

    #[test]
    fn test_outdated_global_true_no_updates() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        setup_fake_boxes(dir.path())?;

        let server = MockServer::start();
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
        let url = server.url("");
        assert!(
            execute_with_config(&cmd, &mut out, dir.path(), || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            })
            .is_ok()
        );
        let output = String::from_utf8(out)?;
        assert!(output.contains("Box 'test/box' is up to date."));

        Ok(())
    }

    #[test]
    fn test_update_with_updates() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        setup_fake_boxes(dir.path())?;

        let server = MockServer::start();

        let mut tar_gz = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tar_gz, flate2::Compression::default());
            let mut tar = tar::Builder::new(enc);
            tar.finish()?;
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
        let url = server.url("");
        assert!(
            execute_with_config(&cmd, &mut out, dir.path(), || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            })
            .is_ok()
        );

        // Check if v3.0.0 was downloaded
        let v3_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("3.0.0")
            .join("virtualbox");
        assert!(v3_dir.exists());

        Ok(())
    }

    #[test]
    fn test_update_no_updates() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        setup_fake_boxes(dir.path())?;

        let server = MockServer::start();

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
        let url = server.url("");
        assert!(
            execute_with_config(&cmd, &mut out, dir.path(), || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            })
            .is_ok()
        );
        let output = String::from_utf8(out)?;
        assert!(output.contains("No boxes were updated."));

        Ok(())
    }

    #[test]
    fn test_repackage_success_cmd() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        setup_fake_boxes(dir.path())?;

        let cmd = BoxCommands::Repackage(BoxRepackageArgs {
            name: "test/box".to_string(),
            provider: "virtualbox".to_string(),
            version: "2.0.0".to_string(),
        });
        let mut out = Vec::new();
        assert!(execute_with_config(&cmd, &mut out, dir.path(), CloudClient::new).is_ok());

        let package_name = "package-test-VAGRANTSLASH-box-2.0.0-virtualbox.box";
        let dest_file = std::env::current_dir()
            .unwrap_or(std::path::PathBuf::from("."))
            .join(package_name);
        assert!(dest_file.exists());
        let _ = fs::remove_file(dest_file);

        Ok(())
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
    fn test_add_with_version_and_provider_match() -> Result<(), MigratoryError> {
        let server = MockServer::start();

        let mut tar_gz = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tar_gz, flate2::Compression::default());
            let mut tar = tar::Builder::new(enc);
            tar.finish()?;
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

        let temp = tempdir()?;

        let mut args = super::tests::mock_add_args();
        args.name = "test/box".to_string();
        args.box_version = Some("1.0.0".to_string());
        args.provider = Some("virtualbox".to_string());
        let cmd = BoxCommands::Add(args);

        let mut out = Vec::new();
        let url = server.url("");
        assert!(
            execute_with_config(&cmd, &mut out, temp.path(), || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            })
            .is_ok()
        );

        Ok(())
    }

    #[test]
    fn test_list_empty_boxes_dir() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let boxes_dir = dir.path().join("boxes");
        fs::create_dir_all(&boxes_dir)?; // Exists but is empty!

        let cmd = BoxCommands::List(BoxListArgs { box_info: false });
        let mut out = Vec::new();
        assert!(execute_with_config(&cmd, &mut out, dir.path(), CloudClient::new).is_ok());

        let mut fail_out = super::tests::FailingWriter;
        assert!(execute_with_config(&cmd, &mut fail_out, dir.path(), CloudClient::new).is_err());

        Ok(())
    }

    #[test]
    fn test_outdated_empty_version_dir() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let boxes_dir = dir.path().join("boxes");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        fs::create_dir_all(&box_path)?; // Box dir exists but has no versions

        let cmd = BoxCommands::Outdated(BoxOutdatedArgs {
            global: true,
            insecure: false,
            cacert: None,
            capath: None,
            cert: None,
            force: false,
        });
        let mut out = Vec::new();
        assert!(execute_with_config(&cmd, &mut out, dir.path(), CloudClient::new).is_ok());

        Ok(())
    }

    #[test]
    fn test_outdated_write_error_with_updates() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let boxes_dir = dir.path().join("boxes");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        let v1_dir = box_path.join("1.0.0").join("virtualbox");
        fs::create_dir_all(&v1_dir)?;

        let server = MockServer::start();

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
        let mut fail_out = super::tests::FailingWriter;
        let url = server.url("");
        assert!(
            execute_with_config(&cmd, &mut fail_out, dir.path(), || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            })
            .is_err()
        );

        Ok(())
    }

    #[test]
    fn test_update_target_box_mismatch() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let boxes_dir = dir.path().join("boxes");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        let v1_dir = box_path.join("1.0.0").join("virtualbox");
        fs::create_dir_all(&v1_dir)?;

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
        assert!(execute_with_config(&cmd, &mut out, dir.path(), CloudClient::new).is_ok());

        Ok(())
    }

    #[test]
    fn test_update_empty_version_dir() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let boxes_dir = dir.path().join("boxes");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        fs::create_dir_all(&box_path)?; // Box dir exists but has no versions

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
        assert!(execute_with_config(&cmd, &mut out, dir.path(), CloudClient::new).is_ok());

        Ok(())
    }

    #[test]
    fn test_update_with_provider_filter() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let boxes_dir = dir.path().join("boxes");
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        let v1_dir = box_path.join("1.0.0").join("virtualbox");
        fs::create_dir_all(&v1_dir)?;

        let server = MockServer::start();

        let mut tar_gz = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tar_gz, flate2::Compression::default());
            let mut tar = tar::Builder::new(enc);
            tar.finish()?;
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
        let url = server.url("");
        assert!(
            execute_with_config(&cmd, &mut out, dir.path(), || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            })
            .is_ok()
        );

        Ok(())
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
    fn test_outdated_global_true_write_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let boxes_dir = dir.path().join("boxes");
        fs::create_dir_all(&boxes_dir)?;
        let box_path = boxes_dir.join("test-VAGRANTSLASH-box");
        let v1_dir = box_path.join("1.0.0").join("virtualbox");
        fs::create_dir_all(&v1_dir)?;

        let server = MockServer::start();

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

        let mut fail_out = super::tests::FailingWriter;
        let url = server.url("");
        assert!(
            execute_with_config(&cmd, &mut fail_out, dir.path(), || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            })
            .is_err()
        );

        Ok(())
    }
}

#[cfg(test)]
mod extra_box_cmd_tests {
    use super::*;
    use httpmock::MockServer;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_box_outdated_with_outdated_box() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        fs::create_dir_all(&boxes_dir)?;

        // Create local box "hashicorp/bionic64" with version "1.0.0"
        let box_dir = boxes_dir.join("hashicorp-VAGRANTSLASH-bionic64");
        fs::create_dir_all(box_dir.join("1.0.0"))?;

        let server = MockServer::start();

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
        let url = server.base_url();
        let result = execute_with_config(&cmd, &mut out, &vagrant_home, || {
            CloudClient::with_config(crate::cloud::CloudClientConfig {
                base_url: url.clone(),
                token: None,
            })
        });

        let output_str = String::from_utf8(out).unwrap_or_default();

        assert!(result.is_ok());
        assert!(output_str.contains("is outdated!"));

        mock.assert();
        Ok(())
    }
}

#[cfg(test)]
mod extra_box_cmd_tests_2 {
    use super::*;
    use crate::cli::commands::box_cmd::tests::FailingWriter;
    use httpmock::MockServer;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_box_outdated_with_outdated_box_write_error() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        fs::create_dir_all(&boxes_dir)?;

        let box_dir = boxes_dir.join("hashicorp-VAGRANTSLASH-bionic64");
        fs::create_dir_all(box_dir.join("1.0.0"))?;

        let server = MockServer::start();

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
        let url = server.base_url();
        let result = execute_with_config(&cmd, &mut out, &vagrant_home, || {
            CloudClient::with_config(crate::cloud::CloudClientConfig {
                base_url: url.clone(),
                token: None,
            })
        });

        assert!(result.is_err());
        Ok(())
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
    fn test_execute_add_download_box_fails() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let server = MockServer::start();

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

        let url = server.url("");
        let res = execute_with_config(
            &BoxCommands::Add(args),
            &mut Vec::new(),
            temp.path(),
            || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            },
        );
        assert!(res.is_err());
        metadata_mock.assert_calls(1);
        box_mock.assert_calls(1);
        Ok(())
    }

    #[test]
    fn test_execute_update_add_box_fails_and_loop_miss() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let boxes_dir = dir.path().join("boxes");
        fs::create_dir_all(&boxes_dir)?;
        fs::create_dir_all(
            boxes_dir
                .join("test-VAGRANTSLASH-box")
                .join("1.0.0")
                .join("virtualbox"),
        )?;

        let server = MockServer::start();

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

        let url = server.url("");
        let res = execute_with_config(
            &BoxCommands::Update(args),
            &mut Vec::new(),
            dir.path(),
            || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            },
        );
        assert!(res.is_err());
        metadata_mock.assert_calls(1);
        box_mock.assert_calls(1);
        Ok(())
    }

    #[test]
    fn test_execute_add_provider_not_found() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let server = MockServer::start();

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
        let url = server.url("");
        let res = execute_with_config(
            &BoxCommands::Add(args),
            &mut Vec::new(),
            temp.path(),
            || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            },
        );
        assert!(res.is_err());
        metadata_mock.assert_calls(1);
        Ok(())
    }

    #[test]
    fn test_execute_update_provider_not_found() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let boxes_dir = dir.path().join("boxes");
        fs::create_dir_all(&boxes_dir)?;
        fs::create_dir_all(
            boxes_dir
                .join("test-VAGRANTSLASH-box")
                .join("1.0.0")
                .join("virtualbox"),
        )?;

        let server = MockServer::start();

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
        let url = server.url("");
        let res = execute_with_config(
            &BoxCommands::Update(args),
            &mut Vec::new(),
            dir.path(),
            || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            },
        );
        assert!(res.is_ok());
        metadata_mock.assert_calls(1);
        Ok(())
    }

    #[test]
    fn test_outdated_global_true_uptodate_write_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let boxes_dir = dir.path().join("boxes");
        fs::create_dir_all(&boxes_dir)?;
        fs::create_dir_all(
            boxes_dir
                .join("test-VAGRANTSLASH-box")
                .join("1.0.0")
                .join("virtualbox"),
        )?;

        let server = MockServer::start();

        let metadata_json = serde_json::json!({
            "name": "test/box", "description_markdown": "Test", "short_description": "Test",
            "versions": [{ "version": "1.0.0", "providers": [{"name": "virtualbox", "url": "some_url"}] }]
        });
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(metadata_json);
        });

        let mut fail_out = FailingWriter;
        let url = server.url("");
        assert!(
            execute_with_config(
                &BoxCommands::Outdated(BoxOutdatedArgs {
                    global: true,
                    insecure: false,
                    cacert: None,
                    capath: None,
                    cert: None,
                    force: false,
                }),
                &mut fail_out,
                dir.path(),
                || CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            )
            .is_err()
        );

        Ok(())
    }

    #[test]
    fn test_execute_box_outdated_with_up_to_date_box() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        fs::create_dir_all(&boxes_dir)?;

        let box_dir = boxes_dir.join("hashicorp-VAGRANTSLASH-bionic64");
        fs::create_dir_all(box_dir.join("2.0.0"))?;

        let server = MockServer::start();

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

        let url = server.base_url();
        assert!(
            execute_with_config(&cmd, &mut out, &vagrant_home, || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            })
            .is_ok()
        );

        mock.assert();
        let output_str = String::from_utf8(out)?;
        assert!(output_str.contains("is up to date."));
        Ok(())
    }
}

#[cfg(test)]
mod final_box_coverage_tests {
    use super::*;
    use crate::cli::commands::box_cmd::tests::{
        mock_add_args, mock_outdated_args, mock_update_args,
    };
    use httpmock::prelude::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_client_new_errors() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let boxes_dir = temp.path().join("boxes");
        fs::create_dir_all(&boxes_dir)?;
        let mut out = Vec::new();
        assert!(
            execute_with_config(
                &BoxCommands::Add(mock_add_args()),
                &mut out,
                temp.path(),
                || { Err(MigratoryError::Generic("client err".to_string())) }
            )
            .is_err()
        );
        assert!(
            execute_with_config(
                &BoxCommands::Outdated(mock_outdated_args()),
                &mut out,
                temp.path(),
                || Err(MigratoryError::Generic("client err".to_string()))
            )
            .is_err()
        );
        assert!(
            execute_with_config(
                &BoxCommands::Update(mock_update_args()),
                &mut out,
                temp.path(),
                || Err(MigratoryError::Generic("client err".to_string()))
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn test_box_add_version_loop_miss() -> Result<(), MigratoryError> {
        let server = MockServer::start();

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

        let temp = tempdir()?;
        let mut out = Vec::new();
        let url = server.base_url();
        let _ = execute_with_config(&BoxCommands::Add(args), &mut out, temp.path(), || {
            CloudClient::with_config(crate::cloud::CloudClientConfig {
                base_url: url.clone(),
                token: None,
            })
        });
        Ok(())
    }

    #[test]
    fn test_outdated_version_comparisons_and_metadata_error() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        let box_dir = boxes_dir.join("test-VAGRANTSLASH-box");
        fs::create_dir_all(box_dir.join("2.0.0"))?;
        fs::create_dir_all(box_dir.join("1.0.0"))?;

        let server = MockServer::start();

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
        let url = server.base_url();
        assert!(
            execute_with_config(
                &BoxCommands::Outdated(args.clone()),
                &mut out,
                &vagrant_home,
                || {
                    CloudClient::with_config(crate::cloud::CloudClientConfig {
                        base_url: url.clone(),
                        token: None,
                    })
                }
            )
            .is_ok()
        );

        let server_err = MockServer::start();
        server_err.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(404);
        });
        let url_err = server_err.base_url();
        assert!(
            execute_with_config(
                &BoxCommands::Outdated(args),
                &mut out,
                &vagrant_home,
                || {
                    CloudClient::with_config(crate::cloud::CloudClientConfig {
                        base_url: url_err.clone(),
                        token: None,
                    })
                }
            )
            .is_ok()
        );
        Ok(())
    }

    #[test]
    fn test_update_version_comparisons_and_download_fail() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        let box_dir = boxes_dir.join("test-VAGRANTSLASH-box");
        fs::create_dir_all(box_dir.join("2.0.0"))?;
        fs::create_dir_all(box_dir.join("1.0.0"))?;

        let server = MockServer::start();

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
        let url = server.base_url();
        assert!(
            execute_with_config(
                &BoxCommands::Update(args.clone()),
                &mut out,
                &vagrant_home,
                || {
                    CloudClient::with_config(crate::cloud::CloudClientConfig {
                        base_url: url.clone(),
                        token: None,
                    })
                }
            )
            .is_err()
        );

        let server_err = MockServer::start();
        server_err.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(404);
        });
        let url_err = server_err.base_url();
        assert!(
            execute_with_config(&BoxCommands::Update(args), &mut out, &vagrant_home, || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url_err.clone(),
                    token: None,
                })
            })
            .is_ok()
        );
        Ok(())
    }

    #[test]
    fn test_writeln_failures_in_outdated_update_repackage() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        fs::create_dir_all(&boxes_dir)?;

        let mut out = tests::FailingWriter;
        let mut args = mock_outdated_args();
        args.global = false;
        assert!(
            execute_with_config(
                &BoxCommands::Outdated(args),
                &mut out,
                &vagrant_home,
                CloudClient::new
            )
            .is_err()
        );

        let box_dir = boxes_dir.join("test-VAGRANTSLASH-box");
        fs::create_dir_all(box_dir.join("1.0.0"))?;
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(404);
        });
        let mut out2 = tests::FailingWriter;
        let mut args2 = mock_outdated_args();
        args2.global = true;
        let url = server.base_url();
        assert!(
            execute_with_config(
                &BoxCommands::Outdated(args2),
                &mut out2,
                &vagrant_home,
                || {
                    CloudClient::with_config(crate::cloud::CloudClientConfig {
                        base_url: url.clone(),
                        token: None,
                    })
                }
            )
            .is_err()
        );

        let mut out3 = tests::FailingWriter;
        assert!(
            execute_with_config(
                &BoxCommands::Update(mock_update_args()),
                &mut out3,
                &vagrant_home,
                || CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            )
            .is_err()
        );

        let temp_rep = tempdir()?;
        super::extra_tests::setup_fake_boxes(temp_rep.path())?;
        let mut out4 = tests::FailingWriter;
        let rep_args = crate::cli::BoxRepackageArgs {
            name: "test/box".to_string(),
            provider: "virtualbox".to_string(),
            version: "1.0.0".to_string(),
        };
        assert!(
            execute_with_config(
                &BoxCommands::Repackage(rep_args),
                &mut out4,
                temp_rep.path(),
                CloudClient::new
            )
            .is_err()
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_permission_denied_branches() -> Result<(), MigratoryError> {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempdir()?;
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        let box_dir = boxes_dir.join("test-VAGRANTSLASH-box");
        let ver_dir = box_dir.join("1.0.0");
        let prov_dir = ver_dir.join("virtualbox");
        fs::create_dir_all(&prov_dir)?;

        fs::set_permissions(&box_dir, fs::Permissions::from_mode(0o444))?;
        let mut out = Vec::new();
        let remove_args = crate::cli::BoxRemoveArgs {
            name: "test/box".to_string(),
            force: true,
            ..Default::default()
        };
        let res_remove = execute_with_config(
            &BoxCommands::Remove(remove_args),
            &mut out,
            &vagrant_home,
            CloudClient::new,
        );
        fs::set_permissions(&box_dir, fs::Permissions::from_mode(0o755))?;
        assert!(res_remove.is_err());

        fs::set_permissions(&ver_dir, fs::Permissions::from_mode(0o000))?;
        let mut out = Vec::new();
        let _ = execute_with_config(
            &BoxCommands::List(crate::cli::BoxListArgs::default()),
            &mut out,
            &vagrant_home,
            CloudClient::new,
        );
        let _ = execute_with_config(
            &BoxCommands::Outdated(mock_outdated_args()),
            &mut out,
            &vagrant_home,
            CloudClient::new,
        );
        let _ = execute_with_config(
            &BoxCommands::Update(mock_update_args()),
            &mut out,
            &vagrant_home,
            CloudClient::new,
        );
        fs::set_permissions(&ver_dir, fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    #[test]
    fn test_execute_box_outdated_with_empty_remote_versions() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let vagrant_home = temp.path().join(".vagrant.d");
        let boxes_dir = vagrant_home.join("boxes");
        fs::create_dir_all(&boxes_dir)?;

        let box_dir = boxes_dir.join("hashicorp-VAGRANTSLASH-bionic64");
        fs::create_dir_all(box_dir.join("1.0.0"))?;

        let server = MockServer::start();

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
        let url = server.base_url();
        let result = execute_with_config(&cmd, &mut out, &vagrant_home, || {
            CloudClient::with_config(crate::cloud::CloudClientConfig {
                base_url: url.clone(),
                token: None,
            })
        });
        assert!(result.is_ok());
        let out_str = String::from_utf8_lossy(&out);
        assert!(out_str.contains("is up to date"));

        // Also test when remote version is newer than local version
        let server_newer = MockServer::start();
        let _mock_newer = server_newer.mock(|when, then| {
            when.method(GET).path("/boxes/hashicorp/bionic64");
            then.status(200).json_body(serde_json::json!({
                "description_markdown": "Test Box",
                "short_description": "Short",
                "name": "hashicorp/bionic64",
                "versions": [{"version": "2.0.0", "status": "active", "description_html": "", "description_markdown": "", "providers": []}]
            }));
        });
        let mut out_newer = Vec::new();
        let url_newer = server_newer.base_url();
        assert!(
            execute_with_config(&cmd, &mut out_newer, &vagrant_home, || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url_newer.clone(),
                    token: None,
                })
            })
            .is_ok()
        );
        assert!(String::from_utf8_lossy(&out_newer).contains("is outdated!"));
        Ok(())
    }

    #[test]
    fn test_execute_box_add_local_box_file_suffix() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let vagrant_home = temp.path().join(".vagrant.d");
        fs::create_dir_all(&vagrant_home)?;

        let box_file = temp.path().join("local_package.box");
        fs::write(&box_file, "dummy content")?;

        let mut args = mock_add_args();
        args.name = box_file.to_string_lossy().to_string();
        let cmd = BoxCommands::Add(args);

        let mut out = Vec::new();
        let _ = execute_with_config(&cmd, &mut out, &vagrant_home, CloudClient::new);
        Ok(())
    }

    #[test]
    fn test_execute_box_add_http_url() -> Result<(), MigratoryError> {
        let temp = tempdir()?;
        let vagrant_home = temp.path().join(".vagrant.d");
        fs::create_dir_all(&vagrant_home)?;

        let mut args = mock_add_args();
        args.name = "http://127.0.0.1:9/box.box".to_string();
        let cmd = BoxCommands::Add(args);

        let mut out = Vec::new();
        let _ = execute_with_config(&cmd, &mut out, &vagrant_home, CloudClient::new);
        Ok(())
    }
}
