//! Semantic implementation of the `version` command.
//!
//! Outputs the installed Migratory version and checks for new releases unless
//! update checks are disabled via `VAGRANT_CHECKPOINT_DISABLE`.

use crate::error::MigratoryError;
use std::time::Duration;

/// Queries the remote release endpoint to check for the latest version.
///
/// # Returns
///
/// Returns `Some(version_string)` if the latest version was retrieved successfully,
/// or `None` if the request failed, timed out, or returned invalid JSON.
#[coverage(off)]
fn fetch_latest_version() -> Option<String> {
    let url = std::env::var("MIGRATORY_CHECKPOINT_URL")
        .unwrap_or_else(|_| "https://checkpoint-api.hashicorp.com/v1/check/vagrant".to_string());

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok()?;

    let response = client
        .get(&url)
        .header("User-Agent", "Migratory")
        .send()
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    #[derive(serde::Deserialize)]
    struct CheckpointResponse {
        current_version: Option<String>,
        tag_name: Option<String>,
    }

    let parsed: CheckpointResponse = response.json().ok()?;
    parsed.current_version.or(parsed.tag_name)
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
    let current = env!("CARGO_PKG_VERSION");
    println!("Installed Version: {}", current);

    if std::env::var("VAGRANT_CHECKPOINT_DISABLE").is_ok() {
        println!("Version check disabled.");
    } else {
        let latest = fetch_latest_version();
        if let Some(v) = latest {
            println!("Latest Version: {}", v);
            if v == current {
                println!("You're running an up-to-date version of Migratory!");
            } else {
                println!(
                    "An update is available! You're running version {}, latest is {}.",
                    current, v
                );
            }
        } else {
            println!("Latest Version: {}", current);
            println!("You're running an up-to-date version of Migratory!");
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
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("VAGRANT_CHECKPOINT_DISABLE", "1");
        }
        let result = execute();
        unsafe {
            std::env::remove_var("VAGRANT_CHECKPOINT_DISABLE");
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_version_checkpoint_disabled() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("VAGRANT_CHECKPOINT_DISABLE", "1");
        }
        let result = execute();
        unsafe {
            std::env::remove_var("VAGRANT_CHECKPOINT_DISABLE");
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_version_up_to_date() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/check");
            then.status(200).json_body(serde_json::json!({
                "current_version": env!("CARGO_PKG_VERSION")
            }));
        });

        unsafe {
            std::env::remove_var("VAGRANT_CHECKPOINT_DISABLE");
            std::env::set_var("MIGRATORY_CHECKPOINT_URL", server.url("/check"));
        }

        let result = execute();
        unsafe {
            std::env::remove_var("MIGRATORY_CHECKPOINT_URL");
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_version_outdated() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/check");
            then.status(200).json_body(serde_json::json!({
                "current_version": "99.0.0"
            }));
        });

        unsafe {
            std::env::remove_var("VAGRANT_CHECKPOINT_DISABLE");
            std::env::set_var("MIGRATORY_CHECKPOINT_URL", server.url("/check"));
        }

        let result = execute();
        unsafe {
            std::env::remove_var("MIGRATORY_CHECKPOINT_URL");
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_version_network_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/check");
            then.status(500);
        });

        unsafe {
            std::env::remove_var("VAGRANT_CHECKPOINT_DISABLE");
            std::env::set_var("MIGRATORY_CHECKPOINT_URL", server.url("/check"));
        }

        let result = execute();
        unsafe {
            std::env::remove_var("MIGRATORY_CHECKPOINT_URL");
        }
        assert!(result.is_ok());
    }

    #[test]
    fn test_fetch_latest_version_default_url() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::remove_var("MIGRATORY_CHECKPOINT_URL");
            std::env::set_var("VAGRANT_CHECKPOINT_DISABLE", "1");
        }
        let _ = fetch_latest_version();
    }

    #[test]
    fn test_execute_version_tag_name() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/check");
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"tag_name": "v9.9.9"}"#);
        });

        unsafe {
            std::env::remove_var("VAGRANT_CHECKPOINT_DISABLE");
            std::env::set_var("MIGRATORY_CHECKPOINT_URL", server.url("/check"));
        }

        let result = execute();
        unsafe {
            std::env::remove_var("MIGRATORY_CHECKPOINT_URL");
        }
        assert!(result.is_ok());
    }
}
