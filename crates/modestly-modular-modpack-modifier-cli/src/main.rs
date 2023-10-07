use std::{fs, io::Write, path::PathBuf, thread, time::Duration};

use clap::Parser;
use color_eyre::{
    eyre::{Context, Result},
    Section,
};
use mmmm_core::orch::MMMMConfig;
use tokio::sync::broadcast::error::TryRecvError;
use tracing_error::ErrorLayer;
use tracing_indicatif::{writer::get_indicatif_stderr_writer, IndicatifLayer};
use tracing_subscriber::{filter::LevelFilter, prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, Layer};

fn main() -> Result<()> {
    color_eyre::install()?;
    let indicatif_layer = IndicatifLayer::new();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(indicatif_layer.get_stderr_writer())
                .with_filter(LevelFilter::INFO),
        )
        .with(indicatif_layer)
        .with(ErrorLayer::default())
        .init();

    let args = Args::parse();
    let pack_def = fs::read_to_string(&args.definition)
        .wrap_err_with(|| format!("Failed to read pack definition YAML from {}", args.definition.to_string_lossy()))
        .suggestion("Provide a valid path to a pack definition YAML file")?;
    let global_config: MMMMConfig = fs::read_to_string(match args.config_dir {
        Some(dir) => dir.join("mmmm.toml"),
        None => "mmmm.toml".into(),
    })
    .ok()
    .and_then(|s| toml::from_str(s.as_ref()).ok())
    .unwrap_or_default();
    let mut graph = mmmm_core::orch::build_graph(&pack_def, global_config)
        .wrap_err("Failed to construct node graph")
        .suggestion("Confirm that the pack definition is valid")?;
    graph
        .context
        .run()
        .wrap_err("Failed to trigger processing")
        .suggestion("Report this as a bug")?;

    let output_dir = match args.output_dir {
        Some(dir) => dir.canonicalize().wrap_err("Failed to get absolute output path")?,
        None => std::env::current_dir()
            .wrap_err("Could not get current working directory")
            .suggestion("Specify config and output paths")?,
    };

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
        file_outputs.retain_mut(|channel| match channel.1.try_recv() {
            Ok(data) => {
                let out_path = output_dir.join::<PathBuf>(channel.0.clone().into());
                println!("Output ready, writing to {}", out_path.to_string_lossy());
                fs::write(&out_path, data)
                    .wrap_err(format!("Could not write to file {}", out_path.to_string_lossy()))
                    .suggestion("Ensure the parent directory exists, and that the current user has write access to it")
                    .unwrap();
                println!("Finished writing to {}", out_path.to_string_lossy());
                false
            },
            Err(TryRecvError::Closed) => false,
            _ => true,
        });

        zip_outputs.retain_mut(|channel| match channel.1.try_recv() {
            Ok(data) => {
                let out_path = output_dir.join::<PathBuf>(channel.0.clone().into()).with_extension("zip");
                writeln!(
                    get_indicatif_stderr_writer().unwrap(),
                    "Output ready, writing to {}",
                    out_path.to_string_lossy()
                )
                .unwrap();
                let mut out_file = fs::File::create(&out_path)
                    .wrap_err(format!("Could not write to file {}", out_path.to_string_lossy()))
                    .suggestion("Ensure the parent directory exists, and that the current user has write access to it")
                    .unwrap();
                let bytes = data.zip(&mut out_file).wrap_err("Failed to write to file buffer").unwrap();
                writeln!(
                    get_indicatif_stderr_writer().unwrap(),
                    "Finished writing to {}. Wrote {} bytes.",
                    out_path.to_string_lossy(),
                    bytes
                )
                .unwrap();
                false
            },
            Err(TryRecvError::Closed) => false,
            _ => true,
        });

        // If all outputs have been read, break from loop.
        if zip_outputs.is_empty() && file_outputs.is_empty() {
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
