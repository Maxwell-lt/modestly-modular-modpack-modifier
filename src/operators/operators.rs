use std::fs::{remove_dir, remove_file};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Ok, Result};
use dircpy::{copy_dir, CopyBuilder};
use regex::Regex;
use tempdir::TempDir;
use walkdir::WalkDir;
use zip::ZipArchive;

/// Trait that defines an output function for Operators.
pub trait Operator<T> {
    /// Assume this function can be called multiple times,
    /// so implementations should return cloned copies of internal data, if applicable.
    fn output(&self) -> Result<T>;
}

pub struct URILiteral {
    value: String,
}

impl URILiteral {
    pub fn new(value: &str) -> URILiteral {
        URILiteral { value: value.to_owned() }
    }
}

impl Operator<String> for URILiteral {
    fn output(&self) -> Result<String> {
        Ok(self.value.clone())
    }
}

pub struct PathLiteral {
    value: PathBuf,
}

impl PathLiteral {
    pub fn new(value: &str) -> PathLiteral {
        PathLiteral { value: PathBuf::from(value) }
    }
}

impl Operator<PathBuf> for PathLiteral {
    fn output(&self) -> Result<PathBuf> {
        Ok(self.value.clone())
    }
}

pub struct RegexLiteral {
    value: Regex,
}

impl RegexLiteral {
    pub fn new(value: &str) -> Result<RegexLiteral> {
        Ok(RegexLiteral { value: Regex::new(value)? })
    }
}

impl Operator<Regex> for RegexLiteral {
    fn output(&self) -> Result<Regex> {
        Ok(self.value.clone())
    }
}

pub struct ArchiveDownloader {
    archive: TempDir,
}

impl ArchiveDownloader {
    pub fn new(src: &str) -> Result<ArchiveDownloader> {
        let tempdir = TempDir::new("ArchiveDownloader")?;
        let bytes = reqwest::blocking::get(src)?.bytes()?;
        let mut archive = ZipArchive::new(Cursor::new(bytes))?;
        archive.extract(tempdir.path())?;
        Ok(ArchiveDownloader { archive: tempdir })
    }
}

impl Operator<TempDir> for ArchiveDownloader {
    fn output(&self) -> Result<TempDir> {
        let tempdir = TempDir::new("ArchiveDownloader")?;
        copy_dir(self.archive.path(), tempdir.path())?;
        Ok(tempdir)
    }
}

pub struct ArchiveFilter {
    archive: TempDir,
}

impl ArchiveFilter {
    pub fn new(filter: Regex, dir: TempDir) -> Result<ArchiveFilter> {
        for entry in WalkDir::new(dir.path()).contents_first(true) {
            let entry = entry?;
            if !filter.is_match(entry.path().to_string_lossy().as_ref()) {
                if entry.path().is_dir() {
                    if 0 == entry.path().read_dir()?.count() {
                        remove_dir(entry.path())?;
                    }
                } else {
                    remove_file(entry.path())?;
                }
            }
        }
        Ok(ArchiveFilter { archive: dir })
    }
}

impl Operator<TempDir> for ArchiveFilter {
    fn output(&self) -> Result<TempDir> {
        let tempdir = TempDir::new("ArchiveFilter")?;
        copy_dir(self.archive.path(), tempdir.path())?;
        Ok(tempdir)
    }
}

pub struct FileWriter {}

impl FileWriter {
    /// Copies all files from the input directory to the destination directory.
    pub fn new(dir: TempDir, destination: &Path) -> Result<FileWriter> {
        CopyBuilder::new(dir.path(), destination).overwrite(true).run()?;
        Ok(FileWriter {})
    }
}

impl Operator<()> for FileWriter {
    fn output(&self) -> Result<()> {
        Ok(())
    }
}
