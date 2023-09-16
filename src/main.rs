use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::File;

use anyhow::{bail, Context, Result};
use operators::model::Operators;
use options::Options;

use crate::operators::model::{CalculatedOperator, OperatorConfig};
use crate::operators::operators::*;

mod di;
mod filetree;
mod node;
mod operators;
mod options;

fn main() -> Result<()> {
    let opt: Options = options::parse();
    let config_map: HashMap<String, OperatorConfig> = serde_yaml::from_reader::<File, Operators>(
        File::open(&opt.config_file).context(format!("Failed to open config file {}", opt.config_file.to_string_lossy()))?,
    )
    .context("Failed to parse YAML")?
    .operators
    .into_iter()
    .map(|o| (o.get_name(), o))
    .collect();

    let node_order = topological_sort(&config_map)?;

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
                let uri_operator = calculated_operators
                    .get(uri)
                    .context(format!("Operator {} not calculated yet, but operator {} depends on it!", uri, name))?;
                if let CalculatedOperator::Str(uri) = uri_operator {
                    calculated_operators.insert(
                        name.to_owned(),
                        CalculatedOperator::Folder(Box::new(ArchiveDownloader::new(&uri.output()?)?)),
                    );
                } else {
                    bail!(format!("Operator {} expected to be type URI!", uri));
                }
            },
            OperatorConfig::ArchiveFilter { name, archive, path_regex } => {
                let archive_operator = calculated_operators
                    .get(archive)
                    .context(format!("Operator {} not calculated yet, but operator {} depends on it!", archive, name))?;
                let path_regex_operator = calculated_operators.get(path_regex).context(format!(
                    "Operator {} not calculated yet, but operator {} depends on it!",
                    path_regex, name
                ))?;
                if let (CalculatedOperator::Folder(path), CalculatedOperator::Regex(regex)) = (archive_operator, path_regex_operator) {
                    calculated_operators.insert(
                        name.to_owned(),
                        CalculatedOperator::Folder(Box::new(ArchiveFilter::new(regex.output()?, path.output()?)?)),
                    );
                } else {
                    bail!(format!(
                        "One of these operators expected to be a different type: {}, {}",
                        archive, path_regex
                    ));
                }
            },
            OperatorConfig::FileWriter { name, archive, destination } => {
                let archive_operator = calculated_operators
                    .get(archive)
                    .context(format!("Operator {} not calculated yet, but operator {} depends on it!", archive, name))?;
                let destination_operator = calculated_operators.get(destination).context(format!(
                    "Operator {} not calculated yet, but operator {} depends on it!",
                    destination, name
                ))?;
                if let (CalculatedOperator::Folder(src), CalculatedOperator::Path(dest)) = (archive_operator, destination_operator) {
                    calculated_operators.insert(
                        name.to_owned(),
                        CalculatedOperator::Terminal(Box::new(FileWriter::new(src.output()?, &dest.output()?)?)),
                    );
                } else {
                    bail!(format!(
                        "One of these operators expected to be a different type: {}, {}",
                        archive, destination
                    ));
                }
            },
            _ => {
                bail!("Hit unimplemented operator!");
            },
        };
    }

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
            matrix[i][indices.binary_search(&edge).ok().context(format!(
                "Failed to build adjacency matrix, node with name {} referenced by {} does not exist",
                edge, indices[i]
            ))?] = true;
        }
    }

    Ok(matrix)
}

fn topological_sort(config_map: &HashMap<String, OperatorConfig>) -> Result<Vec<String>> {
    // Build graph representations
    let adjacency_list: BTreeMap<String, Vec<String>> = config_map.iter().map(|(k, v)| (k.to_owned(), v.get_refs())).collect();
    let mut matrix = create_adjacency_matrix(&adjacency_list)?;

    // Find operators with no dependencies (indegree 0)
    let root_nodes: Vec<String> = matrix
        .iter()
        .zip(adjacency_list.keys())
        .filter(|(row, _name)| row.iter().all(|cell| cell == &false))
        .map(|(_row, name)| name.to_owned())
        .collect();

    // Index of node names in same order as adjacency_list and matrix
    let indices: Vec<&String> = adjacency_list.keys().collect();
    let operator_count = config_map.len();
    let mut nodes_in_order: Vec<String> = Vec::with_capacity(operator_count);
    let mut s = VecDeque::from_iter(root_nodes);
    loop {
        if let Some(node) = s.pop_front() {
            nodes_in_order.push(node.to_owned());
            let col = indices.binary_search(&&node).expect("Name not found in adjacency list!");
            for row in 0..operator_count {
                if matrix[row][col] {
                    let m = indices[row];
                    matrix[row][col] = false;
                    if matrix[row].iter().all(|x| *x == false) {
                        s.push_back(m.to_owned());
                    }
                }
            }
        } else {
            break;
        }
    }

    Ok(nodes_in_order)
}
