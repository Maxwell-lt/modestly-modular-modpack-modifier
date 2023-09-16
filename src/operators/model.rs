use std::path::PathBuf;

use regex::Regex;
use serde::{Deserialize, Serialize};
use tempdir::TempDir;

use super::operators::Operator;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum OperatorConfig {
    // Literals
    URI { name: String, value: String },
    Path { name: String, value: String },
    Number { name: String, value: i64 },
    Text { name: String, value: String },
    Regex { name: String, value: String },
    // Operators
    ArchiveDownloader { name: String, uri: String },
    ArchiveFilter { name: String, archive: String, path_regex: String },
    FileWriter { name: String, archive: String, destination: String },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Operators {
    pub operators: Vec<OperatorConfig>,
}

impl OperatorConfig {
    pub fn get_name(&self) -> String {
        match self {
            Self::URI { name, .. } => name.to_owned(),
            Self::Path { name, .. } => name.to_owned(),
            Self::Number { name, .. } => name.to_owned(),
            Self::Text { name, .. } => name.to_owned(),
            Self::Regex { name, .. } => name.to_owned(),
            Self::ArchiveDownloader { name, .. } => name.to_owned(),
            Self::ArchiveFilter { name, .. } => name.to_owned(),
            Self::FileWriter { name, .. } => name.to_owned(),
        }
    }

    pub fn get_refs(&self) -> Vec<String> {
        match self {
            Self::URI { .. } => Vec::new(),
            Self::Path { .. } => Vec::new(),
            Self::Number { .. } => Vec::new(),
            Self::Text { .. } => Vec::new(),
            Self::Regex { .. } => Vec::new(),
            Self::ArchiveDownloader { uri, .. } => vec![uri.to_owned()],
            Self::ArchiveFilter { archive, path_regex, .. } => vec![archive.to_owned(), path_regex.to_owned()],
            Self::FileWriter { archive, destination, .. } => vec![archive.to_owned(), destination.to_owned()],
        }
    }
}

pub enum CalculatedOperator {
    Str(Box<dyn Operator<String>>),
    Number(Box<dyn Operator<i64>>),
    Path(Box<dyn Operator<PathBuf>>),
    Regex(Box<dyn Operator<Regex>>),
    Folder(Box<dyn Operator<TempDir>>),
    Terminal(Box<dyn Operator<()>>),
}
