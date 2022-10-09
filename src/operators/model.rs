use serde::{Serialize, Deserialize};

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
            Self::URI { name, value } => name.to_owned(),
            Self::Path { name, value } => name.to_owned(),
            Self::Number { name, value } => name.to_owned(),
            Self::Text { name, value } => name.to_owned(),
            Self::Regex { name, value } => name.to_owned(),
            Self::ArchiveDownloader { name, uri } => name.to_owned(),
            Self::ArchiveFilter { name, archive, path_regex } => name.to_owned(),
            Self::FileWriter { name, archive, destination } => name.to_owned(),
        }
    }

    pub fn get_refs(&self) -> Vec<String> {
        match self {
            Self::URI { name, value } => Vec::new(),
            Self::Path { name, value } => Vec::new(),
            Self::Number { name, value } => Vec::new(),
            Self::Text { name, value } => Vec::new(),
            Self::Regex { name, value } => Vec::new(),
            Self::ArchiveDownloader { name, uri } => vec![uri.to_owned()],
            Self::ArchiveFilter { name, archive, path_regex } => vec![archive.to_owned(), path_regex.to_owned()],
            Self::FileWriter { name, archive, destination } => vec![archive.to_owned(), destination.to_owned()],
        }
    }
}
