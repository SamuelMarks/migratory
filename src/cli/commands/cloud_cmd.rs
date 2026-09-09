//! Cloud command implementation.
//!
//! Handles the `cloud` subcommand and its various operations such as
//! `auth`, `box`, `provider`, `publish`, `search`, and `version`.

use crate::cli::{
    CloudAuthCommands, CloudBoxCommands, CloudCommands, CloudProviderCommands, CloudVersionCommands,
};
use crate::error::MigratoryError;
use std::io::Write;

/// Executes the `cloud` command cluster.
///
/// # Arguments
///
/// * `cmd` - The parsed `CloudCommands` subcommand enum.
/// * `writer` - A writable destination for output (e.g. stdout).
///
/// # Returns
///
/// Returns `Ok(())` on successful execution, or a `MigratoryError` on failure.
///
/// # Errors
///
/// Returns a `MigratoryError` if writing to the output stream fails.
pub fn execute(cmd: &CloudCommands, writer: &mut dyn Write) -> Result<(), MigratoryError> {
    match cmd {
        CloudCommands::Auth(auth_cmd) => execute_auth(auth_cmd, writer)?,
        CloudCommands::Box(box_cmd) => execute_box(box_cmd, writer)?,
        CloudCommands::Provider(provider_cmd) => execute_provider(provider_cmd, writer)?,
        CloudCommands::Publish(args) => {
            let box_name = args.name.as_deref().unwrap_or("test/box");
            let version = args.version.as_deref().unwrap_or("1.0.0");
            let provider = args.provider.as_deref().unwrap_or("virtualbox");

            writeln!(
                writer,
                "Publishing box {} v{} for provider {}...",
                box_name, version, provider
            )?;

            let client = crate::cloud::CloudClient::new()?;

            client.create_box(
                box_name,
                args.description.as_deref(),
                args.short_description.as_deref(),
            )?;
            client.create_version(box_name, version, args.version_description.as_deref())?;
            client.create_provider(box_name, version, provider)?;

            if let Some(file_path) = &args.file_path {
                let upload_url = client.get_upload_url(box_name, version, provider)?;
                client.upload_file(&upload_url, std::path::Path::new(file_path))?;
            }

            writeln!(writer, "Box published successfully!")?;
        }
        CloudCommands::Search(args) => {
            let query = args.query.as_deref().unwrap_or("");
            let client = crate::cloud::CloudClient::new()?;

            writeln!(writer, "Searching for boxes matching '{}'...", query)?;

            let results = client.search_boxes(query)?;

            if results.is_empty() {
                writeln!(writer, "No boxes found.")?;
            } else {
                for b in results.iter().take(args.limit.unwrap_or(usize::MAX)) {
                    writeln!(writer, "{} - {}", b.name, b.short_description)?;
                }
            }
        }
        CloudCommands::Version(version_cmd) => execute_version(version_cmd, writer)?,
    }
    Ok(())
}

fn execute_auth(cmd: &CloudAuthCommands, writer: &mut dyn Write) -> Result<(), MigratoryError> {
    let client = crate::cloud::CloudClient::new()?;
    match cmd {
        CloudAuthCommands::Login(args) => {
            let _ = args;
            writeln!(
                writer,
                "Please use `migratory login` for interactive authentication."
            )?;
        }
        CloudAuthCommands::Logout => {
            client.delete_token()?;
            writeln!(writer, "Logged out successfully.")?;
        }
        CloudAuthCommands::Whoami => {
            let user = client.whoami()?;
            writeln!(writer, "Currently logged in as: {}", user)?;
        }
    };
    Ok(())
}

fn execute_box(cmd: &CloudBoxCommands, writer: &mut dyn Write) -> Result<(), MigratoryError> {
    let client = crate::cloud::CloudClient::new()?;
    match cmd {
        CloudBoxCommands::Create(args) => {
            let name = args.name.as_deref().unwrap_or("");
            client.create_box(
                name,
                args.description.as_deref(),
                args.short_description.as_deref(),
            )?;
            writeln!(writer, "Box created successfully.")?;
        }
        CloudBoxCommands::Delete(args) => {
            let name = args.name.as_deref().unwrap_or("");
            client.delete_box(name)?;
            writeln!(writer, "Box deleted successfully.")?;
        }
        CloudBoxCommands::Show(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let meta = client.fetch_metadata(name)?;
            writeln!(writer, "Box: {}", meta.name)?;
            writeln!(writer, "Description: {}", meta.short_description)?;
            for v in meta.versions {
                writeln!(writer, "  Version: {}", v.version)?;
                for p in v.providers {
                    writeln!(writer, "    Provider: {}", p.name)?;
                }
            }
        }
        CloudBoxCommands::Update(args) => {
            let name = args.name.as_deref().unwrap_or("");
            client.update_box(
                name,
                args.description.as_deref(),
                args.short_description.as_deref(),
            )?;
            writeln!(writer, "Box updated successfully.")?;
        }
    };
    Ok(())
}

fn execute_provider(
    cmd: &CloudProviderCommands,
    mut writer: impl Write,
) -> Result<(), MigratoryError> {
    let client = crate::cloud::CloudClient::new()?;
    match cmd {
        CloudProviderCommands::Create(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let version = args.version.as_deref().unwrap_or("");
            let provider = args.provider.as_deref().unwrap_or("");
            client.create_provider(name, version, provider)?;
            writeln!(writer, "Provider created successfully.")?;
        }
        CloudProviderCommands::Delete(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let version = args.version.as_deref().unwrap_or("");
            let provider = args.provider.as_deref().unwrap_or("");
            client.delete_provider(name, version, provider)?;
            writeln!(writer, "Provider deleted successfully.")?;
        }
        CloudProviderCommands::Update(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let version = args.version.as_deref().unwrap_or("");
            let provider = args.provider.as_deref().unwrap_or("");
            client.update_provider(
                name,
                version,
                provider,
                args.checksum.as_deref(),
                args.checksum_type.as_deref(),
            )?;
            writeln!(writer, "Provider updated successfully.")?;
        }
        CloudProviderCommands::Upload(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let version = args.version.as_deref().unwrap_or("");
            let provider = args.provider.as_deref().unwrap_or("");
            let upload_url = client.get_upload_url(name, version, provider)?;
            if let Some(file_path) = &args.file_path {
                client.upload_file(&upload_url, std::path::Path::new(file_path))?;
                writeln!(writer, "Provider upload successful.")?;
            } else {
                writeln!(writer, "No file path provided for upload.")?;
            }
        }
    };
    Ok(())
}

fn execute_version(
    cmd: &CloudVersionCommands,
    mut writer: impl Write,
) -> Result<(), MigratoryError> {
    let client = crate::cloud::CloudClient::new()?;
    match cmd {
        CloudVersionCommands::Create(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let version = args.version.as_deref().unwrap_or("");
            client.create_version(name, version, args.description.as_deref())?;
            writeln!(writer, "Version created successfully.")?;
        }
        CloudVersionCommands::Delete(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let version = args.version.as_deref().unwrap_or("");
            client.delete_version(name, version)?;
            writeln!(writer, "Version deleted successfully.")?;
        }
        CloudVersionCommands::Release(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let version = args.version.as_deref().unwrap_or("");
            client.release_version(name, version)?;
            writeln!(writer, "Version released successfully.")?;
        }
        CloudVersionCommands::Revoke(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let version = args.version.as_deref().unwrap_or("");
            client.revoke_version(name, version)?;
            writeln!(writer, "Version revoked successfully.")?;
        }
        CloudVersionCommands::Update(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let version = args.version.as_deref().unwrap_or("");
            client.update_version(name, version, args.description.as_deref())?;
            writeln!(writer, "Version updated successfully.")?;
        }
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[derive(Default)]
    pub(crate) struct FailingWriter {
        pub fail_on_content: Option<String>,
        pub accumulated: String,
    }

    impl Write for FailingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.accumulated.push_str(&String::from_utf8_lossy(buf));
            if let Some(ref needle) = self.fail_on_content {
                if self.accumulated.contains(needle) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "write failed",
                    ));
                }
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "write failed",
                ));
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    pub(crate) fn mock_publish_args() -> crate::cli::CloudPublishArgs {
        crate::cli::CloudPublishArgs {
            name: Some("test/box".to_string()),
            version: Some("1.0.0".to_string()),
            provider: Some("virtualbox".to_string()),
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
        }
    }

    pub(crate) fn mock_search_args() -> crate::cli::CloudSearchArgs {
        crate::cli::CloudSearchArgs {
            query: Some("test".to_string()),
            json: false,
            short: false,
            limit: None,
            sort_by: None,
            order: None,
            page: None,
            provider: None,
            architecture: None,
            auth: None,
            no_auth: false,
        }
    }

    #[test]
    fn test_execute_cloud_auth() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        let mock_delete = server.mock(|when, then| {
            when.method(DELETE).path("/api/v2/authenticate");
            then.status(200);
        });

        let mock_whoami = server.mock(|when, then| {
            when.method(GET).path("/api/v2/authenticate");
            then.status(200).json_body(serde_json::json!({
                "user": {
                    "username": "testuser"
                }
            }));
        });

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Auth(CloudAuthCommands::Login(crate::cli::CloudAuthLoginArgs {
                    token: None,
                    username: None,
                    description: None,
                    check: false,
                })),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(
            output_str.trim(),
            "Please use `migratory login` for interactive authentication."
        );

        let mut out = Vec::new();
        assert!(execute(&CloudCommands::Auth(CloudAuthCommands::Logout), &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Logged out successfully.");
        mock_delete.assert();

        let mut out = Vec::new();
        assert!(execute(&CloudCommands::Auth(CloudAuthCommands::Whoami), &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Currently logged in as: testuser");
        mock_whoami.assert();
    }

    #[test]
    fn test_execute_cloud_auth_write_error() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let mut out = FailingWriter::default();
        let result = execute(
            &CloudCommands::Auth(CloudAuthCommands::Login(crate::cli::CloudAuthLoginArgs {
                token: None,
                username: None,
                description: None,
                check: false,
            })),
            &mut out,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_cloud_box() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        let mock_create = server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });

        let mock_delete = server.mock(|when, then| {
            when.method(DELETE).path("/api/v2/boxes/test/box");
            then.status(200);
        });

        let mock_show = server.mock(|when, then| {
            when.method(GET).path("/api/v2/boxes/test/box");
            then.status(200).json_body(serde_json::json!({
                "name": "test/box",
                "short_description": "short",
                "description_markdown": "",
                "versions": [
                    {
                        "version": "1.0",
                        "providers": [
                            { "name": "virtualbox", "url": "http" }
                        ]
                    }
                ]
            }));
        });

        let mock_update = server.mock(|when, then| {
            when.method(PUT).path("/api/v2/boxes/test/box");
            then.status(200);
        });

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Box(CloudBoxCommands::Create(crate::cli::CloudBoxCreateArgs {
                    name: Some("test/box".to_string()),
                    ..Default::default()
                })),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Box created successfully.");
        mock_create.assert();

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Box(CloudBoxCommands::Delete(crate::cli::CloudBoxDeleteArgs {
                    name: Some("test/box".to_string()),
                    ..Default::default()
                })),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Box deleted successfully.");
        mock_delete.assert();

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Box(CloudBoxCommands::Show(crate::cli::CloudBoxShowArgs {
                    name: Some("test/box".to_string()),
                    ..Default::default()
                })),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(
            output_str.trim(),
            "Box: test/box
Description: short
  Version: 1.0
    Provider: virtualbox"
        );
        mock_show.assert();

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Box(CloudBoxCommands::Update(crate::cli::CloudBoxUpdateArgs {
                    name: Some("test/box".to_string()),
                    ..Default::default()
                })),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Box updated successfully.");
        mock_update.assert();
    }

    #[test]
    fn test_execute_cloud_box_write_error() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });
        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });

        let mut out = FailingWriter::default();
        let result = execute(
            &CloudCommands::Box(CloudBoxCommands::Create(crate::cli::CloudBoxCreateArgs {
                name: Some("test/box".to_string()),
                ..Default::default()
            })),
            &mut out,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_cloud_provider() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        let mock_create = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });

        let mock_delete = server.mock(|when, then| {
            when.method(DELETE)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox");
            then.status(200);
        });

        let mock_update = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox");
            then.status(200);
        });

        let mock_upload = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox/upload");
            then.status(200).json_body(serde_json::json!({
                "upload_path": server.url("/upload")
            }));
        });

        let mock_upload_put = server.mock(|when, then| {
            when.method(PUT).path("/upload");
            then.status(200);
        });

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Create(
                    crate::cli::CloudProviderCreateArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Provider created successfully.");
        mock_create.assert();

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Delete(
                    crate::cli::CloudProviderDeleteArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Provider deleted successfully.");
        mock_delete.assert();

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Update(
                    crate::cli::CloudProviderUpdateArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Provider updated successfully.");
        mock_update.assert();

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Upload(
                    crate::cli::CloudProviderUploadArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "No file path provided for upload.");
        mock_upload.assert();
        mock_upload_put.assert_calls(0);

        use std::io::Write as _;
        let mut tf = tempfile::NamedTempFile::new().expect("operation should succeed");
        tf.write_all(b"test").expect("operation should succeed");
        let path_str = tf.path().to_string_lossy().to_string();

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Upload(
                    crate::cli::CloudProviderUploadArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        file_path: Some(path_str),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Provider upload successful.");
        mock_upload_put.assert_calls(1);
    }

    #[test]
    fn test_execute_cloud_provider_write_error() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        server.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });

        let mut out = FailingWriter::default();
        let result = execute(
            &CloudCommands::Provider(CloudProviderCommands::Create(
                crate::cli::CloudProviderCreateArgs {
                    name: Some("test/box".to_string()),
                    version: Some("1.0.0".to_string()),
                    provider: Some("virtualbox".to_string()),
                    ..Default::default()
                },
            )),
            &mut out,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_cloud_version() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        let mock_create = server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });

        let mock_delete = server.mock(|when, then| {
            when.method(DELETE)
                .path("/api/v2/boxes/test/box/versions/1.0.0");
            then.status(200);
        });

        let mock_release = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/release");
            then.status(200);
        });

        let mock_revoke = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/revoke");
            then.status(200);
        });

        let mock_update = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0");
            then.status(200);
        });

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Create(
                    crate::cli::CloudVersionCreateArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        description: None,
                    }
                )),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Version created successfully.");
        mock_create.assert();

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Delete(
                    crate::cli::CloudVersionDeleteArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Version deleted successfully.");
        mock_delete.assert();

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Release(
                    crate::cli::CloudVersionReleaseArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Version released successfully.");
        mock_release.assert();

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Revoke(
                    crate::cli::CloudVersionRevokeArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Version revoked successfully.");
        mock_revoke.assert();

        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Update(
                    crate::cli::CloudVersionUpdateArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Version updated successfully.");
        mock_update.assert();
    }

    #[test]
    fn test_execute_cloud_version_write_error() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });

        let mut out = FailingWriter::default();
        let result = execute(
            &CloudCommands::Version(CloudVersionCommands::Create(
                crate::cli::CloudVersionCreateArgs {
                    name: Some("test/box".to_string()),
                    version: Some("1.0.0".to_string()),
                    ..Default::default()
                },
            )),
            &mut out,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_cloud_search() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        server.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/search")
                .query_param("q", "test");
            then.status(200).json_body(serde_json::json!({
                "boxes": [
                    {
                        "name": "test/box",
                        "short_description": "test box",
                        "description_markdown": "",
                        "versions": []
                    }
                ]
            }));
        });

        let mut out = Vec::new();
        assert!(execute(&CloudCommands::Search(mock_search_args()), &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("Searching for boxes matching 'test'"));
    }

    #[test]
    fn test_execute_cloud_search_empty() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        server.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/search")
                .query_param("q", "test");
            then.status(200).json_body(serde_json::json!({
                "boxes": []
            }));
        });

        let mut out = Vec::new();
        assert!(execute(&CloudCommands::Search(mock_search_args()), &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("No boxes found."));
    }

    #[test]
    fn test_execute_cloud_search_write_error() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        server.mock(|when, then| {
            when.method(GET).path("/api/v2/search");
            then.status(200).json_body(serde_json::json!({"boxes": []}));
        });
        let mut out = FailingWriter::default();
        let result = execute(&CloudCommands::Search(mock_search_args()), &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_cloud_publish() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });
        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });

        let mut out = Vec::new();
        assert!(execute(&CloudCommands::Publish(mock_publish_args()), &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("Publishing box test/box v1.0.0 for provider virtualbox..."));
    }

    #[test]
    fn test_execute_cloud_publish_with_file() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });
        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });

        server.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox/upload");
            then.status(200).json_body(serde_json::json!({
                "upload_path": server.url("/upload")
            }));
        });
        server.mock(|when, then| {
            when.method(PUT).path("/upload");
            then.status(200);
        });

        let mut args = mock_publish_args();

        use std::io::Write as _;
        let mut tf = tempfile::NamedTempFile::new().expect("operation should succeed");
        tf.write_all(b"test").expect("operation should succeed");
        args.file_path = Some(tf.path().to_string_lossy().to_string());

        let mut out = Vec::new();
        assert!(execute(&CloudCommands::Publish(args), &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("Box published successfully!"));
    }

    #[test]
    fn test_execute_cloud_failing_writer_flush() {
        use std::io::Write;
        let mut writer = FailingWriter::default();
        assert!(writer.flush().is_ok());
    }

    #[test]
    fn test_execute_cloud_box_create_error() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        let mock_create = server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(401);
        });

        let mut out = Vec::new();
        let res = execute(
            &CloudCommands::Box(CloudBoxCommands::Create(crate::cli::CloudBoxCreateArgs {
                name: Some("test/box".to_string()),
                ..Default::default()
            })),
            &mut out,
        );
        assert!(res.is_err());
        mock_create.assert();
    }

    #[test]
    fn test_execute_cloud_box_update_error() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        let mock_update = server.mock(|when, then| {
            when.method(PUT).path("/api/v2/boxes/test/box");
            then.status(401);
        });

        let mut out = Vec::new();
        let res = execute(
            &CloudCommands::Box(CloudBoxCommands::Update(crate::cli::CloudBoxUpdateArgs {
                name: Some("test/box".to_string()),
                ..Default::default()
            })),
            &mut out,
        );
        assert!(res.is_err());
        mock_update.assert();
    }

    #[test]
    fn test_execute_cloud_provider_update_error() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        let mock_update = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0/providers/virtualbox");
            then.status(401);
        });

        let mut out = Vec::new();
        let res = execute(
            &CloudCommands::Provider(CloudProviderCommands::Update(
                crate::cli::CloudProviderUpdateArgs {
                    name: Some("test/box".to_string()),
                    version: Some("1.0".to_string()),
                    provider: Some("virtualbox".to_string()),
                    ..Default::default()
                },
            )),
            &mut out,
        );
        assert!(res.is_err());
        mock_update.assert();
    }

    #[test]
    fn test_execute_cloud_publish_write_error() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });
        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });

        let mut out = FailingWriter::default();
        let result = execute(&CloudCommands::Publish(mock_publish_args()), &mut out);
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod extra_cloud_cmd_coverage_tests {
    use super::*;
    use httpmock::Method::{DELETE, GET, POST, PUT};
    use httpmock::MockServer;

    #[test]
    fn test_execute_cloud_client_new_error() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_CLIENT_ERROR", "1");
        }
        let mut out = Vec::new();
        assert!(execute(&CloudCommands::Publish(Default::default()), &mut out).is_err());
        assert!(execute(&CloudCommands::Search(Default::default()), &mut out).is_err());
        assert!(execute(&CloudCommands::Auth(CloudAuthCommands::Logout), &mut out).is_err());
        assert!(
            execute(
                &CloudCommands::Box(CloudBoxCommands::Show(Default::default())),
                &mut out
            )
            .is_err()
        );
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Delete(Default::default())),
                &mut out
            )
            .is_err()
        );
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Delete(Default::default())),
                &mut out
            )
            .is_err()
        );
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_CLIENT_ERROR");
        }
    }

    #[test]
    fn test_execute_cloud_publish_errors() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        // 0. create_box fails
        let server0 = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server0.url("/api/v2"));
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        server0.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(401);
        });
        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Publish(tests::mock_publish_args()),
                &mut out
            )
            .is_err()
        );

        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        // 1. create_version fails
        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });
        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(401);
        });
        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Publish(tests::mock_publish_args()),
                &mut out
            )
            .is_err()
        );

        // 2. create_provider fails
        let server2 = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server2.url("/api/v2"));
        }
        server2.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });
        server2.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });
        server2.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(401);
        });
        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Publish(tests::mock_publish_args()),
                &mut out
            )
            .is_err()
        );

        // 3. get_upload_url fails with file_path
        let server3 = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server3.url("/api/v2"));
        }
        server3.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });
        server3.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });
        server3.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });
        server3.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox/upload");
            then.status(401);
        });
        let mut tf = tempfile::NamedTempFile::new().expect("operation should succeed");
        std::io::Write::write_all(&mut tf, b"test").expect("operation should succeed");
        let mut args_with_file = tests::mock_publish_args();
        args_with_file.file_path = Some(tf.path().to_string_lossy().to_string());
        let mut out = Vec::new();
        assert!(execute(&CloudCommands::Publish(args_with_file.clone()), &mut out).is_err());

        // 4. upload_file fails (bad upload_path)
        let server4 = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server4.url("/api/v2"));
        }
        server4.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });
        server4.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });
        server4.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });
        server4.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox/upload");
            then.status(200).json_body(serde_json::json!({
                "upload_path": "http://127.0.0.1:1/nonexistent"
            }));
        });
        let mut out = Vec::new();
        assert!(execute(&CloudCommands::Publish(args_with_file), &mut out).is_err());
    }

    #[test]
    fn test_execute_cloud_search_error() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        server.mock(|when, then| {
            when.method(GET).path("/api/v2/search");
            then.status(401);
        });
        let mut out = Vec::new();
        assert!(execute(&CloudCommands::Search(tests::mock_search_args()), &mut out).is_err());
    }

    #[test]
    fn test_execute_cloud_auth_errors() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        server.mock(|when, then| {
            when.method(DELETE).path("/api/v2/authenticate");
            then.status(401);
        });
        server.mock(|when, then| {
            when.method(GET).path("/api/v2/authenticate");
            then.status(401);
        });
        let mut out = Vec::new();
        assert!(execute(&CloudCommands::Auth(CloudAuthCommands::Logout), &mut out).is_err());
        assert!(execute(&CloudCommands::Auth(CloudAuthCommands::Whoami), &mut out).is_err());
    }

    #[test]
    fn test_execute_cloud_box_errors() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        server.mock(|when, then| {
            when.method(DELETE).path("/api/v2/boxes/test/box");
            then.status(401);
        });
        server.mock(|when, then| {
            when.method(GET).path("/api/v2/boxes/test/box");
            then.status(401);
        });
        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Box(CloudBoxCommands::Delete(crate::cli::CloudBoxDeleteArgs {
                    name: Some("test/box".to_string()),
                    ..Default::default()
                })),
                &mut out
            )
            .is_err()
        );
        assert!(
            execute(
                &CloudCommands::Box(CloudBoxCommands::Show(crate::cli::CloudBoxShowArgs {
                    name: Some("test/box".to_string()),
                    ..Default::default()
                })),
                &mut out
            )
            .is_err()
        );
    }

    #[test]
    fn test_execute_cloud_provider_errors() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        server.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(401);
        });
        server.mock(|when, then| {
            when.method(DELETE)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox");
            then.status(401);
        });
        server.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox/upload");
            then.status(401);
        });
        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Create(
                    crate::cli::CloudProviderCreateArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Delete(
                    crate::cli::CloudProviderDeleteArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );
        let mut tf = tempfile::NamedTempFile::new().expect("operation should succeed");
        std::io::Write::write_all(&mut tf, b"test").expect("operation should succeed");
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Upload(
                    crate::cli::CloudProviderUploadArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        file_path: Some(tf.path().to_string_lossy().to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );

        // upload_file error
        let server_up = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server_up.url("/api/v2"));
        }
        server_up.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox/upload");
            then.status(200).json_body(serde_json::json!({
                "upload_path": "http://127.0.0.1:1/nonexistent"
            }));
        });
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Upload(
                    crate::cli::CloudProviderUploadArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        file_path: Some(tf.path().to_string_lossy().to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );
    }

    #[test]
    fn test_execute_cloud_version_errors() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v2"));
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }
        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(401);
        });
        server.mock(|when, then| {
            when.method(DELETE)
                .path("/api/v2/boxes/test/box/versions/1.0.0");
            then.status(401);
        });
        server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/release");
            then.status(401);
        });
        server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/revoke");
            then.status(401);
        });
        server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0");
            then.status(401);
        });
        let mut out = Vec::new();
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Create(
                    crate::cli::CloudVersionCreateArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Delete(
                    crate::cli::CloudVersionDeleteArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Release(
                    crate::cli::CloudVersionReleaseArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Revoke(
                    crate::cli::CloudVersionRevokeArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Update(
                    crate::cli::CloudVersionUpdateArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );
    }

    #[test]
    fn test_execute_cloud_all_writeln_failures() {
        let _env_lock = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_TOKEN", "test-token");
        }

        // Publish: fail on final writeln
        let s_pub = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_pub.url("/api/v2"));
        }
        s_pub.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });
        s_pub.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });
        s_pub.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Box published successfully!".to_string()),
            ..Default::default()
        };
        assert!(
            execute(
                &CloudCommands::Publish(tests::mock_publish_args()),
                &mut out
            )
            .is_err()
        );

        // Search empty: fail on "No boxes found."
        let s_search_empty = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_search_empty.url("/api/v2"));
        }
        s_search_empty.mock(|when, then| {
            when.method(GET).path("/api/v2/search");
            then.status(200)
                .json_body(serde_json::json!({ "boxes": [] }));
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("No boxes found.".to_string()),
            ..Default::default()
        };
        assert!(execute(&CloudCommands::Search(tests::mock_search_args()), &mut out).is_err());

        // Search with box: fail on printing box
        let s_search = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_search.url("/api/v2"));
        }
        s_search.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/search")
                .query_param("q", "test");
            then.status(200).json_body(serde_json::json!({
                "boxes": [{
                    "name": "test/box",
                    "short_description": "desc",
                    "description_markdown": "",
                    "versions": []
                }]
            }));
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("test/box - desc".to_string()),
            ..Default::default()
        };
        assert!(execute(&CloudCommands::Search(tests::mock_search_args()), &mut out).is_err());

        // Auth logout: fail on "Logged out successfully."
        let s_logout = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_logout.url("/api/v2"));
        }
        s_logout.mock(|when, then| {
            when.method(DELETE).path("/api/v2/authenticate");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Logged out successfully.".to_string()),
            ..Default::default()
        };
        assert!(execute(&CloudCommands::Auth(CloudAuthCommands::Logout), &mut out).is_err());

        // Auth whoami: fail on "Currently logged in as..."
        let s_whoami = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_whoami.url("/api/v2"));
        }
        s_whoami.mock(|when, then| {
            when.method(GET).path("/api/v2/authenticate");
            then.status(200)
                .json_body(serde_json::json!({ "user": { "username": "u" } }));
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Currently logged in as:".to_string()),
            ..Default::default()
        };
        assert!(execute(&CloudCommands::Auth(CloudAuthCommands::Whoami), &mut out).is_err());

        // Box delete: fail on "Box deleted successfully."
        let s_bdel = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_bdel.url("/api/v2"));
        }
        s_bdel.mock(|when, then| {
            when.method(DELETE).path("/api/v2/boxes/test/box");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Box deleted successfully.".to_string()),
            ..Default::default()
        };
        assert!(
            execute(
                &CloudCommands::Box(CloudBoxCommands::Delete(crate::cli::CloudBoxDeleteArgs {
                    name: Some("test/box".to_string()),
                    ..Default::default()
                })),
                &mut out
            )
            .is_err()
        );

        // Box show: fail on writeln lines "Box: ", "Description: ", "Version: ", "Provider: "
        let s_bshow = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_bshow.url("/api/v2"));
        }
        s_bshow.mock(|when, then| {
            when.method(GET).path("/api/v2/boxes/test/box");
            then.status(200).json_body(serde_json::json!({
                "name": "test/box",
                "short_description": "desc",
                "description_markdown": "",
                "versions": [{
                    "version": "1.0",
                    "providers": [{ "name": "virtualbox", "url": "http" }]
                }]
            }));
        });
        for content in ["Box: ", "Description: ", "Version: ", "Provider: "] {
            let mut out = tests::FailingWriter {
                fail_on_content: Some(content.to_string()),
                ..Default::default()
            };
            assert!(
                execute(
                    &CloudCommands::Box(CloudBoxCommands::Show(crate::cli::CloudBoxShowArgs {
                        name: Some("test/box".to_string()),
                        ..Default::default()
                    })),
                    &mut out
                )
                .is_err()
            );
        }

        // Box update: fail on "Box updated successfully."
        let s_bup = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_bup.url("/api/v2"));
        }
        s_bup.mock(|when, then| {
            when.method(PUT).path("/api/v2/boxes/test/box");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Box updated successfully.".to_string()),
            ..Default::default()
        };
        assert!(
            execute(
                &CloudCommands::Box(CloudBoxCommands::Update(crate::cli::CloudBoxUpdateArgs {
                    name: Some("test/box".to_string()),
                    ..Default::default()
                })),
                &mut out
            )
            .is_err()
        );

        // Provider delete: fail on "Provider deleted successfully."
        let s_pdel = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_pdel.url("/api/v2"));
        }
        s_pdel.mock(|when, then| {
            when.method(DELETE)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Provider deleted successfully.".to_string()),
            ..Default::default()
        };
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Delete(
                    crate::cli::CloudProviderDeleteArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );

        // Provider update: fail on "Provider updated successfully."
        let s_pup = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_pup.url("/api/v2"));
        }
        s_pup.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Provider updated successfully.".to_string()),
            ..Default::default()
        };
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Update(
                    crate::cli::CloudProviderUpdateArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );

        // Provider upload without file: fail on "No file path provided for upload."
        let s_pupload_nofile = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_pupload_nofile.url("/api/v2"));
        }
        s_pupload_nofile.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox/upload");
            then.status(200)
                .json_body(serde_json::json!({ "upload_path": s_pupload_nofile.url("/upload") }));
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("No file path provided for upload.".to_string()),
            ..Default::default()
        };
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Upload(
                    crate::cli::CloudProviderUploadArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );

        // Provider upload with file: fail on "Provider upload successful."
        let s_pupload_file = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_pupload_file.url("/api/v2"));
        }
        s_pupload_file.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox/upload");
            then.status(200)
                .json_body(serde_json::json!({ "upload_path": s_pupload_file.url("/upload") }));
        });
        s_pupload_file.mock(|when, then| {
            when.method(PUT).path("/upload");
            then.status(200);
        });
        let mut tf = tempfile::NamedTempFile::new().expect("operation should succeed");
        std::io::Write::write_all(&mut tf, b"test").expect("operation should succeed");
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Provider upload successful.".to_string()),
            ..Default::default()
        };
        assert!(
            execute(
                &CloudCommands::Provider(CloudProviderCommands::Upload(
                    crate::cli::CloudProviderUploadArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        provider: Some("virtualbox".to_string()),
                        file_path: Some(tf.path().to_string_lossy().to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );

        // Version delete: fail on "Version deleted successfully."
        let s_vdel = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_vdel.url("/api/v2"));
        }
        s_vdel.mock(|when, then| {
            when.method(DELETE)
                .path("/api/v2/boxes/test/box/versions/1.0.0");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Version deleted successfully.".to_string()),
            ..Default::default()
        };
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Delete(
                    crate::cli::CloudVersionDeleteArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );

        // Version release: fail on "Version released successfully."
        let s_vrel = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_vrel.url("/api/v2"));
        }
        s_vrel.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/release");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Version released successfully.".to_string()),
            ..Default::default()
        };
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Release(
                    crate::cli::CloudVersionReleaseArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );

        // Version revoke: fail on "Version revoked successfully."
        let s_vrev = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_vrev.url("/api/v2"));
        }
        s_vrev.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/revoke");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Version revoked successfully.".to_string()),
            ..Default::default()
        };
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Revoke(
                    crate::cli::CloudVersionRevokeArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );

        // Version update: fail on "Version updated successfully."
        let s_vup = MockServer::start();
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", s_vup.url("/api/v2"));
        }
        s_vup.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Version updated successfully.".to_string()),
            ..Default::default()
        };
        assert!(
            execute(
                &CloudCommands::Version(CloudVersionCommands::Update(
                    crate::cli::CloudVersionUpdateArgs {
                        name: Some("test/box".to_string()),
                        version: Some("1.0.0".to_string()),
                        ..Default::default()
                    }
                )),
                &mut out
            )
            .is_err()
        );
    }
}
