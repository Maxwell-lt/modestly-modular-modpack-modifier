use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum Operator {
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
    pub operators: Vec<Operator>,
}
