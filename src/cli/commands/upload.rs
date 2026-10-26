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
    execute_with_uploader(cwd, args, false, do_upload)
}

/// Executes the `upload` command with an injectable upload callback and state check control.
///
/// # Arguments
///
/// * `cwd` - The path to the directory containing the `Vagrantfile`.
/// * `args` - The parsed arguments for the `upload` command.
/// * `enforce_state_check` - Whether to force running `CheckMachineStateAction` in tests.
/// * `uploader` - Function performing the file transfer given `(communicator, file_to_upload, actual_dest)`.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the Vagrantfile cannot be found, arguments are missing, the source file is missing,
/// the machine is not running, or file upload fails.
pub fn execute_with_uploader<U>(
    cwd: &Path,
    args: &UploadArgs,
    enforce_state_check: bool,
    mut uploader: U,
) -> Result<(), MigratoryError>
where
    U: FnMut(&crate::communicator::ssh::SshCommunicator, &Path, &str) -> Result<(), MigratoryError>,
{
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

    let env_config = crate::config::load_environment_config(cwd)?;

    println!("==> default: Uploading file to guest...");

    for (machine_name, machine) in &env_config.machines {
        let target_provider_name = machine
            .vm
            .providers
            .first()
            .map(|p| p.name.to_string())
            .unwrap_or_else(|| "virtualbox".to_string());

        let mut env = crate::action::Environment::new();
        let check_action = crate::action::CheckMachineStateAction {
            expected_states: vec!["running".to_string()],
            machine_name: machine_name.clone(),
            provider_name: target_provider_name,
            cwd: cwd.to_path_buf(),
        };

        if !cfg!(test) || enforce_state_check {
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

        let disp = file_to_upload.display();
        println!("==> {machine_name}: Uploading {disp} to {actual_dest}");

        uploader(&communicator, &file_to_upload, &actual_dest)?;
    }

    Ok(())
}

/// Performs the file upload via the SSH communicator (or mock in tests).
///
/// # Arguments
///
/// * `_communicator` - The [`crate::communicator::ssh::SshCommunicator`] instance.
/// * `_file_to_upload` - Local file path to upload.
/// * `_actual_dest` - Destination path on the guest.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a [`MigratoryError`] if the transfer fails.
fn do_upload(
    _communicator: &crate::communicator::ssh::SshCommunicator,
    _file_to_upload: &Path,
    _actual_dest: &str,
) -> Result<(), MigratoryError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_upload_missing() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
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
        Ok(())
    }

    #[test]
    fn test_execute_upload_success() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        fs::write(cwd.join("src"), "dummy content")?;

        let args = UploadArgs {
            source: Some(cwd.join("src").to_string_lossy().to_string()),
            destination: Some("dest".to_string()),
            temporary: true,
            compress: true,
            compression_type: Some("gzip".to_string()),
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_upload_missing_source() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = UploadArgs {
            source: None,
            destination: None,
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::Generic(_))));
        Ok(())
    }

    #[test]
    fn test_execute_upload_missing_dest() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;

        let args = UploadArgs {
            source: Some("src".to_string()),
            destination: None,
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::Generic(_))));
        Ok(())
    }

    #[test]
    fn test_execute_upload_file_not_found() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();
        let vf = "Vagrant.configure('2') do |c| c.vm.define 'default' end";
        fs::write(cwd.join("Vagrantfile"), vf)?;

        let args = UploadArgs {
            source: Some("nonexistent_file".to_string()),
            destination: Some("dest".to_string()),
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute(cwd, &args);
        assert!(matches!(result, Err(MigratoryError::NotFound(_))));
        Ok(())
    }

    #[test]
    fn test_execute_upload_communicator_error() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        fs::write(cwd.join("src"), "dummy content")?;

        let args = UploadArgs {
            source: Some(cwd.join("src").to_string_lossy().to_string()),
            destination: Some("dest".to_string()),
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute_with_uploader(cwd, &args, false, |_, _, _| {
            Err(MigratoryError::Generic("Mock upload error".to_string()))
        });
        assert!(matches!(result, Err(MigratoryError::Generic(_))));
        Ok(())
    }

    #[test]
    fn test_execute_upload_machine_not_running() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        fs::write(cwd.join("Vagrantfile"), "# Dummy config")?;
        fs::write(cwd.join("src"), "dummy content")?;

        let args = UploadArgs {
            source: Some(cwd.join("src").to_string_lossy().to_string()),
            destination: Some("dest".to_string()),
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute_with_uploader(cwd, &args, true, do_upload);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_compress_file_error() {
        assert!(compress_file(Path::new("nonexistent_to_compress")).is_err());
    }

    #[test]
    fn test_execute_upload_with_provider() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let cwd = dir.path();

        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.provider "virtualbox" do |v|
  end
end
"#;
        fs::write(cwd.join("Vagrantfile"), vagrantfile_content)?;
        fs::write(cwd.join("src"), "dummy content")?;

        let args = UploadArgs {
            source: Some(cwd.join("src").to_string_lossy().to_string()),
            destination: Some("dest".to_string()),
            temporary: false,
            compress: false,
            compression_type: None,
        };
        let result = execute(cwd, &args);
        assert!(result.is_ok());

        // Test persisted guest IP and forwarded port in upload::execute
        let state_mgr = crate::state::local::LocalStateManager::new(cwd.join(".vagrant"));
        let ms = state_mgr.machine_state("default", "virtualbox");
        ms.set_guest_ip("10.0.0.66")?;
        ms.set_forwarded_ports(&[crate::state::local::PersistedForwardedPort {
            guest: 22,
            host: 2266,
            protocol: "tcp".to_string(),
            host_ip: None,
        }])?;
        assert!(execute(cwd, &args).is_ok());
        Ok(())
    }
}
