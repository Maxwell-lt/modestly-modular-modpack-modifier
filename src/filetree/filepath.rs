use core::fmt;
use glob_match::glob_match;
use std::hash::{Hash, Hasher};
use std::{path::Path, str::FromStr};
use thiserror::Error;

#[derive(Debug, Clone, Eq)]
pub(crate) struct FilePath {
    dirs: Vec<String>,
    name: String,
    cached_path: String,
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

    pub(crate) fn glob_match<T: AsRef<str>>(&self, patterns: &[T]) -> bool {
        patterns.iter().any(|pattern| glob_match(pattern.as_ref(), &self.cached_path))
    }
}

impl FromStr for FilePath {
    type Err = FilePathError;

    fn from_str(path: &str) -> Result<Self, Self::Err> {
        if path.chars().last() == Some('/') {
            return Err(FilePathError::DirectoryError(path.to_string()));
        }
        if path.chars().nth(0) == Some('/') {
            return Err(FilePathError::AbsolutePathError(path.to_string()));
        }
        let parts: Vec<&str> = path.split("/").filter(|s| !s.is_empty()).collect();
        if let Some((filename, directories)) = parts.split_last() {
            Ok(FilePath {
                name: filename.to_string(),
                dirs: directories.iter().map(|s| s.to_string()).collect::<Vec<String>>(),
                cached_path: parts.join("/"),
            })
        } else {
            return Err(FilePathError::PathEmptyError(path.to_string()));
        }
    }
}

impl TryFrom<&Path> for FilePath {
    type Error = FilePathError;

    fn try_from(value: &Path) -> Result<Self, Self::Error> {
        FilePath::from_str(&value.to_string_lossy())
    }
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.cached_path)?;
        Ok(())
    }
}

impl PartialEq for FilePath {
    fn eq(&self, other: &Self) -> bool {
        self.cached_path == other.cached_path
    }
}

impl Hash for FilePath {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.cached_path.hash(state);
    }
}

#[derive(Error, Debug, PartialEq)]
pub enum FilePathError {
    #[error("provided path string resolved to an empty path (got path \"{0}\")")]
    PathEmptyError(String),
    #[error("provided path string resolved to a directory (got path \"{0}\")")]
    DirectoryError(String),
    #[error("provided path string is an absolute path (got path \"{0}\"")]
    AbsolutePathError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_blank_components() {
        let path = FilePath::from_str("empty///components.txt").unwrap();
        assert_eq!(path.to_string(), "empty/components.txt");
    }

    #[test]
    fn rejects_empty_path() {
        let result = FilePath::from_str("");
        assert_eq!(result.unwrap_err(), FilePathError::PathEmptyError("".to_string()));
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
        assert_eq!(result.unwrap_err(), FilePathError::DirectoryError("this/is/a/directory/".to_string()));
    }

    // I don't care all that much about Windows absolute paths at the moment.
    #[test]
    fn reject_unix_absolute_path() {
        let result = FilePath::from_str("/etc/passwd");
        assert_eq!(result.unwrap_err(), FilePathError::AbsolutePathError("/etc/passwd".to_string()));
    }

    #[test]
    fn matches_glob() {
        let path = FilePath::from_str("test/path/containing/a/file.json").unwrap();
        assert!(path.glob_match(&["test/**/*.json"]));
        assert!(!path.glob_match(&["test/**/*.txt"]));
        assert!(path.glob_match(&["test/**", "does/not/match/**"]));
    }
}
