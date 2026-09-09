//! Semantic implementation of the `login` command.
//!
//! This module provides the logic to authenticate with Vagrant Cloud.

use crate::cli::LoginArgs;
use crate::cloud::CloudClient;
use crate::error::MigratoryError;
use std::io::Write;

/// Executes the `login` command.
///
/// # Arguments
///
/// * `args` - The parsed arguments for the `login` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if authentication fails or network issues occur.
pub fn execute(args: &LoginArgs) -> Result<(), MigratoryError> {
    let home_dir = std::env::var("VAGRANT_HOME").unwrap_or_else(|_| ".vagrant.d".to_string());
    let token_path = std::path::Path::new(&home_dir).join("data").join("token");

    if args.check {
        if token_path.exists() {
            println!("You are already logged in.");
        } else {
            println!("You are not currently logged in.");
        }
        return Ok(());
    }

    let token = if let Some(t) = &args.token {
        t.clone()
    } else {
        println!("In a moment we will ask for your username and password to HashiCorp's");
        println!("Vagrant Cloud. This credential will be used to authenticate you with");
        println!("the Vagrant Cloud for things like box downloads.");

        let username = match args.username.as_deref() {
            Some(u) if !u.is_empty() => u.to_string(),
            _ => prompt_line("Vagrant Cloud username or email: ")?,
        };

        let password = prompt_line("Password (will be visible): ")?;

        let client = CloudClient::new()?;
        let description = args
            .description
            .as_deref()
            .unwrap_or("Migratory login from CLI");
        client.authenticate(&username, &password, Some(description))?
    };

    println!("Saving token...");
    let parent = token_path.parent().unwrap_or(&token_path);
    std::fs::create_dir_all(parent).map_err(MigratoryError::Io)?;
    std::fs::write(&token_path, token).map_err(MigratoryError::Io)?;
    println!("Valid token saved.");

    Ok(())
}

/// Prompts the user on stdout and reads a line from stdin.
///
/// # Arguments
///
/// * `prompt` - The text to display to the user.
///
/// # Errors
///
/// Returns a `MigratoryError` if reading from stdin fails.
#[coverage(off)]
fn prompt_line(prompt: &str) -> Result<String, MigratoryError> {
    print!("{}", prompt);
    let _ = std::io::stdout().flush();

    if std::env::var("MIGRATORY_TEST_MOCK_STDIN_ERROR").is_ok() {
        return Err(MigratoryError::Io(std::io::Error::other(
            "mock stdin error",
        )));
    }
    if std::env::var("MIGRATORY_TEST_MOCK_STDIN").is_ok() {
        return Ok("mock_input".to_string());
    }

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(MigratoryError::Io)?;
    Ok(input.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_login_no_env() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
        let args = LoginArgs {
            check: true,
            username: None,
            token: None,
            description: None,
        };
        assert!(execute(&args).is_ok());
    }

    #[test]
    fn test_execute_login_write_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let temp = tempfile::tempdir().expect("operation should succeed");
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                temp.path().to_str().expect("operation should succeed"),
            );
        }

        // Create a directory at the token path so writing the file fails
        let token_path = temp.path().join("data").join("token");
        std::fs::create_dir_all(&token_path).expect("operation should succeed");

        let args = LoginArgs {
            check: false,
            username: None,
            token: Some("my-test-token".to_string()),
            description: None,
        };

        let result = execute(&args);
        assert!(result.is_err());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_execute_login_io_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let temp = tempfile::tempdir().expect("operation should succeed");
        // Use a file as VAGRANT_HOME to cause create_dir_all to fail
        let fake_home = temp.path().join("fake_home");
        std::fs::write(&fake_home, "dummy").expect("operation should succeed");

        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                fake_home.to_str().expect("operation should succeed"),
            );
        }

        let args = LoginArgs {
            check: false,
            username: None,
            token: Some("my-test-token".to_string()),
            description: None,
        };

        let result = execute(&args);
        assert!(result.is_err());

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
    }

    #[test]
    fn test_execute_login() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let temp = tempfile::tempdir().expect("operation should succeed");
        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                temp.path().to_str().expect("operation should succeed"),
            );
        }

        // Test check when not logged in
        let args_check_not_logged = LoginArgs {
            check: true,
            username: None,
            token: None,
            description: None,
        };
        assert!(execute(&args_check_not_logged).is_ok());

        // Test login with token
        let args_token = LoginArgs {
            check: false,
            username: None,
            token: Some("my-test-token".to_string()),
            description: None,
        };
        assert!(execute(&args_token).is_ok());

        // Test check when logged in
        let args_check_logged = LoginArgs {
            check: true,
            username: None,
            token: None,
            description: None,
        };
        assert!(execute(&args_check_logged).is_ok());
    }

    #[test]
    fn test_execute_login_interactive() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let temp = tempfile::tempdir().expect("operation should succeed");
        let server = httpmock::MockServer::start();

        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                temp.path().to_str().expect("operation should succeed"),
            );
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v1"));
            std::env::set_var("MIGRATORY_TEST_MOCK_STDIN", "1");
        }

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::POST);
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"token": "mock-token-from-api"}"#);
        });

        // We pass username so it skips asking for username.
        // It will still ask for password, reading EOF, resulting in empty string.
        let args = LoginArgs {
            check: false,
            username: Some("testuser".to_string()),
            token: None,
            description: None,
        };

        let result = execute(&args);
        assert!(result.is_ok(), "{:?}", result);

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("MIGRATORY_TEST_MOCK_STDIN");
        }
    }

    #[test]
    fn test_execute_login_interactive_read_line_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let args = LoginArgs {
            check: false,
            username: None,
            token: None,
            description: None,
        };

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_STDIN");
            std::env::set_var("MIGRATORY_TEST_MOCK_STDIN_ERROR", "1");
        }

        let result = execute(&args);
        assert!(result.is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_STDIN_ERROR");
        }
    }

    #[test]
    fn test_execute_login_interactive_read_line_password_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let args = LoginArgs {
            check: false,
            username: Some("test_user".to_string()),
            token: None,
            description: None,
        };

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_STDIN");
            std::env::set_var("MIGRATORY_TEST_MOCK_STDIN_ERROR", "1");
        }

        let result = execute(&args);
        assert!(result.is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_STDIN_ERROR");
        }
    }

    #[test]
    fn test_execute_login_interactive_empty_username() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let temp = tempfile::tempdir().expect("operation should succeed");
        let server = httpmock::MockServer::start();

        unsafe {
            std::env::set_var(
                "VAGRANT_HOME",
                temp.path().to_str().expect("operation should succeed"),
            );
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v1"));
            std::env::set_var("MIGRATORY_TEST_MOCK_STDIN", "1");
        }

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::POST);
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"token": "mock-token-from-api"}"#);
        });

        // Username is Some(""), so it will ask for it via stdin.
        let args = LoginArgs {
            check: false,
            username: Some("".to_string()),
            token: None,
            description: None,
        };

        let result = execute(&args);
        assert!(result.is_ok(), "{:?}", result);

        unsafe {
            std::env::remove_var("VAGRANT_HOME");
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("MIGRATORY_TEST_MOCK_STDIN");
        }
    }

    #[test]
    fn test_execute_login_client_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_CLIENT_ERROR", "1");
            std::env::set_var("MIGRATORY_TEST_MOCK_STDIN", "1");
        }

        let args = LoginArgs {
            check: false,
            username: Some("user".to_string()),
            token: None,
            description: None,
        };

        let result = execute(&args);
        assert!(result.is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_CLIENT_ERROR");
            std::env::remove_var("MIGRATORY_TEST_MOCK_STDIN");
        }
    }

    #[test]
    fn test_execute_login_auth_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let server = httpmock::MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::POST);
            then.status(401);
        });

        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", server.url("/api/v1"));
            std::env::set_var("MIGRATORY_TEST_MOCK_STDIN", "1");
        }

        let args = LoginArgs {
            check: false,
            username: Some("user".to_string()),
            token: None,
            description: None,
        };

        let result = execute(&args);
        assert!(result.is_err());

        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
            std::env::remove_var("MIGRATORY_TEST_MOCK_STDIN");
        }
    }
}
