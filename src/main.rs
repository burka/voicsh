use clap::Parser;
use voicsh::cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    if cli.quiet {
        // TODO: Set up quiet mode (suppress output)
    }

    if let Some(config_path) = &cli.config {
        // TODO: Load config from custom path
        println!("Using config: {}", config_path.display());
    }

    match cli.command {
        Commands::Record {
            device,
            model,
            language,
        } => {
            println!("Recording... (not implemented)");
            if let Some(d) = device {
                println!("  Device: {}", d);
            }
            if let Some(m) = model {
                println!("  Model: {}", m);
            }
            if let Some(l) = language {
                println!("  Language: {}", l);
            }
        }
        Commands::Devices => {
            println!("Listing devices... (not implemented)");
        }
        Commands::Start { foreground } => {
            if foreground {
                println!("Starting daemon in foreground... (not implemented)");
            } else {
                println!("Starting daemon... (not implemented)");
            }
        }
        Commands::Stop => {
            println!("Stopping daemon... (not implemented)");
        }
        Commands::Toggle => {
            println!("Toggling recording... (not implemented)");
        }
        Commands::Status => {
            println!("Daemon status... (not implemented)");
        }
    }
}
