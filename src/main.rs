use std::collections::{HashMap, BTreeMap, VecDeque, HashSet};
use std::fs::File;

use anyhow::{Context, Result, bail};
use operators::model::Operators;
use options::Options;

use crate::operators::model::{OperatorConfig, CalculatedOperator};
use crate::operators::operators::*;


mod operators;
mod options;

fn main() -> Result<()> {
    let opt: Options = options::parse();
    let config: Operators = serde_yaml::from_reader(
        File::open(&opt.config_file).context(format!("Failed to open config file {}", opt.config_file.to_string_lossy()))?)
        .context("Failed to parse YAML")?;

    let operator_count = config.operators.len();

    let config_map: HashMap<String, OperatorConfig> = config.operators.
        into_iter()
        .map(|o| (o.get_name(), o))
        .collect();

    let adjacency_list: BTreeMap<String, Vec<String>> = config_map.iter()
        .map(|(k, v)| (k.to_owned(), v.get_refs()))
        .collect();

    let adjacency_matrix = create_adjacency_matrix(&adjacency_list)?;

    // Find operators with no dependencies
    let root_nodes: Vec<String> = adjacency_matrix.iter()
        .zip(adjacency_list.keys())
        .filter(|(row, _name)| row.iter().all(|cell| cell == &false))
        .map(|(_row, name)| name.to_owned())
        .collect();

    // BFS over the transpose of the graph
    let node_order: Vec<String> = {
        let indices: Vec<&String> = adjacency_list.keys().collect();
        let mut nodes_in_order: Vec<String> = Vec::with_capacity(operator_count);
        let mut visited_nodes: HashSet<String> = HashSet::with_capacity(operator_count);
        let mut to_visit = VecDeque::from_iter(root_nodes);
        loop {
            let popped_node = to_visit.pop_front();
            if let Some(node) = popped_node {
                if !visited_nodes.contains(&node) {
                    if visited_nodes.is_superset(&HashSet::from_iter(adjacency_list
                                                                     .get(&node)
                                                                     .context("Name not found in adjacency list!")?
                                                                     .iter()
                                                                     .map(|s| s.to_owned()))) {
                        nodes_in_order.push(node.to_owned());
                        visited_nodes.insert(node.to_owned());
                        let matrix_index = indices.binary_search(&&node).expect("Name not found in adjacency list!");
                        for edge in 0..operator_count {
                            // Swapping col and row to search the transpose of the adjacency matrix,
                            // which is equivalent to the transpose of the digraph (all edges
                            // have inverted direction)
                            if adjacency_matrix[edge][matrix_index] {
                                let name = *indices.get(edge).context("Tried to find a node by index in alphabetical order")?;
                                if !visited_nodes.contains(name) && !to_visit.contains(name) {
                                    to_visit.push_back(name.to_owned());
                                }
                            }
                        }
                    } else {
                        to_visit.push_back(node);
                    }
                }
            } else {
                break;
            }
        }
        nodes_in_order
    };

    // Run operators
    let mut calculated_operators: HashMap<String, CalculatedOperator> = HashMap::new();
    for node in node_order {
        let config: &OperatorConfig = config_map.get(&node).context("Node not found in map!")?;
        println!("Executing step: {:?}", config);
        match config {
            OperatorConfig::URI { name, value } => {
                calculated_operators.insert(name.to_owned(), CalculatedOperator::Str(Box::new(URILiteral::new(value))));
            },
            OperatorConfig::Path { name, value } => {
                calculated_operators.insert(name.to_owned(), CalculatedOperator::Path(Box::new(PathLiteral::new(value))));
            },
            OperatorConfig::Regex { name, value } => {
                calculated_operators.insert(name.to_owned(), CalculatedOperator::Regex(Box::new(RegexLiteral::new(value)?)));
            },
            OperatorConfig::ArchiveDownloader { name, uri } => {
                let uri_operator = calculated_operators.get(uri)
                    .context(format!("Operator {} not calculated yet, but operator {} depends on it!", uri, name))?;
                if let CalculatedOperator::Str(uri) = uri_operator {
                    calculated_operators.insert(name.to_owned(), CalculatedOperator::Folder(Box::new(ArchiveDownloader::new(&uri.output()?)?)));
                } else {
                    bail!(format!("Operator {} expected to be type URI!", uri));
                }
            },
            OperatorConfig::ArchiveFilter { name, archive, path_regex } => {
                let archive_operator = calculated_operators.get(archive)
                    .context(format!("Operator {} not calculated yet, but operator {} depends on it!", archive, name))?;
                let path_regex_operator = calculated_operators.get(path_regex)
                    .context(format!("Operator {} not calculated yet, but operator {} depends on it!", path_regex, name))?;
                if let (CalculatedOperator::Folder(path), CalculatedOperator::Regex(regex)) = (archive_operator, path_regex_operator) {
                    calculated_operators.insert(name.to_owned(),
                    CalculatedOperator::Folder(Box::new(ArchiveFilter::new(regex.output()?, path.output()?)?)));
                } else {
                    bail!(format!("One of these operators expected to be a different type: {}, {}", archive, path_regex));
                }
            },
            OperatorConfig::FileWriter { name, archive, destination } => {
                let archive_operator = calculated_operators.get(archive)
                    .context(format!("Operator {} not calculated yet, but operator {} depends on it!", archive, name))?;
                let destination_operator = calculated_operators.get(destination)
                    .context(format!("Operator {} not calculated yet, but operator {} depends on it!", destination, name))?;
                if let (CalculatedOperator::Folder(src), CalculatedOperator::Path(dest)) = (archive_operator, destination_operator) {
                    calculated_operators.insert(name.to_owned(),
                    CalculatedOperator::Terminal(Box::new(FileWriter::new(src.output()?, &dest.output()?)?)));
                } else {
                    bail!(format!("One of these operators expected to be a different type: {}, {}", archive, destination));
                }
            },
            _ => {
                bail!("Hit unimplemented operator!");
            },
        };
    }
    
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
