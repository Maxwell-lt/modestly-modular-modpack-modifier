use std::fs::File;

use anyhow::Result;
use operators::model::Operators;
use operators::operators::Operator;
use options::Options;
use walkdir::WalkDir;

use crate::operators::operators::*;


mod operators;
mod options;

fn main() -> Result<()> {
    let opt: Options = options::parse();
    let config: Operators = serde_yaml::from_reader(File::open(&opt.config_file)?)?;

    println!("{:?}", config);
    
    let uri = URILiteral::new("https://cdn.modrinth.com/data/p87Jiw2q/versions/6D8o98Bp/LostEra_modpack_1.5.2a.mrpack".to_string());
    let regex = RegexLiteral::new("modrinth\\.index\\.json|overrides/config/NuclearCraft/ToolConfig\\.cfg".to_string())?;

    let downloader = ArchiveDownloader::new(uri.output()?)?;
    let filter = ArchiveFilter::new(regex.output()?, downloader.output()?)?;

    println!("Downloaded files:");
    let tempdir = downloader.output()?;
    for file in WalkDir::new(tempdir.path()) {
        let file = file?;
        println!("{}", file.path().to_string_lossy());
    }

    println!("Filtered files:");
    let tempdir = filter.output()?;
    for file in WalkDir::new(tempdir.path()) {
        let file = file?;
        println!("{}", file.path().to_string_lossy());
    }

    let outpath = PathLiteral::new("./output".to_string());
    println!("{}", outpath.output()?.as_path().canonicalize()?.to_string_lossy());
    FileWriter::new(filter.output()?, &outpath.output()?)?;

    Ok(())
}
