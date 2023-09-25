use thiserror::Error;

pub const USER_AGENT: &str = const_format::formatcp!("modestly-modular-modpack-modifier/{} ureq", env!("CARGO_PKG_VERSION"));

#[derive(Error, Debug)]
pub enum ArchiveDownloadError {
    #[error("Failed to download archive from URL {0}. Error: {1}")]
    Download(String, ureq::Error),
    #[error("Failed to read downloaded archive to bytes. Error: {0}")]
    Read(std::io::Error),
}

pub fn download_archive(url: &str) -> Result<Vec<u8>, ArchiveDownloadError> {
    let response = ureq::get(&url).set("User-Agent", USER_AGENT).call().map_err(|e| ArchiveDownloadError::Download(url.into(), e))?;
    let mut archive = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut archive)
        .map_err(|e| ArchiveDownloadError::Read(e))?;
    Ok(archive)
}
