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

    pub(crate) fn get_components(&self) -> Vec<String> {
        self.dirs
            .iter()
            .chain(std::iter::once(&self.name))
            .map(String::to_owned)
            .collect::<Vec<String>>()
    }
}

impl FromStr for FilePath {
    type Err = anyhow::Error;
    
    fn from_str(path: &str) -> Result<Self, Self::Err> {
        if path.chars().last() == Some('/') {
            bail!("Got directory!");
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_blank_components() {
        let path = FilePath::from_str("//hello/world//directory/test.txt").unwrap();
        assert_eq!(path.to_string(), "hello/world/directory/test.txt");
    }

    #[test]
    fn rejects_empty_path() {
        let result = FilePath::from_str("/");
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn get_filename() {
        let path = FilePath::from_str("test/path/file").unwrap();
        assert_eq!(path.get_filename(), "file");
    }

    #[test]
    fn get_components() {
        let path = FilePath::from_str("test/path/file.txt").unwrap();
        assert_eq!(path.get_components(), vec!["test", "path", "file.txt"])
    }

    #[test]
    fn convert_from_path() {
        let path: &Path = Path::new("test/path/file.txt");
        let converted_path: FilePath = path.try_into().unwrap();
        assert_eq!(converted_path.to_string(), "test/path/file.txt");
    }

    #[test]
    fn reject_trailing_slash() {
        let result = FilePath::from_str("this/is/a/directory/");
        assert_eq!(result.is_err(), true);
    }
}
