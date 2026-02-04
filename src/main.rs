use anyhow::Result;
use clap::Parser;
use voicsh::audio::capture::list_devices;
use voicsh::cli::{Cli, Commands};
use voicsh::config::Config;
use voicsh::pipeline::run_record_command;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Record {
            device,
            model,
            language,
        } => {
            // Load configuration
            let config = load_config(cli.config.as_deref())?;

            // Run the record pipeline
            run_record_command(config, device, model, language, cli.quiet)?;
        }
        Commands::Devices => {
            list_audio_devices()?;
        }
        Commands::Start { foreground } => {
            if foreground {
                eprintln!("Starting daemon in foreground... (not implemented)");
            } else {
                eprintln!("Starting daemon... (not implemented)");
            }
            std::process::exit(1);
        }
        Commands::Stop => {
            eprintln!("Stopping daemon... (not implemented)");
            std::process::exit(1);
        }
        Commands::Toggle => {
            eprintln!("Toggling recording... (not implemented)");
            std::process::exit(1);
        }
        Commands::Status => {
            eprintln!("Daemon status... (not implemented)");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Load configuration from file or use defaults.
///
/// Priority order:
/// 1. Custom config path from CLI (--config)
/// 2. Default config path (~/.config/voicsh/config.toml)
/// 3. Built-in defaults with environment variable overrides
fn load_config(custom_path: Option<&std::path::Path>) -> Result<Config> {
    let config = if let Some(path) = custom_path {
        // Load from custom path
        Config::load(path)?
    } else {
        // Try default path, fall back to defaults
        let default_path = Config::default_path();
        Config::load_or_default(&default_path)
    };

    // Apply environment variable overrides
    Ok(config.with_env_overrides())
}

/// List available audio input devices.
fn list_audio_devices() -> Result<()> {
    let devices = list_devices()?;

    if devices.is_empty() {
        eprintln!("No audio input devices found");
        std::process::exit(1);
    }

    println!("Available audio input devices:");
    for (idx, device) in devices.iter().enumerate() {
        println!("  [{}] {}", idx, device);
    }

    Ok(())
}
