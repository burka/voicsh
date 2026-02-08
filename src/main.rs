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
                    cli.buffer,
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
                    cli.buffer,
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
        #[cfg(feature = "benchmark")]
        Some(voicsh::cli::Commands::Benchmark {
            audio,
            models,
            iterations,
            output,
        }) => {
            handle_benchmark_command(
                audio,
                models,
                iterations,
                &output,
                cli.no_download,
                cli.verbose,
            )
            .await?;
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

/// Handle benchmark command.
#[cfg(feature = "benchmark")]
async fn handle_benchmark_command(
    audio: Option<std::path::PathBuf>,
    models: Option<String>,
    iterations: usize,
    output: &str,
    no_download: bool,
    verbose: u8,
) -> Result<()> {
    use voicsh::benchmark::{
        BenchmarkReport, ResourceMonitor, SystemInfo, benchmark_model, load_wav_file,
        print_json_report, print_results,
    };
    use voicsh::models::catalog::MODELS;
    use voicsh::models::download::model_path;

    // Determine audio file
    let wav_file = if let Some(path) = audio {
        path.to_string_lossy().to_string()
    } else {
        // Try to use test fixture
        let fixture_path = "tests/fixtures/quick_brown_fox.wav";
        if std::path::Path::new(fixture_path).exists() {
            fixture_path.to_string()
        } else {
            eprintln!("Error: No audio file specified and test fixture not found");
            eprintln!();
            eprintln!("Usage: voicsh benchmark --audio <file.wav>");
            eprintln!();
            eprintln!("Or run from project root to use default test fixture:");
            eprintln!("  tests/fixtures/quick_brown_fox.wav");
            std::process::exit(1);
        }
    };

    // Determine models to test
    let model_list: Vec<String> = if let Some(models_str) = models {
        models_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    } else {
        // Use all installed models
        MODELS
            .iter()
            .filter_map(|m| {
                model_path(&m.name).and_then(|p| {
                    if p.exists() {
                        Some(m.name.to_string())
                    } else {
                        None
                    }
                })
            })
            .collect()
    };

    if model_list.is_empty() {
        eprintln!("Error: No models available for benchmarking");
        eprintln!();
        eprintln!("Install models with:");
        eprintln!("  voicsh models install <model-name>");
        eprintln!();
        eprintln!("Available models:");
        for model in MODELS.iter() {
            eprintln!("  {} ({}MB)", model.name, model.size_mb);
        }
        std::process::exit(1);
    }

    // Load audio file
    let (audio_samples, audio_duration_ms) = match load_wav_file(&wav_file) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to load WAV file: {}", e);
            eprintln!("File: {}", wav_file);
            std::process::exit(1);
        }
    };

    // Print compact benchmark header
    println!(
        "Benchmarking: {} ({:.1}s, {} samples)",
        wav_file,
        audio_duration_ms as f64 / 1000.0,
        audio_samples.len()
    );

    // Print system information
    let system_info = SystemInfo::detect();
    system_info.print_report(verbose);
    println!();

    // Run benchmarks
    let monitor = ResourceMonitor::new();
    let mut results = Vec::new();

    for model_name in &model_list {
        // Check if model exists
        let model_exists = model_path(model_name).map_or(false, |p| p.exists());

        if !model_exists {
            if no_download {
                eprintln!(
                    "Skipping {}: model not installed (--no-download)",
                    model_name
                );
                eprintln!("  Install with: voicsh models install {}", model_name);
                println!();
                continue;
            }

            // Auto-download missing model
            println!("Downloading model {}...", model_name);
            match download_model(model_name, true).await {
                Ok(_) => println!("Model {} downloaded successfully\n", model_name),
                Err(e) => {
                    eprintln!("Failed to download {}: {}", model_name, e);
                    eprintln!("  Try manually: voicsh models install {}", model_name);
                    println!();
                    continue;
                }
            }
        }

        // Print "Running {model}..." without newline
        print!("Running {}... ", model_name);
        std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());

        match benchmark_model(
            model_name,
            "auto", // Use auto language detection
            &audio_samples,
            audio_duration_ms,
            &monitor,
            iterations,
            verbose,
        ) {
            Ok(result) => {
                // Print time on the same line
                println!("{}ms", result.elapsed_ms);

                // Only show transcription with verbose >= 1
                if verbose >= 1 {
                    println!(
                        "  \"{}\" [confidence: {:.2}]",
                        result.transcription.trim(),
                        result.confidence
                    );
                }

                results.push(result);
            }
            Err(e) => {
                println!("FAILED");
                eprintln!("  Error: {}", e);
            }
        }
    }

    if results.is_empty() {
        eprintln!("No models were benchmarked successfully.");
        std::process::exit(1);
    }

    // Output results in requested format
    if output == "json" {
        let report = BenchmarkReport {
            system_info,
            audio_file: wav_file,
            audio_samples: audio_samples.len(),
            audio_duration_ms,
            iterations,
            results,
        };
        print_json_report(&report);
    } else {
        print_results(&results, verbose);
    }

    Ok(())
}
