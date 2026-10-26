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
    execute_with_deps(
        args,
        std::path::Path::new(&home_dir),
        prompt_line,
        CloudClient::new,
    )
}

/// Executes the `login` command with injectable dependencies.
///
/// # Arguments
///
/// * `args` - The parsed arguments for the `login` command.
/// * `home_dir` - Base directory for Vagrant global state.
/// * `prompter` - Function to prompt the user and read a line of input.
/// * `cloud_factory` - Factory function creating a [`CloudClient`].
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if authentication fails or network issues occur.
pub fn execute_with_deps<P, C>(
    args: &LoginArgs,
    home_dir: &std::path::Path,
    mut prompter: P,
    mut cloud_factory: C,
) -> Result<(), MigratoryError>
where
    P: FnMut(&str) -> Result<String, MigratoryError>,
    C: FnMut() -> Result<CloudClient, MigratoryError>,
{
    execute_with_deps_dyn(args, home_dir, &mut prompter, &mut cloud_factory)
}

/// Non-generic implementation of `login` execution with dynamic dependency callbacks.
fn execute_with_deps_dyn(
    args: &LoginArgs,
    home_dir: &std::path::Path,
    prompter: &mut dyn FnMut(&str) -> Result<String, MigratoryError>,
    cloud_factory: &mut dyn FnMut() -> Result<CloudClient, MigratoryError>,
) -> Result<(), MigratoryError> {
    let token_path = home_dir.join("data").join("token");

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
            _ => prompter("Vagrant Cloud username or email: ")?,
        };

        let password = prompter("Password (will be visible): ")?;

        let client = cloud_factory()?;
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
fn prompt_line(prompt: &str) -> Result<String, MigratoryError> {
    prompt_line_from(prompt, &mut std::io::Cursor::new(b"\n"))
}

/// Prompts the user on stdout and reads a line from the provided buffered reader.
///
/// # Arguments
///
/// * `prompt` - The text to display to the user.
/// * `reader` - Buffered reader to read user input from.
///
/// # Errors
///
/// Returns a `MigratoryError` if reading from `reader` fails.
fn prompt_line_from(
    prompt: &str,
    reader: &mut dyn std::io::BufRead,
) -> Result<String, MigratoryError> {
    print!("{}", prompt);
    let _ = std::io::stdout().flush();

    let mut input = String::new();
    reader.read_line(&mut input).map_err(MigratoryError::Io)?;
    Ok(input.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_line_from() -> Result<(), MigratoryError> {
        let mut cursor = std::io::Cursor::new(b"my_input\n");
        let res = prompt_line_from("Prompt: ", &mut cursor)?;
        assert_eq!(res, "my_input");
        assert_eq!(prompt_line("Prompt: ")?, "");
        Ok(())
    }

    #[test]
    fn test_execute_login_no_env() {
        let args = LoginArgs {
            check: true,
            username: None,
            token: None,
            description: None,
        };
        assert!(execute(&args).is_ok());
    }

    #[test]
    fn test_execute_login_write_error() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;

        // Create a directory at the token path so writing the file fails
        let token_path = temp.path().join("data").join("token");
        std::fs::create_dir_all(&token_path)?;

        let args = LoginArgs {
            check: false,
            username: None,
            token: Some("my-test-token".to_string()),
            description: None,
        };

        let result = execute_with_deps(&args, temp.path(), prompt_line, CloudClient::new);
        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_execute_login_io_error() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        // Use a file as VAGRANT_HOME to cause create_dir_all to fail
        let fake_home = temp.path().join("fake_home");
        std::fs::write(&fake_home, "dummy")?;

        let args = LoginArgs {
            check: false,
            username: None,
            token: Some("my-test-token".to_string()),
            description: None,
        };

        let result = execute_with_deps(&args, &fake_home, prompt_line, CloudClient::new);
        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn test_execute_login() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;

        // Test check when not logged in
        let args_check_not_logged = LoginArgs {
            check: true,
            username: None,
            token: None,
            description: None,
        };
        assert!(
            execute_with_deps(
                &args_check_not_logged,
                temp.path(),
                prompt_line,
                CloudClient::new
            )
            .is_ok()
        );

        // Test login with token
        let args_token = LoginArgs {
            check: false,
            username: None,
            token: Some("my-test-token".to_string()),
            description: None,
        };
        assert!(execute_with_deps(&args_token, temp.path(), prompt_line, CloudClient::new).is_ok());

        // Test check when logged in
        let args_check_logged = LoginArgs {
            check: true,
            username: None,
            token: None,
            description: None,
        };
        assert!(
            execute_with_deps(
                &args_check_logged,
                temp.path(),
                prompt_line,
                CloudClient::new
            )
            .is_ok()
        );
        Ok(())
    }

    #[test]
    fn test_execute_login_interactive() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let server = httpmock::MockServer::start();

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::POST);
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"token": "mock-token-from-api"}"#);
        });

        let args = LoginArgs {
            check: false,
            username: Some("testuser".to_string()),
            token: None,
            description: None,
        };

        let url = server.url("/api/v1");
        let result = execute_with_deps(
            &args,
            temp.path(),
            |_| Ok("mock_input".to_string()),
            || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            },
        );
        assert!(result.is_ok(), "{:?}", result);

        Ok(())
    }

    #[test]
    fn test_execute_login_interactive_read_line_error() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let args = LoginArgs {
            check: false,
            username: None,
            token: None,
            description: None,
        };

        let result = execute_with_deps(
            &args,
            temp.path(),
            |_| {
                Err(MigratoryError::Io(std::io::Error::other(
                    "mock stdin error",
                )))
            },
            CloudClient::new,
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_login_interactive_read_line_password_error() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let args = LoginArgs {
            check: false,
            username: Some("test_user".to_string()),
            token: None,
            description: None,
        };

        let result = execute_with_deps(
            &args,
            temp.path(),
            |_| {
                Err(MigratoryError::Io(std::io::Error::other(
                    "mock stdin error",
                )))
            },
            CloudClient::new,
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_login_interactive_empty_username() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let server = httpmock::MockServer::start();

        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::POST);
            then.status(200)
                .header("content-type", "application/json")
                .body(r#"{"token": "mock-token-from-api"}"#);
        });

        let args = LoginArgs {
            check: false,
            username: Some(String::new()),
            token: None,
            description: None,
        };

        let url = server.url("/api/v1");
        let result = execute_with_deps(
            &args,
            temp.path(),
            |_| Ok("mock_input".to_string()),
            || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            },
        );
        assert!(result.is_ok(), "{:?}", result);

        Ok(())
    }

    #[test]
    fn test_execute_login_client_error() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let args = LoginArgs {
            check: false,
            username: Some("user".to_string()),
            token: None,
            description: None,
        };

        let result = execute_with_deps(
            &args,
            temp.path(),
            |_| Ok("mock_input".to_string()),
            || Err(MigratoryError::Generic("client error".to_string())),
        );
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_login_auth_failure() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let server = httpmock::MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(httpmock::Method::POST);
            then.status(401);
        });

        let args = LoginArgs {
            check: false,
            username: Some("user".to_string()),
            token: None,
            description: None,
        };

        let url = server.url("/api/v1");
        let result = execute_with_deps(
            &args,
            temp.path(),
            |_| Ok("mock_input".to_string()),
            || {
                CloudClient::with_config(crate::cloud::CloudClientConfig {
                    base_url: url.clone(),
                    token: None,
                })
            },
        );
        assert!(result.is_err());
        Ok(())
    }
}
