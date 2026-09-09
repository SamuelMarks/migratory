//! Semantic implementation of the `init` command.
//!
//! This module provides the logic to initialize a new Vagrant environment
//! by creating a default Vagrantfile.

use crate::cli::InitArgs;
use crate::error::MigratoryError;
use std::fs;
use std::path::Path;

/// The default basic Vagrantfile content.
pub const VAGRANTFILE_TEMPLATE: &str = r#"# -*- mode: ruby -*-
# vi: set ft=ruby :

Vagrant.configure("2") do |config|
  config.vm.box = "base"
end
"#;

/// Executes the `init` command, creating a basic Vagrantfile in the given directory.
///
/// # Arguments
///
/// * `cwd` - The path to the directory where the Vagrantfile should be created.
/// * `args` - The parsed arguments for the `init` command.
///
/// # Returns
///
/// Returns `Ok(())` on success, indicating the file was created.
///
/// # Errors
///
/// Returns a `MigratoryError` if the `Vagrantfile` already exists or if there's an I/O error during writing.
pub fn execute(cwd: &Path, args: &InitArgs) -> Result<(), MigratoryError> {
    let output_path = args.output.as_deref().unwrap_or("Vagrantfile");
    let path = cwd.join(output_path);

    if path.exists() && !args.force {
        return Err(MigratoryError::AlreadyExists("Vagrantfile".to_string()));
    }

    let box_name = args.box_name.as_deref().unwrap_or("base");
    let template = if let Some(tmpl_path) = &args.template {
        let tmpl_file = Path::new(tmpl_path);
        if !tmpl_file.exists() {
            return Err(MigratoryError::NotFound(tmpl_path.clone()));
        }
        let raw = fs::read_to_string(tmpl_file).map_err(MigratoryError::Io)?;
        raw.replace("{{box_name}}", box_name)
    } else {
        let mut template = format!(
            "# -*- mode: ruby -*-\n# vi: set ft=ruby :\n\nVagrant.configure(\"2\") do |config|\n  config.vm.box = \"{}\"\n",
            box_name
        );

        if let Some(version) = &args.box_version {
            template.push_str(&format!("  config.vm.box_version = \"{}\"\n", version));
        }

        template.push_str("end\n");
        template
    };

    fs::write(&path, template).map_err(MigratoryError::Io)?;
    println!(
        "A `Vagrantfile` has been placed in this directory. You are now\nready to `migratory up` your first virtual environment!"
    );
    Ok(())
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn mock_args() -> InitArgs {
        InitArgs {
            box_name: None,
            output: None,
            box_version: None,
            force: false,
            minimal: false,
            template: None,
        }
    }

    #[test]
    fn test_execute_init() {
        // Test with box_version and box_name
        let dir = tempfile::tempdir().expect("operation should succeed");
        let path = dir.path().join("Vagrantfile");
        let args2 = crate::cli::InitArgs {
            box_name: Some("test/box".to_string()),
            box_version: Some("1.2.3".to_string()),
            force: false,
            minimal: false,
            output: Some(path.to_str().expect("operation should succeed").to_string()),
            template: None,
        };
        assert!(execute(&dir.path(), &args2).is_ok());
        let content = std::fs::read_to_string(&path).expect("operation should succeed");
        assert!(content.contains("config.vm.box_version = \"1.2.3\""));
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let mut args = mock_args();

        // First init should succeed
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        assert!(cwd.join("Vagrantfile").exists());

        let content =
            fs::read_to_string(cwd.join("Vagrantfile")).expect("operation should succeed");
        assert_eq!(content, VAGRANTFILE_TEMPLATE);

        // Second init should fail with AlreadyExists
        let result2 = execute(cwd, &args);
        assert!(matches!(result2, Err(MigratoryError::AlreadyExists(_))));

        // Third init should succeed with force
        args.force = true;
        let result3 = execute(cwd, &args);
        assert!(result3.is_ok());
    }

    #[test]
    fn test_execute_init_io_error() {
        let dir = tempdir().expect("operation should succeed");

        // Create a directory named `Vagrantfile` to simulate an IO error when attempting to write to it.
        // On Unix, writing to a directory as a file throws an error.
        // Oh wait, `path.exists()` would return true and we'll get an `AlreadyExists` error instead.
        // Let's create a file and use it as `cwd`. Then cwd.join("Vagrantfile") will be something like `file/Vagrantfile`.
        // That will throw an IO Error because `file` is not a directory.

        let file_path = dir.path().join("fake_dir");
        fs::write(&file_path, "dummy").expect("operation should succeed");

        let args = mock_args();
        let result = execute(&file_path, &args);
        assert!(matches!(result, Err(MigratoryError::Io(_))));
    }

    #[test]
    fn test_execute_init_template() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let tmpl_path = cwd.join("custom.template");
        fs::write(&tmpl_path, "# Custom: {{box_name}}").expect("write failed");

        let args = InitArgs {
            box_name: Some("mybox".to_string()),
            output: None,
            box_version: None,
            force: false,
            minimal: false,
            template: Some(tmpl_path.to_str().expect("to_str").to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        let content = fs::read_to_string(cwd.join("Vagrantfile")).expect("read failed");
        assert_eq!(content, "# Custom: mybox");

        // Non-existent template
        let args_missing = InitArgs {
            box_name: None,
            output: None,
            box_version: None,
            force: true,
            minimal: false,
            template: Some("/nonexistent/template".to_string()),
        };
        let result_missing = execute(cwd, &args_missing);
        assert!(matches!(result_missing, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_init_template_dir_io_error() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let tmpl_dir = cwd.join("tmpl_dir");
        fs::create_dir(&tmpl_dir).expect("create_dir failed");

        let args = InitArgs {
            box_name: None,
            output: None,
            box_version: None,
            force: true,
            minimal: false,
            template: Some(tmpl_dir.to_str().expect("to_str").to_string()),
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::Io(_))));
    }
}
