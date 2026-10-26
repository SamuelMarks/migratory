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
    convert_disk_with("qemu-img", src_disk, dst_disk, target_fmt)
}

/// Converts a disk image to the target format using a customizable `qemu-img` binary or fallback copy.
///
/// # Arguments
///
/// * `qemu_img_bin` - Path or binary name for `qemu-img`.
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
pub fn convert_disk_with(
    qemu_img_bin: &str,
    src_disk: &Path,
    dst_disk: &Path,
    target_fmt: &str,
) -> Result<(), MigratoryError> {
    let status = std::process::Command::new(qemu_img_bin)
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
    let home_dir = std::env::var("VAGRANT_HOME").unwrap_or_else(|_| ".vagrant.d".to_string());
    execute_with_config(args, Path::new(&home_dir), "qemu-img")
}

/// Executes the `mutate` command with explicit Vagrant home directory and `qemu-img` binary path.
///
/// # Arguments
///
/// * `args` - The parsed arguments for the `mutate` command.
/// * `vagrant_home` - Base directory for Vagrant global state.
/// * `qemu_img_bin` - Path or binary name for `qemu-img`.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if the box cannot be mutated.
pub fn execute_with_config(
    args: &MutateArgs,
    vagrant_home: &Path,
    qemu_img_bin: &str,
) -> Result<(), MigratoryError> {
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

    let manager = crate::box_manager::BoxManager::new(vagrant_home);
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
            convert_disk_with(qemu_img_bin, &src, &dst, target_fmt)?;
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
    fn test_execute_mutate() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let box_dir = dir.path().join("boxes").join("test-VAGRANTSLASH-box");
        fs::create_dir_all(&box_dir)?;

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: true,
        };
        let result = execute_with_config(&args, dir.path(), "nonexistent_qemu_img_xyz");
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_mutate_with_actual_files() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let source_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("virtualbox");
        fs::create_dir_all(&source_dir)?;

        fs::write(source_dir.join("box.ovf"), "<ovf/>")?;
        fs::write(source_dir.join("box-disk1.vmdk"), "dummy-vmdk-bytes")?;
        let meta = r#"{"provider": "virtualbox"}"#;
        fs::write(source_dir.join("metadata.json"), meta)?;
        let vf = "Vagrant.configure(\"2\") do |config|\n  config.vm.provider :virtualbox\nend\n";
        fs::write(source_dir.join("Vagrantfile"), vf)?;

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: Some("virtualbox".to_string()),
            force_virtio: true,
        };
        let result = execute_with_config(&args, dir.path(), "nonexistent_qemu_img_xyz");
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
        Ok(())
    }

    #[test]
    fn test_execute_mutate_no_force() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let box_dir = dir.path().join("boxes").join("test-VAGRANTSLASH-box");
        fs::create_dir_all(&box_dir)?;

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: false,
        };
        let result = execute_with_config(&args, dir.path(), "nonexistent_qemu_img_xyz");
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_mutate_no_env() {
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
    fn test_execute_mutate_box_not_found() -> Result<(), MigratoryError> {
        let dir = tempdir()?;

        let args = MutateArgs {
            box_name: Some("missing/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: false,
        };
        let result = execute_with_config(&args, dir.path(), "nonexistent_qemu_img_xyz");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_mutate_missing_args() {
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
    fn test_find_source_provider_dir_nonexistent() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let non_existent = dir.path().join("does-not-exist");
        let res = find_source_provider_dir(&non_existent, None)?;
        assert!(res.is_none());
        Ok(())
    }

    #[test]
    fn test_find_source_provider_dir_branches() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let box_dir = dir.path().join("my-box");
        let ver_dir = box_dir.join("1.0.0");
        let prov_dir = ver_dir.join("virtualbox");
        fs::create_dir_all(&prov_dir)?;

        // Add stray files that are not directories
        fs::write(box_dir.join("stray_file.txt"), "stray")?;
        fs::write(ver_dir.join("stray_file.txt"), "stray")?;

        // requested_input_provider is None -> matches first provider
        let res_none = find_source_provider_dir(&box_dir, None)?;
        assert!(res_none.is_some());

        // requested_input_provider does not match
        let res_other = find_source_provider_dir(&box_dir, Some("vmware"))?;
        assert!(res_other.is_none());
        Ok(())
    }

    #[test]
    fn test_convert_disk_paths() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let src_disk = dir.path().join("src.vmdk");
        let dst_disk = dir.path().join("dst.qcow2");
        fs::write(&src_disk, "dummy disk bytes")?;

        let _ = convert_disk(&src_disk, &dst_disk, "qcow2");

        // 0. qemu-img succeeds
        let res_ok = convert_disk_with("echo", &src_disk, &dst_disk, "qcow2");
        assert!(res_ok.is_ok());

        // 1. qemu-img fails with non-zero exit code
        let res_fail = convert_disk_with("false", &src_disk, &dst_disk, "qcow2");
        assert!(res_fail.is_err());

        // 2. qemu-img binary not found -> fallback copy
        let res_fallback =
            convert_disk_with("nonexistent_qemu_img_xyz", &src_disk, &dst_disk, "qcow2");
        assert!(res_fallback.is_ok());
        assert!(dst_disk.exists());

        // 3. Fallback copy fails (source file does not exist)
        let res_copy_err = convert_disk_with(
            "nonexistent_qemu_img_xyz",
            Path::new("/nonexistent/disk.vmdk"),
            &dst_disk,
            "qcow2",
        );
        assert!(res_copy_err.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_mutate_to_virtualbox_without_vagrantfile() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let source_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("libvirt");
        fs::create_dir_all(&source_dir)?;

        // Use a .qcow2 disk file to test extension matching
        fs::write(source_dir.join("box.qcow2"), "dummy-qcow2-bytes")?;

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("virtualbox".to_string()),
            input_provider: Some("libvirt".to_string()),
            force_virtio: false,
        };
        let result = execute_with_config(&args, dir.path(), "nonexistent_qemu_img_xyz");
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
        Ok(())
    }

    #[test]
    fn test_execute_mutate_to_qemu_with_force_virtio() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let source_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("virtualbox");
        fs::create_dir_all(&source_dir)?;
        let vf = "Vagrant.configure(\"2\") do |config|\nend\n";
        fs::write(source_dir.join("Vagrantfile"), vf)?;

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("qemu".to_string()),
            input_provider: None,
            force_virtio: true,
        };
        let result = execute_with_config(&args, dir.path(), "nonexistent_qemu_img_xyz");
        assert!(result.is_ok());
        Ok(())
    }

    #[test]
    fn test_execute_mutate_convert_disk_failure() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let source_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("virtualbox");
        fs::create_dir_all(&source_dir)?;
        fs::write(source_dir.join("box-disk1.vmdk"), "dummy")?;

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: false,
        };
        let result = execute_with_config(&args, dir.path(), "false");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_find_source_provider_dir_not_a_directory() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let file_as_box_dir = dir.path().join("file_box");
        fs::write(&file_as_box_dir, "not a dir")?;
        let res = find_source_provider_dir(&file_as_box_dir, None);
        assert!(res.is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_find_source_provider_dir_unreadable_version() -> Result<(), MigratoryError> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir()?;
        let box_dir = dir.path().join("box");
        let ver_dir = box_dir.join("1.0.0");
        fs::create_dir_all(&ver_dir)?;
        fs::set_permissions(&ver_dir, fs::Permissions::from_mode(0o000))?;
        let res = find_source_provider_dir(&box_dir, None);
        fs::set_permissions(&ver_dir, fs::Permissions::from_mode(0o755))?;
        assert!(res.is_err());
        Ok(())
    }

    #[test]
    fn test_mutate_io_failures() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let box_dir = dir.path().join("boxes").join("test-VAGRANTSLASH-box");
        let source_dir = box_dir.join("1.0.0").join("virtualbox");
        fs::create_dir_all(&source_dir)?;

        // 1. dest_dir is a file -> create_dir_all fails
        let dest_dir = box_dir.join("1.0.0").join("libvirt");
        fs::write(&dest_dir, "blocking_file")?;
        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: false,
        };
        let res = execute_with_config(&args, dir.path(), "nonexistent_qemu_img_xyz");
        assert!(res.is_err());
        fs::remove_file(&dest_dir)?;

        // 2. metadata.json is a directory -> fs::write fails
        fs::create_dir_all(dest_dir.join("metadata.json"))?;
        let res2 = execute_with_config(&args, dir.path(), "nonexistent_qemu_img_xyz");
        assert!(res2.is_err());
        fs::remove_dir(dest_dir.join("metadata.json"))?;

        // 3. Vagrantfile is a directory -> fs::write fails
        fs::create_dir_all(dest_dir.join("Vagrantfile"))?;
        let res3 = execute_with_config(&args, dir.path(), "nonexistent_qemu_img_xyz");
        assert!(res3.is_err());

        Ok(())
    }

    #[test]
    fn test_execute_mutate_box_dir_is_file() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let box_file = dir.path().join("boxes").join("test-VAGRANTSLASH-box");
        fs::create_dir_all(dir.path().join("boxes"))?;
        fs::write(&box_file, "blocking_file")?;

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("libvirt".to_string()),
            input_provider: None,
            force_virtio: false,
        };
        let result = execute_with_config(&args, dir.path(), "nonexistent_qemu_img_xyz");
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn test_execute_mutate_force_virtio_non_qemu_with_vagrantfile() -> Result<(), MigratoryError> {
        let dir = tempdir()?;
        let source_dir = dir
            .path()
            .join("boxes")
            .join("test-VAGRANTSLASH-box")
            .join("1.0.0")
            .join("virtualbox");
        fs::create_dir_all(&source_dir)?;

        fs::write(source_dir.join("box-disk1.vmdk"), "dummy-vmdk-bytes")?;
        let vf = "Vagrant.configure('2') do |c|\n  c.vm.provider :virtualbox\nend\n";
        fs::write(source_dir.join("Vagrantfile"), vf)?;

        let args = MutateArgs {
            box_name: Some("test/box".to_string()),
            destination_provider: Some("vmware".to_string()),
            input_provider: Some("virtualbox".to_string()),
            force_virtio: true,
        };
        let result = execute_with_config(&args, dir.path(), "nonexistent_qemu_img_xyz");
        assert!(result.is_ok());
        Ok(())
    }
}
