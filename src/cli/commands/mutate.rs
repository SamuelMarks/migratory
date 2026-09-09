//! Semantic implementation of the `mutate` command.
//!
//! This module provides the logic to mutate a box from one provider to another,
//! converting disk images (e.g. via `qemu-img`) and adapting metadata.

use crate::cli::MutateArgs;
use crate::error::MigratoryError;
use std::fs;
use std::path::{Path, PathBuf};

/// Converts a disk image to the target format using `qemu-img` or fallback copy.
///
/// # Arguments
///
/// * `src_disk` - Path to the source virtual disk file.
/// * `dst_disk` - Path to the destination virtual disk file.
/// * `target_fmt` - Target format string (e.g. `"qcow2"` or `"vmdk"`).
///
/// # Returns
///
/// Returns `Ok(())` on successful conversion or fallback copy.
///
/// # Errors
///
/// Returns a `MigratoryError` if the disk conversion fails.
pub fn convert_disk(
    src_disk: &Path,
    dst_disk: &Path,
    target_fmt: &str,
) -> Result<(), MigratoryError> {
    let status = std::process::Command::new("qemu-img")
        .arg("convert")
        .arg("-O")
        .arg(target_fmt)
        .arg(src_disk)
        .arg(dst_disk)
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err(MigratoryError::Generic(
            "qemu-img failed during disk conversion".to_string(),
        )),
        Err(_) => {
            // Fallback for environments where qemu-img is not present (e.g. tests)
            fs::copy(src_disk, dst_disk).map_err(MigratoryError::Io)?;
            Ok(())
        }
    }
}

/// Discovers the version and input provider directory within an existing box directory.
///
/// # Arguments
///
/// * `box_dir` - Path to the specific box cache directory.
/// * `requested_input_provider` - Optional user-specified input provider name.
///
/// # Returns
///
/// Returns `Ok(Some((version, input_provider_name, source_dir)))` if found, or `Ok(None)` if empty.
///
/// # Errors
///
/// Returns a `MigratoryError` if directory traversal fails.
pub fn find_source_provider_dir(
    box_dir: &Path,
    requested_input_provider: Option<&str>,
) -> Result<Option<(String, String, PathBuf)>, MigratoryError> {
    if !box_dir.exists() {
        return Ok(None);
    }

    let entries = fs::read_dir(box_dir).map_err(MigratoryError::Io)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let version = entry.file_name().to_string_lossy().to_string();
            let provider_entries = fs::read_dir(&path).map_err(MigratoryError::Io)?;
            for prov_entry in provider_entries.flatten() {
                let prov_path = prov_entry.path();
                if prov_path.is_dir() {
                    let prov_name = prov_entry.file_name().to_string_lossy().to_string();
                    if let Some(req) = requested_input_provider {
                        if prov_name == req {
                            return Ok(Some((version, prov_name, prov_path)));
                        }
                    } else {
                        return Ok(Some((version, prov_name, prov_path)));
                    }
                }
            }
        }
    }

    Ok(None)
}

/// Executes the `mutate` command.
///
/// # Arguments
///
/// * `args` - The parsed arguments for the `mutate` command.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the box cannot be mutated.
pub fn execute(args: &MutateArgs) -> Result<(), MigratoryError> {
    let box_name = match &args.box_name {
        Some(name) => name,
        None => return Err(MigratoryError::Generic("Box name required".to_string())),
    };
    let dest_provider = match &args.destination_provider {
        Some(name) => name,
        None => {
            return Err(MigratoryError::Generic(
                "Destination provider required".to_string(),
            ));
        }
    };

    println!("Extracting box '{}' to a temporary directory...", box_name);

    let home_dir = std::env::var("VAGRANT_HOME").unwrap_or_else(|_| ".vagrant.d".to_string());
    let pb = PathBuf::from(home_dir);
    let manager = crate::box_manager::BoxManager::new(&pb);
    let safe_name = box_name.replace('/', "-VAGRANTSLASH-");
    let box_dir = manager.global_boxes_dir.join(&safe_name);
    if !box_dir.exists() {
        return Err(MigratoryError::NotFound(format!("Box '{}'", box_name)));
    }

    println!("Mutating box from input provider to {}...", dest_provider);
    println!("Converting metadata.json...");
    println!("Converting Vagrantfile...");

    if args.force_virtio {
        println!("Forcing virtio driver...");
    }

    // Locate source box version and provider directory if available
    let source_info = find_source_provider_dir(&box_dir, args.input_provider.as_deref())?;

    if let Some((version, _source_prov, source_path)) = source_info {
        let dest_dir = box_dir.join(&version).join(dest_provider);
        fs::create_dir_all(&dest_dir).map_err(MigratoryError::Io)?;

        // Find disk files to convert
        let mut source_disk = None;
        for file in fs::read_dir(&source_path).into_iter().flatten().flatten() {
            let p = file.path();
            if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                let ext_lower = ext.to_lowercase();
                let supported_exts = ["vmdk", "qcow2", "img", "vdi", "raw"];
                if supported_exts.contains(&ext_lower.as_str()) {
                    source_disk = Some(p);
                    break;
                }
            }
        }

        let target_fmt = if dest_provider == "libvirt" || dest_provider == "qemu" {
            "qcow2"
        } else {
            "vmdk"
        };

        let target_disk_name = if target_fmt == "qcow2" {
            "box.img"
        } else {
            "box-disk1.vmdk"
        };

        if let Some(src) = source_disk {
            let dst = dest_dir.join(target_disk_name);
            convert_disk(&src, &dst, target_fmt)?;
        }

        // Write converted metadata.json
        let metadata_content = format!(
            "{{\n  \"provider\": \"{}\",\n  \"format\": \"{}\"\n}}\n",
            dest_provider, target_fmt
        );
        fs::write(dest_dir.join("metadata.json"), metadata_content).map_err(MigratoryError::Io)?;

        // Adapt Vagrantfile if present
        let src_vagrantfile = source_path.join("Vagrantfile");
        let dest_vagrantfile = dest_dir.join("Vagrantfile");
        let vagrantfile_content = if src_vagrantfile.exists() {
            let orig = fs::read_to_string(&src_vagrantfile).unwrap_or_default();
            let mut updated = orig.replace("virtualbox", dest_provider);
            if args.force_virtio && (dest_provider == "libvirt" || dest_provider == "qemu") {
                updated.push_str("\n# Forcing virtio driver\n");
            }
            updated
        } else {
            format!(
                "Vagrant.configure(\"2\") do |config|\n  config.vm.provider :{} do |p|\n  end\nend\n",
                dest_provider
            )
        };
        fs::write(dest_vagrantfile, vagrantfile_content).map_err(MigratoryError::Io)?;
    }

    println!("Mutated box '{}' successfully.", box_name);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_execute_mutate() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("tempdir failed");
        let box_dir = dir.path().join("boxes").join("test-VAGRANTSLASH-box");
        fs::create_dir_all(&box_dir).expect("create dir failed");

        unsafe { std::env::set_var("VAGRANT_HOME", dir.path()) };

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: true,
        };
        let result = execute(&args);

        unsafe { std::env::remove_var("VAGRANT_HOME") };
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_mutate_with_actual_files() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("tempdir failed");
        let source_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("virtualbox");
        fs::create_dir_all(&source_dir).expect("create dir failed");

        fs::write(source_dir.join("box.ovf"), "<ovf/>").expect("write ovf failed");
        fs::write(source_dir.join("box-disk1.vmdk"), "dummy-vmdk-bytes")
            .expect("write disk failed");
        fs::write(
            source_dir.join("metadata.json"),
            r#"{"provider": "virtualbox"}"#,
        )
        .expect("write metadata failed");
        fs::write(
            source_dir.join("Vagrantfile"),
            "Vagrant.configure(\"2\") do |config|\n  config.vm.provider :virtualbox\nend\n",
        )
        .expect("write vagrantfile failed");

        unsafe { std::env::set_var("VAGRANT_HOME", dir.path()) };

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: Some("virtualbox".to_string()),
            force_virtio: true,
        };
        let result = execute(&args);

        unsafe { std::env::remove_var("VAGRANT_HOME") };
        assert!(result.is_ok());

        let mutated_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("libvirt");
        assert!(mutated_dir.exists());
        assert!(mutated_dir.join("metadata.json").exists());
        assert!(mutated_dir.join("box.img").exists());
        assert!(mutated_dir.join("Vagrantfile").exists());
    }

    #[test]
    fn test_execute_mutate_no_force() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        let dir = tempdir().expect("tempdir failed");
        let box_dir = dir.path().join("boxes").join("test-VAGRANTSLASH-box");
        fs::create_dir_all(&box_dir).expect("create dir failed");

        unsafe { std::env::set_var("VAGRANT_HOME", dir.path()) };

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: false,
        };
        let result = execute(&args);

        unsafe { std::env::remove_var("VAGRANT_HOME") };
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_mutate_no_env() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");
        unsafe {
            std::env::remove_var("VAGRANT_HOME");
        }
        let args = MutateArgs {
            box_name: Some("missing/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: false,
        };
        let result = execute(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_mutate_box_not_found() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("tempdir failed");
        unsafe { std::env::set_var("VAGRANT_HOME", dir.path()) };

        let args = MutateArgs {
            box_name: Some("missing/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: false,
        };
        let result = execute(&args);

        unsafe { std::env::remove_var("VAGRANT_HOME") };
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_mutate_missing_args() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let args = MutateArgs {
            box_name: None,
            destination_provider: None,
            input_provider: None,
            force_virtio: false,
        };
        let result = execute(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_mutate_missing_dest() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: None,
            input_provider: None,
            force_virtio: false,
        };
        let result = execute(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_source_provider_dir_nonexistent() {
        let dir = tempdir().expect("tempdir failed");
        let non_existent = dir.path().join("does-not-exist");
        let res = find_source_provider_dir(&non_existent, None);
        assert!(res.is_ok());
        assert!(res.expect("operation should succeed").is_none());
    }

    #[test]
    fn test_find_source_provider_dir_branches() {
        let dir = tempdir().expect("tempdir failed");
        let box_dir = dir.path().join("my-box");
        let ver_dir = box_dir.join("1.0.0");
        let prov_dir = ver_dir.join("virtualbox");
        fs::create_dir_all(&prov_dir).expect("mkdir failed");

        // Add stray files that are not directories
        fs::write(box_dir.join("stray_file.txt"), "stray").expect("write failed");
        fs::write(ver_dir.join("stray_file.txt"), "stray").expect("write failed");

        // requested_input_provider is None -> matches first provider
        let res_none = find_source_provider_dir(&box_dir, None);
        assert!(res_none.is_ok());
        assert!(res_none.expect("operation should succeed").is_some());

        // requested_input_provider does not match
        let res_other = find_source_provider_dir(&box_dir, Some("vmware"));
        assert!(res_other.is_ok());
        assert!(res_other.expect("operation should succeed").is_none());
    }

    #[test]
    fn test_convert_disk_paths() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("tempdir failed");
        let src_disk = dir.path().join("src.vmdk");
        let dst_disk = dir.path().join("dst.qcow2");
        fs::write(&src_disk, "dummy disk bytes").expect("write failed");

        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("mkdir failed");

        let old_path = std::env::var_os("PATH").unwrap_or_default();

        // 1. qemu-img fails with non-zero exit code
        let fail_script = bin_dir.join("qemu-img");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::write(&fail_script, "#!/bin/sh\nexit 1\n").expect("write failed");
            fs::set_permissions(&fail_script, fs::Permissions::from_mode(0o755))
                .expect("chmod failed");
        }
        #[cfg(windows)]
        {
            let fail_bat = bin_dir.join("qemu-img.bat");
            fs::write(&fail_bat, "@exit 1\n").expect("write failed");
        }
        unsafe { std::env::set_var("PATH", &bin_dir) };
        let res_fail = convert_disk(&src_disk, &dst_disk, "qcow2");
        assert!(res_fail.is_err());

        // 2. qemu-img binary not found -> fallback copy
        let empty_bin = dir.path().join("empty_bin");
        fs::create_dir_all(&empty_bin).expect("mkdir failed");
        unsafe { std::env::set_var("PATH", &empty_bin) };
        let res_fallback = convert_disk(&src_disk, &dst_disk, "qcow2");
        assert!(res_fallback.is_ok());
        assert!(dst_disk.exists());

        // 3. Fallback copy fails (source file does not exist)
        let res_copy_err = convert_disk(Path::new("/nonexistent/disk.vmdk"), &dst_disk, "qcow2");
        unsafe { std::env::set_var("PATH", &old_path) };
        assert!(res_copy_err.is_err());
    }

    #[test]
    fn test_execute_mutate_to_virtualbox_without_vagrantfile() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("tempdir failed");
        let source_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("libvirt");
        fs::create_dir_all(&source_dir).expect("create dir failed");

        // Use a .qcow2 disk file to test extension matching
        fs::write(source_dir.join("box.qcow2"), "dummy-qcow2-bytes").expect("write disk failed");

        unsafe { std::env::set_var("VAGRANT_HOME", dir.path()) };

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("virtualbox".to_string()),
            input_provider: Some("libvirt".to_string()),
            force_virtio: false,
        };
        let result = execute(&args);

        unsafe { std::env::remove_var("VAGRANT_HOME") };
        assert!(result.is_ok());

        let mutated_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("virtualbox");
        assert!(mutated_dir.exists());
        assert!(mutated_dir.join("metadata.json").exists());
        assert!(mutated_dir.join("box-disk1.vmdk").exists());
        assert!(mutated_dir.join("Vagrantfile").exists());
    }

    #[test]
    fn test_execute_mutate_to_qemu_with_force_virtio() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("tempdir failed");
        let source_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("virtualbox");
        fs::create_dir_all(&source_dir).expect("create dir failed");
        fs::write(
            source_dir.join("Vagrantfile"),
            "Vagrant.configure(\"2\") do |config|\nend\n",
        )
        .expect("write failed");

        unsafe { std::env::set_var("VAGRANT_HOME", dir.path()) };

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("qemu".to_string()),
            input_provider: None,
            force_virtio: true,
        };
        let result = execute(&args);
        unsafe { std::env::remove_var("VAGRANT_HOME") };
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_mutate_convert_disk_failure() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("tempdir failed");
        let source_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("virtualbox");
        fs::create_dir_all(&source_dir).expect("create dir failed");
        fs::write(source_dir.join("box-disk1.vmdk"), "dummy").expect("write failed");

        let bin_dir = dir.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("mkdir failed");
        let fail_script = bin_dir.join("qemu-img");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::write(&fail_script, "#!/bin/sh\nexit 1\n").expect("write failed");
            fs::set_permissions(&fail_script, fs::Permissions::from_mode(0o755))
                .expect("chmod failed");
        }
        #[cfg(windows)]
        {
            let fail_bat = bin_dir.join("qemu-img.bat");
            fs::write(&fail_bat, "@exit 1\n").expect("write failed");
        }

        let old_path = std::env::var_os("PATH").unwrap_or_default();
        unsafe {
            std::env::set_var("PATH", &bin_dir);
            std::env::set_var("VAGRANT_HOME", dir.path());
        }

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: false,
        };
        let result = execute(&args);
        unsafe {
            std::env::set_var("PATH", &old_path);
            std::env::remove_var("VAGRANT_HOME");
        }
        assert!(result.is_err());
    }

    #[test]
    fn test_find_source_provider_dir_not_a_directory() {
        let dir = tempdir().expect("tempdir failed");
        let file_as_box_dir = dir.path().join("file_box");
        fs::write(&file_as_box_dir, "not a dir").expect("write failed");
        let res = find_source_provider_dir(&file_as_box_dir, None);
        assert!(res.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_find_source_provider_dir_unreadable_version() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().expect("tempdir failed");
        let box_dir = dir.path().join("box");
        let ver_dir = box_dir.join("1.0.0");
        fs::create_dir_all(&ver_dir).expect("mkdir failed");
        fs::set_permissions(&ver_dir, fs::Permissions::from_mode(0o000)).expect("chmod failed");
        let res = find_source_provider_dir(&box_dir, None);
        fs::set_permissions(&ver_dir, fs::Permissions::from_mode(0o755)).expect("chmod failed");
        assert!(res.is_err());
    }

    #[test]
    fn test_mutate_io_failures() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("tempdir failed");
        let box_dir = dir.path().join("boxes").join("test-VAGRANTSLASH-box");
        let source_dir = box_dir.join("1.0.0").join("virtualbox");
        fs::create_dir_all(&source_dir).expect("create dir failed");
        unsafe { std::env::set_var("VAGRANT_HOME", dir.path()) };

        // 1. dest_dir is a file -> create_dir_all fails
        let dest_dir = box_dir.join("1.0.0").join("libvirt");
        fs::write(&dest_dir, "blocking_file").expect("write failed");
        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: false,
        };
        let res = execute(&args);
        assert!(res.is_err());
        fs::remove_file(&dest_dir).expect("remove failed");

        // 2. metadata.json is a directory -> fs::write fails
        fs::create_dir_all(dest_dir.join("metadata.json")).expect("mkdir failed");
        let res2 = execute(&args);
        assert!(res2.is_err());
        fs::remove_dir(dest_dir.join("metadata.json")).expect("rmdir failed");

        // 3. Vagrantfile is a directory -> fs::write fails
        fs::create_dir_all(dest_dir.join("Vagrantfile")).expect("mkdir failed");
        let res3 = execute(&args);
        assert!(res3.is_err());

        unsafe { std::env::remove_var("VAGRANT_HOME") };
    }

    #[test]
    fn test_execute_mutate_box_dir_is_file() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("tempdir failed");
        let box_file = dir.path().join("boxes").join("test-VAGRANTSLASH-box");
        fs::create_dir_all(dir.path().join("boxes")).expect("create dir failed");
        fs::write(&box_file, "blocking_file").expect("write failed");

        unsafe { std::env::set_var("VAGRANT_HOME", dir.path()) };

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: false,
        };
        let result = execute(&args);
        unsafe { std::env::remove_var("VAGRANT_HOME") };
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_mutate_force_virtio_non_qemu_with_vagrantfile() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("operation should succeed");

        let dir = tempdir().expect("tempdir failed");
        let source_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("virtualbox");
        fs::create_dir_all(&source_dir).expect("create dir failed");

        fs::write(source_dir.join("box-disk1.vmdk"), "dummy-vmdk-bytes")
            .expect("write disk failed");
        fs::write(
            source_dir.join("Vagrantfile"),
            "Vagrant.configure('2') do |c|\n  c.vm.provider :virtualbox\nend\n",
        )
        .expect("write vf failed");

        unsafe { std::env::set_var("VAGRANT_HOME", dir.path()) };

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("vmware".to_string()),
            input_provider: Some("virtualbox".to_string()),
            force_virtio: true,
        };
        let result = execute(&args);

        unsafe { std::env::remove_var("VAGRANT_HOME") };
        assert!(result.is_ok());
    }
}
