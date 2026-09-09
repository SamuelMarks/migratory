use flate2::Compression;
use flate2::write::GzEncoder;
use std::fs::File;

pub fn create_valid_box(path: &std::path::Path) -> Result<(), std::io::Error> {
    let tar_gz = File::create(path)?;
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = tar::Builder::new(enc);
    tar.finish()?;
    Ok(())
}
