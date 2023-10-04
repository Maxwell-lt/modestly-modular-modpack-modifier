use std::{path::PathBuf, fs, thread, time::Duration};

use clap::Parser;
use color_eyre::eyre::Result;
use mmmm_core::{orch::MMMMConfig, logger::LogLevel};

fn main() -> Result<()> {
    let args = Args::parse();
    let pack_def = fs::read_to_string(args.definition)?;
    let global_config: MMMMConfig = toml::from_str(&fs::read_to_string(match args.config_dir {
        Some(dir) => dir.join("mmmm.toml"),
        None => "mmmm.toml".into(),
    })?)?;
    let mut graph = mmmm_core::orch::build_graph(&pack_def, global_config)?;
    graph.context.run()?;

    let output_dir = match args.output_dir {
        Some(dir) => dir,
        None => std::env::current_dir()?,
    };

    let logger = graph.context.get_logger();
    let mut file_outputs = vec![];
    let mut zip_outputs = vec![];
    for output in graph.outputs {
        match output.1 {
            mmmm_core::OutputType::Text(channel) => file_outputs.push((output.0, channel)),
            mmmm_core::OutputType::Files(channel) => zip_outputs.push((output.0, channel)),
            _ => {},
        }
    }
    let tick_rate = Duration::from_millis(100);
    loop {
        if let Some(log) = logger.get_logs().find(|l| l.level == LogLevel::Panic) {
            println!("Node '{}' failed with message {} and data {:?}!", log.source, log.message, log.data);
            panic!();
        }

        file_outputs.retain_mut(|channel| {
            if let Ok(data) = channel.1.try_recv() {
                let out_path = output_dir.join::<PathBuf>(channel.0.clone().into());
                println!("Output ready, writing to {}", out_path.to_string_lossy());
                fs::write(&out_path, data).unwrap();
                println!("Finished writing to {}", out_path.to_string_lossy());
                return false;
            }
            return true;
        });

        zip_outputs.retain_mut(|channel| {
            if let Ok(data) = channel.1.try_recv() {
                let out_path = output_dir.join::<PathBuf>(channel.0.clone().into()).with_extension("zip");
                println!("Output ready, writing to {}", out_path.to_string_lossy());
                let mut out_file = fs::File::create(&out_path).unwrap();
                let bytes = data.zip(&mut out_file).unwrap();
                println!("Finished writing to {}. Wrote {} bytes.", out_path.to_string_lossy(), bytes);
                return false;
            }
            return true;
        });

        if zip_outputs.len() == 0 && file_outputs.len() == 0 {
            break;
        }
        
        thread::sleep(tick_rate);
    }
    Ok(())
}

/// CLI frontend for Modestly Modular Modpack Modifier
///
/// Build modpacks by declaring a graph of processing nodes
#[derive(Parser, Debug)]
#[command(author, version)]
struct Args {
    /// Path to pack definition YAML file.
    definition: PathBuf,
    /// Directory where output files should be written. Default is current directory.
    #[arg(short, long)]
    output_dir: Option<PathBuf>,
    /// Directory where configurations files should be stored. Default is current directory.
    #[arg(short, long)]
    config_dir: Option<PathBuf>,
}
