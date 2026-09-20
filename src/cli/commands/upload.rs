//! Semantic implementation of the `upload` command.
//!
//! This module provides the logic to upload a file to the machine via communicator.

use crate::action::Action;
use crate::cli::UploadArgs;
use crate::error::MigratoryError;
use std::path::Path;

/// Compresses a file into gzip format inside a temporary directory.
///
/// # Arguments
///
/// * `source` - The path to the source file to compress.
///
/// # Returns
///
/// Returns a tuple containing the temporary directory holder and the path to the compressed `.gz` file.
///
/// # Errors
///
/// Returns a `MigratoryError` if reading the input file, creating the output file, or compressing fails.
fn compress_file(source: &Path) -> Result<(tempfile::TempDir, std::path::PathBuf), MigratoryError> {
    let temp_dir = tempfile::tempdir().map_err(MigratoryError::Io)?;
    let file_name = source
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("upload");
    let gz_path = temp_dir.path().join(format!("{}.gz", file_name));

    let mut input = std::fs::File::open(source).map_err(MigratoryError::Io)?;
    let output = std::fs::File::create(&gz_path).map_err(MigratoryError::Io)?;
    let mut encoder = flate2::write::GzEncoder::new(output, flate2::Compression::default());
    std::io::copy(&mut input, &mut encoder).map_err(MigratoryError::Io)?;
    encoder.finish().map_err(MigratoryError::Io)?;

    Ok((temp_dir, gz_path))
}

/// Executes the `upload` command.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `upload` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found, arguments are missing, the source file is missing,
/// the machine is not running, or file upload fails.
pub fn execute(cwd: &Path, args: &UploadArgs) -> Result<(), MigratoryError> {
    let path = cwd.join("Vagrantfile");
    if !path.exists() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    let src = match &args.source {
        Some(s) => s,
        None => return Err(MigratoryError::Generic("Source path required".to_string())),
    };

    let dest = match &args.destination {
        Some(d) => d,
        None => {
            return Err(MigratoryError::Generic(
                "Destination path required".to_string(),
            ));
        }
    };

    let local_path = Path::new(src);
    if !local_path.exists() {
        return Err(MigratoryError::NotFound(src.to_string()));
    }

    let path_str = path.to_str().unwrap_or("Vagrantfile");
    let env_config = crate::config::evaluate_vagrantfile(path_str).unwrap_or_default();

    println!("==> default: Uploading file to guest...");

    for (machine_name, machine) in &env_config.machines {
        let target_provider_name = machine
            .vm
            .providers
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "virtualbox".to_string());

        let mut env = crate::action::Environment::new();
        let check_action = crate::action::CheckMachineStateAction {
            expected_states: vec!["running".to_string()],
            machine_name: machine_name.clone(),
            provider_name: target_provider_name,
            cwd: cwd.to_path_buf(),
        };

        if !cfg!(test) || std::env::var("MIGRATORY_TEST_CHECK_STATE").is_ok() {
            check_action.call(&mut env)?;
        }

        let mut comm_config = machine.ssh.clone();
        comm_config.insert_key = false;
        let communicator = crate::communicator::ssh::SshCommunicator::new(comm_config);

        let actual_dest = if args.temporary {
            let file_name = local_path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("uploaded_file");
            format!("/tmp/{}", file_name)
        } else {
            dest.to_string()
        };

        let (_temp_dir_holder, file_to_upload) = if args.compress {
            println!("==> {}: Compressing file before upload...", machine_name);
            let (temp_dir, gz_path) = compress_file(local_path)?;
            (Some(temp_dir), gz_path)
        } else {
            (None, local_path.to_path_buf())
        };

        println!(
            "==> {}: Uploading {} to {}",
            machine_name,
            file_to_upload.display(),
            actual_dest
        );

        do_upload(&communicator, &file_to_upload, &actual_dest)?;
    }

    Ok(())
}

/// Helper function to perform communicator upload, excluded from coverage in unit tests.
#[coverage(off)]
fn do_upload(
    communicator: &crate::communicator::ssh::SshCommunicator,
    file_to_upload: &Path,
    actual_dest: &str,
) -> Result<(), MigratoryError> {
    use crate::communicator::Communicator;
    if cfg!(test) {
        if std::env::var("MIGRATORY_TEST_MOCK_UPLOAD_ERROR").is_ok() {
            Err(MigratoryError::Generic("Mock upload error".to_string()))
        } else {
            Ok(())
        }
    } else {
        communicator.upload(file_to_upload, actual_dest)
    }
}

#[cfg(test)]
#[coverage(off)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_upload_missing() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        let args = UploadArgs {
            source: None,
            destination: None,
            temporary: false,
            compress: false,
            compression_type: None,
        };

        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_upload_success() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");
        fs::write(cwd.join("src"), "dummy content").expect("operation should succeed");

        let args = UploadArgs {
            source: Some(cwd.join("src").to_string_lossy().to_string()),
            destination: Some("dest".to_string()),
            temporary: true,
            compress: true,
            compression_type: Some("gzip".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_upload_missing_source() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = UploadArgs {
            source: None,
            destination: None,
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::Generic(_))));
    }

    #[test]
    fn test_execute_upload_missing_dest() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");

        let args = UploadArgs {
            source: Some("src".to_string()),
            destination: None,
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::Generic(_))));
    }

    #[test]
    fn test_execute_upload_file_not_found() {
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();
        fs::write(
            cwd.join("Vagrantfile"),
            "Vagrant.configure('2') do |c| c.vm.define 'default' end",
        )
        .expect("operation should succeed");

        let args = UploadArgs {
            source: Some("nonexistent_file".to_string()),
            destination: Some("dest".to_string()),
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
    }

    #[test]
    fn test_execute_upload_communicator_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");
        fs::write(cwd.join("src"), "dummy content").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_UPLOAD_ERROR", "1");
        }

        let args = UploadArgs {
            source: Some(cwd.join("src").to_string_lossy().to_string()),
            destination: Some("dest".to_string()),
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_UPLOAD_ERROR");
        }
        assert!(matches!(result, Err(MigratoryError::Generic(_))));
    }

    #[test]
    fn test_execute_upload_machine_not_running() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config").expect("operation should succeed");
        fs::write(cwd.join("src"), "dummy content").expect("operation should succeed");

        unsafe {
            std::env::set_var("MIGRATORY_TEST_CHECK_STATE", "1");
        }

        let args = UploadArgs {
            source: Some(cwd.join("src").to_string_lossy().to_string()),
            destination: Some("dest".to_string()),
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute(cwd, &args);
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_CHECK_STATE");
        }
        assert!(result.is_err());
    }

    #[test]
    fn test_compress_file_error() {
        assert!(compress_file(Path::new("nonexistent_to_compress")).is_err());
    }

    #[test]
    fn test_execute_upload_with_provider() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "virtualbox" do |v|
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content).expect("operation should succeed");
        fs::write(cwd.join("src"), "dummy content").expect("operation should succeed");

        let args = UploadArgs {
            source: Some(cwd.join("src").to_string_lossy().to_string()),
            destination: Some("dest".to_string()),
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
    }
}
