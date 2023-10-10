use std::{fs, io::Write, path::PathBuf, thread, time::Duration};

use cache::SqliteCache;
use clap::Parser;
use color_eyre::{
    eyre::{eyre, Context, Result},
    Section,
};
use directories::ProjectDirs;
use mmmm_core::orch::MMMMConfig;
use tokio::sync::broadcast::error::TryRecvError;
use tracing::{event, span, Level};
use tracing_error::ErrorLayer;
use tracing_indicatif::{writer::get_indicatif_stderr_writer, IndicatifLayer};
use tracing_subscriber::{filter::LevelFilter, prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, Layer};

mod cache;

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
        .wrap_err_with(|| format!("Failed to read pack definition YAML from {}", args.definition.display()))
        .suggestion("Provide a valid path to a pack definition YAML file")?;
    let global_config: MMMMConfig = get_config(args.config_dir)?;
    let project_dirs = get_project_dirs()?;
    let cache_dir = project_dirs.cache_dir();
    let cache = SqliteCache::new(cache_dir, args.clear_cache)?;
    let mut graph = mmmm_core::orch::build_graph(&pack_def, global_config, Some(Box::new(cache)))
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
                println!("Output ready, writing to {}", out_path.display());
                fs::write(&out_path, data)
                    .wrap_err(format!("Could not write to file {}", out_path.display()))
                    .suggestion("Ensure the parent directory exists, and that the current user has write access to it")
                    .unwrap();
                println!("Finished writing to {}", out_path.display());
                false
            },
            Err(TryRecvError::Closed) => false,
            _ => true,
        });

        zip_outputs.retain_mut(|channel| match channel.1.try_recv() {
            Ok(data) => {
                let out_path = output_dir.join::<PathBuf>(channel.0.clone().into()).with_extension("zip");
                writeln!(get_indicatif_stderr_writer().unwrap(), "Output ready, writing to {}", out_path.display()).unwrap();
                let mut out_file = fs::File::create(&out_path)
                    .wrap_err(format!("Could not write to file {}", out_path.display()))
                    .suggestion("Ensure the parent directory exists, and that the current user has write access to it")
                    .unwrap();
                let bytes = data.zip(&mut out_file).wrap_err("Failed to write to file buffer").unwrap();
                writeln!(
                    get_indicatif_stderr_writer().unwrap(),
                    "Finished writing to {}. Wrote {} bytes.",
                    out_path.display(),
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

fn get_config(override_dir: Option<PathBuf>) -> Result<MMMMConfig> {
    let _span = span!(Level::DEBUG, "get_config").entered();
    if let Some(dir) = override_dir {
        event!(Level::INFO, "Loading mmmm.toml from user-provided directory");
        let data = fs::read_to_string(dir.join("mmmm.toml"))
            .wrap_err("Failed to read from config file")
            .with_suggestion(|| format!("Ensure that the file {}/mmmm.toml exists and is readable.", dir.display()))?;
        return toml::from_str(&data).wrap_err("Failed to parse config file");
    }
    let project_dirs = get_project_dirs()?;
    let config_dir = project_dirs.config_dir();
    let config_path = config_dir.join("mmmm.toml");
    let data = fs::read_to_string(&config_path);
    match data {
        Ok(config_contents) => {
            event!(Level::INFO, "Loading mmmm.toml from user config directory");
            toml::from_str(&config_contents).wrap_err("Failed to parse config file")
        },
        Err(_) => {
            event!(Level::INFO, "Creating example mmmm.toml in user config directory");
            fs::create_dir_all(config_dir).wrap_err_with(|| format!("Failed to initialize config directory {}", config_dir.display()))?;
            fs::write(&config_path, "# Set one of these keys to enable the Curse client\n# Curse API key from https://console.curseforge.com/#/api-keys\n#curse_api_key = \"\"\n# Base URL of a Curse API proxy service\n#curse_proxy_url = \"\"").wrap_err_with(|| format!("Failed to write example config file to {}", config_path.display()))?;

            Ok(MMMMConfig::default())
        },
    }
}

fn get_project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("dev", "maxwell-lt", "modestly-modular-modpack-modifier").ok_or_else(|| eyre!("Could not find user config directory!"))
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
    /// Directory where the configuration file is located.
    /// Default on Linux is $XDG_CONFIG_HOME/modestly-modular-modpack-modifier or
    /// $HOME/.config/modestly-modular-modpack-modifier.
    /// Default on Windows is %AppData%\maxwell-lt\modestly-modular-modpack-modifier\config.
    #[arg(short, long)]
    config_dir: Option<PathBuf>,
    /// Clear all cached data before running.
    #[arg(long)]
    clear_cache: bool,
}
