use anyhow::Result;
use clap::Parser;
use std::io::IsTerminal;
use voicsh::app::run_record_command;
use voicsh::audio::capture::list_devices;
use voicsh::cli::{Cli, ModelsAction};
use voicsh::config::Config;
use voicsh::daemon::run_daemon;
use voicsh::diagnostics::check_dependencies;
use voicsh::ipc::client::send_command;
use voicsh::ipc::protocol::{Command, Response};
use voicsh::ipc::server::IpcServer;
use voicsh::models::catalog::list_models;
use voicsh::models::download::{download_model, format_model_info};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            let config = load_config(cli.config.as_deref())?;
            if std::io::stdin().is_terminal() {
                // Mic mode
                run_record_command(
                    config,
                    cli.device,
                    cli.model,
                    cli.language,
                    cli.quiet,
                    cli.verbose,
                    cli.no_download,
                    cli.once,
                    cli.fan_out,
                    cli.chunk_size,
                )
                .await?;
            } else {
                // Pipe mode: stdin has WAV data
                voicsh::app::run_pipe_command(
                    config,
                    cli.model,
                    cli.language,
                    cli.quiet,
                    cli.verbose,
                    cli.no_download,
                )
                .await?;
            }
        }
        Some(voicsh::cli::Commands::Devices) => {
            list_audio_devices()?;
        }
        Some(voicsh::cli::Commands::Models { action }) => {
            handle_models_command(action).await?;
        }
        Some(voicsh::cli::Commands::Check) => {
            check_dependencies();
        }
        Some(voicsh::cli::Commands::Daemon { socket }) => {
            let config = load_config(cli.config.as_deref())?;
            run_daemon(config, socket, cli.quiet, cli.verbose, cli.no_download).await?;
        }
        Some(voicsh::cli::Commands::Start { socket }) => {
            handle_ipc_command(socket, Command::Start).await?;
        }
        Some(voicsh::cli::Commands::Stop { socket }) => {
            handle_ipc_command(socket, Command::Stop).await?;
        }
        Some(voicsh::cli::Commands::Toggle { socket }) => {
            handle_ipc_command(socket, Command::Toggle).await?;
        }
        Some(voicsh::cli::Commands::Status { socket }) => {
            handle_ipc_command(socket, Command::Status).await?;
        }
        Some(voicsh::cli::Commands::InstallService) => {
            install_systemd_service()?;
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

/// Handle model management commands.
async fn handle_models_command(action: ModelsAction) -> Result<()> {
    match action {
        ModelsAction::List => {
            println!("Available models:");
            for model in list_models() {
                println!("  {}", format_model_info(model));
            }
        }
        ModelsAction::Install { name } => {
            let path = download_model(&name, true).await?;
            println!("Model '{}' installed successfully", name);
            println!("Location: {}", path.display());
        }
    }
    Ok(())
}

/// Send IPC command to daemon and handle response.
async fn handle_ipc_command(socket: Option<std::path::PathBuf>, command: Command) -> Result<()> {
    let socket_path = socket.unwrap_or_else(IpcServer::default_socket_path);

    match send_command(&socket_path, command).await {
        Ok(response) => match response {
            Response::Ok => {
                println!("OK");
            }
            Response::Transcription { text } => {
                println!("{}", text);
            }
            Response::Status {
                recording,
                model_loaded,
                model_name,
            } => {
                println!("Daemon status:");
                println!("  Recording: {}", if recording { "yes" } else { "no" });
                println!(
                    "  Model loaded: {}",
                    if model_loaded { "yes" } else { "no" }
                );
                if let Some(name) = model_name {
                    println!("  Model: {}", name);
                }
            }
            Response::Error { message } => {
                eprintln!("Error: {}", message);
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("Failed to communicate with daemon: {}", e);
            eprintln!("Is the daemon running? Start it with: voicsh daemon");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Install systemd user service.
fn install_systemd_service() -> Result<()> {
    use std::fs;
    use std::path::PathBuf;

    // Get systemd user directory
    let systemd_dir = if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(config_home).join("systemd/user")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/systemd/user")
    } else {
        anyhow::bail!("Could not determine user config directory");
    };

    // Create directory if it doesn't exist
    fs::create_dir_all(&systemd_dir)?;

    // Get current executable path
    let exe_path = std::env::current_exe()?;

    // Generate service file
    let service_content = format!(
        r#"[Unit]
Description=voicsh - Voice typing daemon
After=graphical-session.target

[Service]
Type=simple
ExecStart={} daemon
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
        exe_path.display()
    );

    // Write service file
    let service_path = systemd_dir.join("voicsh.service");
    fs::write(&service_path, service_content)?;

    println!("Systemd service installed to: {}", service_path.display());
    println!("\nTo enable and start the service:");
    println!("  systemctl --user daemon-reload");
    println!("  systemctl --user enable voicsh.service");
    println!("  systemctl --user start voicsh.service");
    println!("\nTo check status:");
    println!("  systemctl --user status voicsh.service");

    Ok(())
}
