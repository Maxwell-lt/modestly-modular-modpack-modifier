use std::collections::{HashMap, BTreeMap};
use std::fs::File;

use anyhow::{Context, Result};
use operators::model::Operators;
use options::Options;
use walkdir::WalkDir;

use crate::operators::model::OperatorConfig;
use crate::operators::operators::*;


mod operators;
mod options;

fn main() -> Result<()> {
    let opt: Options = options::parse();
    let config: Operators = serde_yaml::from_reader(
        File::open(&opt.config_file).context(format!("Failed to open config file {}", opt.config_file.to_string_lossy()))?)
        .context("Failed to parse YAML")?;

    println!("{:?}", config);

    let config_map: HashMap<String, OperatorConfig> = config.operators.
        into_iter()
        .map(|o| (o.get_name(), o))
        .collect();

    let adjacency_list: BTreeMap<String, Vec<String>> = config_map.iter()
        .map(|(k, v)| (k.to_owned(), v.get_refs()))
        .collect();

    let adjacency_matrix = create_adjacency_matrix(&adjacency_list)?;

    println!("{:?}\n{:?}", adjacency_list, adjacency_matrix);
    
    //let uri = URILiteral::new("https://cdn.modrinth.com/data/p87Jiw2q/versions/6D8o98Bp/LostEra_modpack_1.5.2a.mrpack".to_string());
    //let regex = RegexLiteral::new("modrinth\\.index\\.json|overrides/config/NuclearCraft/ToolConfig\\.cfg".to_string())?;

    //let downloader = ArchiveDownloader::new(uri.output()?)?;
    //let filter = ArchiveFilter::new(regex.output()?, downloader.output()?)?;

    //println!("Downloaded files:");
    //let tempdir = downloader.output()?;
    //for file in WalkDir::new(tempdir.path()) {
    //    let file = file?;
    //    println!("{}", file.path().to_string_lossy());
    //}

    //println!("Filtered files:");
    //let tempdir = filter.output()?;
    //for file in WalkDir::new(tempdir.path()) {
    //    let file = file?;
    //    println!("{}", file.path().to_string_lossy());
    //}

    //let outpath = PathLiteral::new("./output".to_string());
    //println!("{}", outpath.output()?.as_path().canonicalize()?.to_string_lossy());
    //FileWriter::new(filter.output()?, &outpath.output()?)?;

    Ok(())
}

fn create_adjacency_matrix(map: &BTreeMap<String, Vec<String>>) -> Result<Vec<Vec<bool>>> {
    let size = map.len();
    let mut matrix: Vec<Vec<bool>> = Vec::with_capacity(size);
    for _ in 0..size {
        matrix.push(vec![false; size]);
    }

    let indices: Vec<&String> = map.keys().collect();
    for (i, edges) in map.values().enumerate() {
        for edge in edges {
            matrix[i][indices
                .binary_search(&edge).ok()
                .context(format!("Failed to build adjacency matrix, node with name {} referenced by {} does not exist", edge, indices[i]))?
            ] = true;
        }
    }

    Ok(matrix)
}
