use std::path::PathBuf;

use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(about = "Modular modpack assembler.")]
/// Modular modpack assembler.
///
/// This application is used to assemble modpacks by using pre-defined functions chained
/// into a DAG using a YAML config file.
pub struct Options {
    /// Path to the YAML config file
    pub config_file: PathBuf,
}

pub fn parse() -> Options {
    Options::from_args()
}
