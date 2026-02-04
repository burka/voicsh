use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "voicsh", version, about = "Voice typing for Wayland Linux")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Record audio and transcribe (one-shot mode)
    Record,
    /// List available audio devices
    Devices,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Record) => {
            println!("Recording... (not implemented)");
        }
        Some(Commands::Devices) => {
            println!("Listing devices... (not implemented)");
        }
        None => {
            println!("voicsh - Voice typing for Wayland Linux");
            println!("Run with --help for usage");
        }
    }
}
