//! Semantic implementation of the `version` command.
//!
//! Outputs the installed Migratory version and checks for new releases unless
//! update checks are disabled via `VAGRANT_CHECKPOINT_DISABLE`.

use crate::error::MigratoryError;
use std::time::Duration;

/// Queries a specific release endpoint URL to check for the latest version.
///
/// # Arguments
///
/// * `url` - Endpoint URL returning a Checkpoint or GitHub release JSON payload.
///
/// # Returns
///
/// Returns `Some(version_string)` if retrieved, or `None` on failure.
fn fetch_latest_version_from(url: &str) -> Option<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok()?;

    let response = client
        .get(url)
        .header("User-Agent", "Migratory")
        .send()
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    /// Deserialized response payload from the HashiCorp Checkpoint version API.
    #[derive(serde::Deserialize)]
    struct CheckpointResponse {
        /// Latest version reported by HashiCorp Checkpoint.
        current_version: Option<String>,
        /// Optional release tag name fallback (e.g. from GitHub Releases API).
        tag_name: Option<String>,
    }

    let parsed: CheckpointResponse = response.json().ok()?;
    parsed.current_version.or(parsed.tag_name)
}

/// The HashiCorp Vagrant compatibility version reported by Migratory.
pub const VAGRANT_COMPAT_VERSION: &str = "2.4.3";

/// Returns the effective Vagrant compatibility version string.
///
/// If the `VAGRANT_VERSION` environment variable is set, its value is returned.
/// Otherwise, defaults to [`VAGRANT_COMPAT_VERSION`].
///
/// # Returns
///
/// Returns a `String` containing the compatibility version.
pub fn compat_version() -> String {
    compat_version_from(std::env::var("VAGRANT_VERSION").ok().as_deref())
}

/// Resolves the compatibility version from an optional override string.
///
/// # Arguments
///
/// * `override_ver` - Optional version override string.
///
/// # Returns
///
/// Returns the resolved version string.
pub fn compat_version_from(override_ver: Option<&str>) -> String {
    override_ver.unwrap_or(VAGRANT_COMPAT_VERSION).to_string()
}

/// Executes the `version` command, printing current and latest version information.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if output printing fails.
pub fn execute() -> Result<(), MigratoryError> {
    let current = compat_version();
    let checkpoint_disabled = cfg!(test) || std::env::var("VAGRANT_CHECKPOINT_DISABLE").is_ok();
    let url = std::env::var("MIGRATORY_CHECKPOINT_URL")
        .unwrap_or_else(|_| "https://checkpoint-api.hashicorp.com/v1/check/vagrant".to_string());
    execute_with_config(
        &current,
        crate::ui::is_machine_readable(),
        checkpoint_disabled,
        &url,
    )
}

/// Executes the `version` command with explicit configuration parameters.
///
/// # Arguments
///
/// * `current` - Current installed compatibility version string.
/// * `is_machine_readable` - Whether machine-readable CSV output is enabled.
/// * `checkpoint_disabled` - Whether remote checkpoint update checks are disabled.
/// * `checkpoint_url` - Endpoint URL for remote version checks.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if output printing fails.
pub fn execute_with_config(
    current: &str,
    is_machine_readable: bool,
    checkpoint_disabled: bool,
    checkpoint_url: &str,
) -> Result<(), MigratoryError> {
    if is_machine_readable {
        let ui = crate::ui::MachineReadableUi;
        ui.log_csv("", "version-installed", &[current]);
        ui.log_csv("", "version-latest", &[current]);
        return Ok(());
    }

    println!("Installed Version: {}", current);

    if checkpoint_disabled {
        println!("Version check disabled.");
    } else {
        let latest = fetch_latest_version_from(checkpoint_url);
        if let Some(v) = latest {
            println!("Latest Version: {}", v);
            if v == current {
                println!("You're running an up-to-date version of Vagrant!");
            } else {
                println!(
                    "An update is available! You're running version {}, latest is {}.",
                    current, v
                );
            }
        } else {
            println!("Latest Version: {}", current);
            println!("You're running an up-to-date version of Vagrant!");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[test]
    fn test_execute_version() {
        let result = execute();
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_version_checkpoint_disabled() {
        let result = execute_with_config(VAGRANT_COMPAT_VERSION, false, true, "");
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_version_up_to_date() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/check");
            then.status(200).json_body(serde_json::json!({
                "current_version": VAGRANT_COMPAT_VERSION
            }));
        });

        let result =
            execute_with_config(VAGRANT_COMPAT_VERSION, false, false, &server.url("/check"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_version_outdated() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/check");
            then.status(200).json_body(serde_json::json!({
                "current_version": "99.0.0"
            }));
        });

        let result =
            execute_with_config(VAGRANT_COMPAT_VERSION, false, false, &server.url("/check"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_version_network_error() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/check");
            then.status(500);
        });

        let result =
            execute_with_config(VAGRANT_COMPAT_VERSION, false, false, &server.url("/check"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_fetch_latest_version_deterministic() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/latest");
            then.status(200).json_body(serde_json::json!({
                "current_version": "2.4.9"
            }));
        });

        assert_eq!(
            fetch_latest_version_from(&server.url("/latest")),
            Some("2.4.9".to_string())
        );
        assert_eq!(
            fetch_latest_version_from("http://127.0.0.1:1/unreachable"),
            None
        );
    }

    #[test]
    fn test_execute_version_tag_name() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/check");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"tag_name": "v9.9.9"}"#);
        });

        let result =
            execute_with_config(VAGRANT_COMPAT_VERSION, false, false, &server.url("/check"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_compat_version_env_and_default() {
        assert_eq!(compat_version_from(None), VAGRANT_COMPAT_VERSION);
        assert_eq!(compat_version_from(Some("2.4.1")), "2.4.1");
        assert!(!compat_version().is_empty());
    }

    #[test]
    fn test_execute_version_machine_readable() {
        let result = execute_with_config(VAGRANT_COMPAT_VERSION, true, true, "");
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_version_update_available() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/check");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"current_version": "99.0.0"}"#);
        });

        let result =
            execute_with_config(VAGRANT_COMPAT_VERSION, false, false, &server.url("/check"));
        assert!(result.is_ok());
    }
}
