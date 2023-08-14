use core::fmt;
use std::{str::FromStr, path::Path};

use anyhow::bail;

#[derive(Clone)]
pub(crate) struct FilePath {
    dirs: Vec<String>,
    name: String,
}

impl FilePath {
    pub(crate) fn get_filename(&self) -> &str {
        &self.name
    }

    /// Returns the name of the directory at provided recursion level,
    /// or [`None`] if the recursion level points to the filename or deeper.
    pub(crate) fn get_dir_at(&self, level: usize) -> Option<&str> {
        self.dirs.get(level).map(|s| s.as_ref())
    }
}

impl FromStr for FilePath {
    type Err = anyhow::Error;
    
    fn from_str(path: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = path
            .split("/")
            .filter(|s| !s.is_empty())
            .collect();
        if let Some((filename, directories)) = parts.split_last() {
            Ok(FilePath {
                name: filename.to_string(),
                dirs: directories.iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>(),
            })
        } else {
            bail!("Got empty path!");
        }
    }
}

impl TryFrom<&Path> for FilePath {
    type Error = anyhow::Error;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        FilePath::from_str(&value.to_string_lossy())
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for dir in self.dirs.iter() {
            write!(f, "{}/", dir)?;
        }
        write!(f, "{}", self.name)?;
        Ok(())
    }
}
