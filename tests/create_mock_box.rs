//! Helper utilities and tests for creating mock box archives.

use flate2::Compression;
use flate2::write::GzEncoder;
use std::fs::File;

/// Creates a valid (empty) `.box` tar.gz archive at the given destination path.
///
/// # Errors
///
/// Returns an [`std::io::Error`] if creating or writing to the archive fails.
pub fn create_valid_box(path: &std::path::Path) -> Result<(), std::io::Error> {
    let tar_gz = File::create(path)?;
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = tar::Builder::new(enc);
    tar.finish()?;
    Ok(())
}

#[test]
fn test_create_valid_box() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let box_path = temp.path().join("package.box");
    create_valid_box(&box_path)?;
    assert!(box_path.exists());
    Ok(())
}
