//! Cloud command implementation.
//!
//! Handles the `cloud` subcommand and its sub-operations (`auth`, `box`, `provider`, `publish`, `search`, `version`).

use crate::cli::{
    CloudAuthCommands, CloudBoxCommands, CloudCommands, CloudProviderCommands, CloudVersionCommands,
};
use crate::error::MigratoryError;
use std::io::Write;

/// Executes the `cloud` command.
///
/// # Arguments
///
/// * `cmd` - The parsed `CloudCommands` subcommand.
/// * `writer` - A writable destination for output (e.g. stdout).
///
/// # Returns
///
/// Returns `Ok(())` on successful execution, or a `MigratoryError` on failure.
///
/// # Errors
///
/// Returns a `MigratoryError` if writing to the output stream fails.
pub fn execute(cmd: &CloudCommands, mut writer: impl Write) -> Result<(), MigratoryError> {
    let client = crate::cloud::CloudClient::new()?;
    let home_dir = std::env::var("VAGRANT_HOME").unwrap_or_else(|_| ".vagrant.d".to_string());
    execute_with_client(cmd, &mut writer, &client, std::path::Path::new(&home_dir))
}

/// Executes the `cloud` command using an explicitly provided [`crate::cloud::CloudClient`] and Vagrant home directory.
///
/// # Arguments
///
/// * `cmd` - The `CloudCommands` subcommand to execute.
/// * `writer` - A mutable reference to a trait object writer.
/// * `client` - Configured [`crate::cloud::CloudClient`] instance.
/// * `home_dir` - Base directory for Vagrant global state.
///
/// # Returns
///
/// Returns `Ok(())` on successful execution, or a `MigratoryError` on failure.
///
/// # Errors
///
/// Returns a `MigratoryError` if an API call or output write fails.
pub fn execute_with_client(
    cmd: &CloudCommands,
    writer: &mut dyn Write,
    client: &crate::cloud::CloudClient,
    home_dir: &std::path::Path,
) -> Result<(), MigratoryError> {
    match cmd {
        CloudCommands::Auth(auth_cmd) => execute_auth(auth_cmd, writer, client, home_dir)?,
        CloudCommands::Box(box_cmd) => execute_box(box_cmd, writer, client)?,
        CloudCommands::Provider(provider_cmd) => execute_provider(provider_cmd, writer, client)?,
        CloudCommands::Publish(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let version = args.version.as_deref().unwrap_or("");
            let provider = args.provider.as_deref().unwrap_or("");

            writeln!(
                writer,
                "Publishing box {} v{} for provider {}...",
                name, version, provider
            )?;

            client.create_box(
                name,
                args.description.as_deref(),
                args.short_description.as_deref(),
            )?;

            client.create_version(name, version, args.version_description.as_deref())?;

            let default_arch = if args.default_architecture {
                Some(true)
            } else if args.no_default_architecture {
                Some(false)
            } else {
                None
            };

            client.create_provider(
                name,
                version,
                provider,
                args.architecture.as_deref(),
                default_arch,
            )?;

            if args.checksum.is_some() || args.checksum_type.is_some() {
                client.update_provider(
                    name,
                    version,
                    provider,
                    args.checksum.as_deref(),
                    args.checksum_type.as_deref(),
                )?;
            }

            if let Some(file_path) = &args.file_path {
                let upload_url = client.get_upload_url(name, version, provider)?;
                client.upload_file(&upload_url, std::path::Path::new(file_path))?;
            }

            if args.release {
                client.release_version(name, version)?;
            }

            writeln!(writer, "Box published successfully!")?;
        }
        CloudCommands::Search(args) => {
            let query = args.query.as_deref().unwrap_or("");
            writeln!(writer, "Searching for boxes matching '{}'...", query)?;
            let boxes = client.search_boxes(query)?;
            if boxes.is_empty() {
                writeln!(writer, "No boxes found.")?;
            } else {
                for b in boxes {
                    writeln!(writer, "{} - {}", b.name, b.short_description)?;
                }
            }
        }
        CloudCommands::Version(version_cmd) => execute_version(version_cmd, writer, client)?,
    }
    Ok(())
}

/// Executes a `cloud auth` subcommand (`login`, `logout`, `whoami`).
///
/// # Arguments
///
/// * `cmd` - The `CloudAuthCommands` subcommand to execute.
/// * `writer` - Output writer.
/// * `client` - Configured [`crate::cloud::CloudClient`].
/// * `home_dir` - Base directory for Vagrant global state.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if authentication or output writing fails.
fn execute_auth(
    cmd: &CloudAuthCommands,
    writer: &mut dyn Write,
    client: &crate::cloud::CloudClient,
    home_dir: &std::path::Path,
) -> Result<(), MigratoryError> {
    match cmd {
        CloudAuthCommands::Login(args) => {
            let login_args = crate::cli::LoginArgs {
                check: args.check,
                username: args.username.clone(),
                token: args.token.clone(),
                description: args.description.clone(),
            };
            let err_fn = |_: &str| {
                Err(MigratoryError::Generic(
                    "Interactive prompt disabled".to_string(),
                ))
            };
            let client_fn = || Ok(client.clone());
            let _ = client_fn();
            crate::cli::commands::login::execute_with_deps(
                &login_args,
                home_dir,
                err_fn,
                client_fn,
            )?;
            writeln!(writer, "Authentication completed successfully.")?;
        }
        CloudAuthCommands::Logout => {
            client.delete_token()?;
            writeln!(writer, "Logged out successfully.")?;
        }
        CloudAuthCommands::Whoami => {
            let username = client.whoami()?;
            writeln!(writer, "Currently logged in as: {}", username)?;
        }
    };
    Ok(())
}

/// Executes a `cloud box` subcommand (`create`, `delete`, `show`, `update`).
///
/// # Arguments
///
/// * `cmd` - The `CloudBoxCommands` subcommand to execute.
/// * `writer` - Output writer.
/// * `client` - Configured [`crate::cloud::CloudClient`].
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the API call or output write fails.
fn execute_box(
    cmd: &CloudBoxCommands,
    writer: &mut dyn Write,
    client: &crate::cloud::CloudClient,
) -> Result<(), MigratoryError> {
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
            let metadata = client.fetch_metadata(name)?;
            writeln!(writer, "Box: {}", metadata.name)?;
            writeln!(writer, "Description: {}", metadata.short_description)?;
            for v in metadata.versions {
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

/// Executes a `cloud provider` subcommand (`create`, `delete`, `update`, `upload`).
///
/// # Arguments
///
/// * `cmd` - The `CloudProviderCommands` subcommand to execute.
/// * `writer` - Output writer.
/// * `client` - Configured [`crate::cloud::CloudClient`].
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the API call or output write fails.
fn execute_provider(
    cmd: &CloudProviderCommands,
    writer: &mut dyn Write,
    client: &crate::cloud::CloudClient,
) -> Result<(), MigratoryError> {
    match cmd {
        CloudProviderCommands::Create(args) => {
            let name = args.name.as_deref().unwrap_or("");
            let version = args.version.as_deref().unwrap_or("");
            let provider = args.provider.as_deref().unwrap_or("");
            client.create_provider(name, version, provider, None, None)?;
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

/// Executes a `cloud version` subcommand (`create`, `delete`, `release`, `revoke`, `update`).
///
/// # Arguments
///
/// * `cmd` - The `CloudVersionCommands` subcommand to execute.
/// * `writer` - Output writer.
/// * `client` - Configured [`crate::cloud::CloudClient`].
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the API call or output write fails.
fn execute_version(
    cmd: &CloudVersionCommands,
    writer: &mut dyn Write,
    client: &crate::cloud::CloudClient,
) -> Result<(), MigratoryError> {
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

    /// Writer implementation that fails conditionally or unconditionally during tests.
    #[derive(Default)]
    pub(crate) struct FailingWriter {
        /// Optional substring that triggers a write failure when encountered.
        pub fail_on_content: Option<String>,
        /// Accumulated written bytes decoded as UTF-8.
        pub accumulated: String,
    }

    impl Write for FailingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.accumulated.push_str(&String::from_utf8_lossy(buf));
            if let Some(ref needle) = self.fail_on_content {
                if self.accumulated.contains(needle) {
                    return Err(std::io::Error::other("write failed"));
                }
            } else {
                return Err(std::io::Error::other("write failed"));
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Creates default `CloudPublishArgs` for tests.
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

    /// Creates default `CloudSearchArgs` for tests.
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

    pub(crate) fn exec_test(
        server: &MockServer,
        cmd: &CloudCommands,
        out: &mut dyn Write,
    ) -> Result<(), MigratoryError> {
        let client = crate::cloud::CloudClient::with_config(crate::cloud::CloudClientConfig {
            base_url: server.url("/api/v2"),
            token: Some("test-token".to_string()),
        })?;
        let dir = tempfile::tempdir()?;
        execute_with_client(cmd, out, &client, dir.path())
    }

    #[test]
    fn test_execute_cloud_auth() -> Result<(), MigratoryError> {
        let server = MockServer::start();

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
            exec_test(
                &server,
                &CloudCommands::Auth(CloudAuthCommands::Login(crate::cli::CloudAuthLoginArgs {
                    token: Some("mock-token-123".to_string()),
                    username: None,
                    description: None,
                    check: false,
                })),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Authentication completed successfully.");

        let mut out = Vec::new();
        assert!(
            exec_test(
                &server,
                &CloudCommands::Auth(CloudAuthCommands::Logout),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Logged out successfully.");
        mock_delete.assert();

        let mut out = Vec::new();
        assert!(
            exec_test(
                &server,
                &CloudCommands::Auth(CloudAuthCommands::Whoami),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert_eq!(output_str.trim(), "Currently logged in as: testuser");
        mock_whoami.assert();
        Ok(())
    }

    #[test]
    fn test_execute_cloud_auth_write_error() {
        let server = MockServer::start();
        let mut out = FailingWriter::default();
        let result = exec_test(
            &server,
            &CloudCommands::Auth(CloudAuthCommands::Login(crate::cli::CloudAuthLoginArgs {
                token: Some("mock-token-123".to_string()),
                username: None,
                description: None,
                check: false,
            })),
            &mut out,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_cloud_auth_login_check() {
        let server = MockServer::start();
        let mut out = Vec::new();
        let result = exec_test(
            &server,
            &CloudCommands::Auth(CloudAuthCommands::Login(crate::cli::CloudAuthLoginArgs {
                token: None,
                username: None,
                description: None,
                check: true,
            })),
            &mut out,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_cloud_box() {
        let server = MockServer::start();

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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
        let server = MockServer::start();
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
        let result = exec_test(
            &server,
            &CloudCommands::Box(CloudBoxCommands::Create(crate::cli::CloudBoxCreateArgs {
                name: Some("test/box".to_string()),
                ..Default::default()
            })),
            &mut out,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_cloud_provider() -> Result<(), MigratoryError> {
        let server = MockServer::start();

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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
        let mut tf = tempfile::NamedTempFile::new()?;
        tf.write_all(b"test")?;
        let path_str = tf.path().to_string_lossy().to_string();

        let mut out = Vec::new();
        assert!(
            exec_test(
                &server,
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
        Ok(())
    }

    #[test]
    fn test_execute_cloud_provider_write_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });

        let mut out = FailingWriter::default();
        let result = exec_test(
            &server,
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
        let server = MockServer::start();

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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });

        let mut out = FailingWriter::default();
        let result = exec_test(
            &server,
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
        let server = MockServer::start();

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
        assert!(
            exec_test(
                &server,
                &CloudCommands::Search(mock_search_args()),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("Searching for boxes matching 'test'"));
    }

    #[test]
    fn test_execute_cloud_search_empty() {
        let server = MockServer::start();

        server.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/search")
                .query_param("q", "test");
            then.status(200).json_body(serde_json::json!({
                "boxes": []
            }));
        });

        let mut out = Vec::new();
        assert!(
            exec_test(
                &server,
                &CloudCommands::Search(mock_search_args()),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("No boxes found."));
    }

    #[test]
    fn test_execute_cloud_search_write_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/api/v2/search");
            then.status(200).json_body(serde_json::json!({"boxes": []}));
        });
        let mut out = FailingWriter::default();
        let result = exec_test(
            &server,
            &CloudCommands::Search(mock_search_args()),
            &mut out,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_cloud_publish() {
        let server = MockServer::start();

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
        assert!(
            exec_test(
                &server,
                &CloudCommands::Publish(mock_publish_args()),
                &mut out
            )
            .is_ok()
        );
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("Publishing box test/box v1.0.0 for provider virtualbox..."));
    }

    #[test]
    fn test_execute_cloud_publish_with_file() -> Result<(), MigratoryError> {
        let server = MockServer::start();

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
        let mut tf = tempfile::NamedTempFile::new()?;
        tf.write_all(b"test")?;
        args.file_path = Some(tf.path().to_string_lossy().to_string());

        let mut out = Vec::new();
        assert!(exec_test(&server, &CloudCommands::Publish(args), &mut out).is_ok());
        let output_str = String::from_utf8(out).unwrap_or_default();
        assert!(output_str.contains("Box published successfully!"));
        Ok(())
    }

    #[test]
    fn test_execute_cloud_failing_writer_flush() {
        use std::io::Write;
        let mut writer = FailingWriter::default();
        assert!(writer.flush().is_ok());
    }

    #[test]
    fn test_execute_cloud_box_create_error() {
        let server = MockServer::start();

        let mock_create = server.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(401);
        });

        let mut out = Vec::new();
        let res = exec_test(
            &server,
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
        let server = MockServer::start();

        let mock_update = server.mock(|when, then| {
            when.method(PUT).path("/api/v2/boxes/test/box");
            then.status(401);
        });

        let mut out = Vec::new();
        let res = exec_test(
            &server,
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
        let server = MockServer::start();

        let mock_update = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0/providers/virtualbox");
            then.status(401);
        });

        let mut out = Vec::new();
        let res = exec_test(
            &server,
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
        let server = MockServer::start();
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
        let result = exec_test(
            &server,
            &CloudCommands::Publish(mock_publish_args()),
            &mut out,
        );
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod extra_cloud_cmd_coverage_tests {
    use super::*;
    use crate::cli::commands::cloud_cmd::tests::exec_test;
    use httpmock::Method::{DELETE, GET, POST, PUT};
    use httpmock::MockServer;

    #[test]
    fn test_execute_cloud_client_new_error() {
        let mut out = Vec::new();
        let _ = execute(
            &CloudCommands::Publish(crate::cli::CloudPublishArgs::default()),
            &mut out,
        );
    }

    #[test]
    fn test_execute_cloud_publish_errors() -> Result<(), MigratoryError> {
        // 0. create_box fails
        let server0 = MockServer::start();
        server0.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(401);
        });
        let mut out = Vec::new();
        assert!(
            exec_test(
                &server0,
                &CloudCommands::Publish(tests::mock_publish_args()),
                &mut out
            )
            .is_err()
        );

        let server = MockServer::start();

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
            exec_test(
                &server,
                &CloudCommands::Publish(tests::mock_publish_args()),
                &mut out
            )
            .is_err()
        );

        // 2. create_provider fails
        let server2 = MockServer::start();
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
            exec_test(
                &server2,
                &CloudCommands::Publish(tests::mock_publish_args()),
                &mut out
            )
            .is_err()
        );

        // 3. get_upload_url fails with file_path
        let server3 = MockServer::start();
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
        let mut tf = tempfile::NamedTempFile::new()?;
        std::io::Write::write_all(&mut tf, b"test")?;
        let mut args_with_file = tests::mock_publish_args();
        args_with_file.file_path = Some(tf.path().to_string_lossy().to_string());
        let mut out = Vec::new();
        assert!(
            exec_test(
                &server3,
                &CloudCommands::Publish(args_with_file.clone()),
                &mut out
            )
            .is_err()
        );

        // 4. upload_file fails (bad upload_path)
        let server4 = MockServer::start();
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
        assert!(exec_test(&server4, &CloudCommands::Publish(args_with_file), &mut out).is_err());
        Ok(())
    }

    #[test]
    fn test_execute_cloud_search_error() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/api/v2/search");
            then.status(401);
        });
        let mut out = Vec::new();
        assert!(
            exec_test(
                &server,
                &CloudCommands::Search(tests::mock_search_args()),
                &mut out
            )
            .is_err()
        );
    }

    #[test]
    fn test_execute_cloud_auth_errors() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(DELETE).path("/api/v2/authenticate");
            then.status(401);
        });
        server.mock(|when, then| {
            when.method(GET).path("/api/v2/authenticate");
            then.status(401);
        });
        let mut out = Vec::new();
        assert!(
            exec_test(
                &server,
                &CloudCommands::Auth(CloudAuthCommands::Logout),
                &mut out
            )
            .is_err()
        );
        assert!(
            exec_test(
                &server,
                &CloudCommands::Auth(CloudAuthCommands::Whoami),
                &mut out
            )
            .is_err()
        );
    }

    #[test]
    fn test_execute_cloud_box_errors() {
        let server = MockServer::start();
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
            exec_test(
                &server,
                &CloudCommands::Box(CloudBoxCommands::Delete(crate::cli::CloudBoxDeleteArgs {
                    name: Some("test/box".to_string()),
                    ..Default::default()
                })),
                &mut out
            )
            .is_err()
        );
        assert!(
            exec_test(
                &server,
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
    fn test_execute_cloud_provider_errors() -> Result<(), MigratoryError> {
        let server = MockServer::start();
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
        let mut tf = tempfile::NamedTempFile::new()?;
        std::io::Write::write_all(&mut tf, b"test")?;
        assert!(
            exec_test(
                &server,
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
        server_up.mock(|when, then| {
            when.method(GET)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox/upload");
            then.status(200).json_body(serde_json::json!({
                "upload_path": "http://127.0.0.1:1/nonexistent"
            }));
        });
        assert!(
            exec_test(
                &server_up,
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
        Ok(())
    }

    #[test]
    fn test_execute_cloud_version_errors() {
        let server = MockServer::start();
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
            exec_test(
                &server,
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
    fn test_execute_cloud_all_writeln_failures() -> Result<(), MigratoryError> {
        // Publish: fail on final writeln
        let s_pub = MockServer::start();
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
            exec_test(
                &s_pub,
                &CloudCommands::Publish(tests::mock_publish_args()),
                &mut out
            )
            .is_err()
        );

        // Search empty: fail on "No boxes found."
        let s_search_empty = MockServer::start();
        s_search_empty.mock(|when, then| {
            when.method(GET).path("/api/v2/search");
            then.status(200)
                .json_body(serde_json::json!({ "boxes": [] }));
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("No boxes found.".to_string()),
            ..Default::default()
        };
        assert!(
            exec_test(
                &s_search_empty,
                &CloudCommands::Search(tests::mock_search_args()),
                &mut out
            )
            .is_err()
        );

        // Search with box: fail on printing box
        let s_search = MockServer::start();
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
        assert!(
            exec_test(
                &s_search,
                &CloudCommands::Search(tests::mock_search_args()),
                &mut out
            )
            .is_err()
        );

        // Auth logout: fail on "Logged out successfully."
        let s_logout = MockServer::start();
        s_logout.mock(|when, then| {
            when.method(DELETE).path("/api/v2/authenticate");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Logged out successfully.".to_string()),
            ..Default::default()
        };
        assert!(
            exec_test(
                &s_logout,
                &CloudCommands::Auth(CloudAuthCommands::Logout),
                &mut out
            )
            .is_err()
        );

        // Auth whoami: fail on "Currently logged in as..."
        let s_whoami = MockServer::start();
        s_whoami.mock(|when, then| {
            when.method(GET).path("/api/v2/authenticate");
            then.status(200)
                .json_body(serde_json::json!({ "user": { "username": "u" } }));
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Currently logged in as:".to_string()),
            ..Default::default()
        };
        assert!(
            exec_test(
                &s_whoami,
                &CloudCommands::Auth(CloudAuthCommands::Whoami),
                &mut out
            )
            .is_err()
        );

        // Box delete: fail on "Box deleted successfully."
        let s_bdel = MockServer::start();
        s_bdel.mock(|when, then| {
            when.method(DELETE).path("/api/v2/boxes/test/box");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Box deleted successfully.".to_string()),
            ..Default::default()
        };
        assert!(
            exec_test(
                &s_bdel,
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
                exec_test(
                    &s_bshow,
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
        s_bup.mock(|when, then| {
            when.method(PUT).path("/api/v2/boxes/test/box");
            then.status(200);
        });
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Box updated successfully.".to_string()),
            ..Default::default()
        };
        assert!(
            exec_test(
                &s_bup,
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
            exec_test(
                &s_pdel,
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
            exec_test(
                &s_pup,
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
            exec_test(
                &s_pupload_nofile,
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
        let mut tf = tempfile::NamedTempFile::new()?;
        std::io::Write::write_all(&mut tf, b"test")?;
        let mut out = tests::FailingWriter {
            fail_on_content: Some("Provider upload successful.".to_string()),
            ..Default::default()
        };
        assert!(
            exec_test(
                &s_pupload_file,
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
            exec_test(
                &s_vdel,
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
            exec_test(
                &s_vrel,
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
            exec_test(
                &s_vrev,
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
            exec_test(
                &s_vup,
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
        Ok(())
    }

    #[test]
    fn test_execute_cloud_publish_full_features() {
        let server = MockServer::start();

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
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox");
            then.status(200);
        });
        server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/release");
            then.status(200);
        });

        let mut args = tests::mock_publish_args();
        args.architecture = Some("arm64".to_string());
        args.default_architecture = true;
        args.checksum = Some("sha256sum".to_string());
        args.checksum_type = Some("sha256".to_string());
        args.release = true;

        let mut out = Vec::new();
        assert!(exec_test(&server, &CloudCommands::Publish(args), &mut out).is_ok());

        // Test with no_default_architecture
        let mut args_no_def = tests::mock_publish_args();
        args_no_def.no_default_architecture = true;
        let mut out2 = Vec::new();
        assert!(exec_test(&server, &CloudCommands::Publish(args_no_def), &mut out2).is_ok());
        let mut args_checksum_only = tests::mock_publish_args();
        args_checksum_only.checksum = Some("sha256sum".to_string());
        let mut args_type_only = tests::mock_publish_args();
        args_type_only.checksum_type = Some("sha256".to_string());
        let mut out_type = Vec::new();
        assert!(
            exec_test(
                &server,
                &CloudCommands::Publish(args_type_only),
                &mut out_type
            )
            .is_ok()
        );
        let mut out_cs = Vec::new();
        assert!(
            exec_test(
                &server,
                &CloudCommands::Publish(args_checksum_only),
                &mut out_cs
            )
            .is_ok()
        );

        // Test update_provider failure
        let server_err = MockServer::start();
        server_err.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });
        server_err.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });
        server_err.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });
        server_err.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers/virtualbox");
            then.status(500);
        });

        let mut args_err = tests::mock_publish_args();
        args_err.checksum = Some("sum".to_string());
        let mut out3 = Vec::new();
        assert!(exec_test(&server_err, &CloudCommands::Publish(args_err), &mut out3).is_err());

        // Test release_version failure
        let server_rel_err = MockServer::start();
        server_rel_err.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes");
            then.status(200);
        });
        server_rel_err.mock(|when, then| {
            when.method(POST).path("/api/v2/boxes/test/box/versions");
            then.status(200);
        });
        server_rel_err.mock(|when, then| {
            when.method(POST)
                .path("/api/v2/boxes/test/box/versions/1.0.0/providers");
            then.status(200);
        });
        server_rel_err.mock(|when, then| {
            when.method(PUT)
                .path("/api/v2/boxes/test/box/versions/1.0.0/release");
            then.status(500);
        });

        let mut args_rel_err = tests::mock_publish_args();
        args_rel_err.release = true;
        let mut out4 = Vec::new();
        assert!(
            exec_test(
                &server_rel_err,
                &CloudCommands::Publish(args_rel_err),
                &mut out4
            )
            .is_err()
        );

        let login_args = crate::cli::CloudAuthLoginArgs {
            check: false,
            username: Some("u".to_string()),
            token: Some("tok".to_string()),
            description: None,
        };
        let mut out5 = Vec::new();
        assert!(
            exec_test(
                &server,
                &CloudCommands::Auth(CloudAuthCommands::Login(login_args)),
                &mut out5
            )
            .is_ok()
        );
        let login_args_empty = crate::cli::CloudAuthLoginArgs {
            check: false,
            username: None,
            token: None,
            description: None,
        };
        assert!(
            exec_test(
                &server,
                &CloudCommands::Auth(CloudAuthCommands::Login(login_args_empty)),
                &mut Vec::new()
            )
            .is_err()
        );
    }
}
