use anyhow::Result;
use clap::{CommandFactory, Parser};
use owo_colors::OwoColorize;
use std::io::IsTerminal;
use voicsh::app::run_record_command;
use voicsh::audio::capture::list_devices;
use voicsh::cli::{Cli, ConfigAction, ModelsAction};
use voicsh::config::Config;
use voicsh::daemon::run_daemon;
use voicsh::diagnostics::check_dependencies;
use voicsh::ipc::client::send_command;
use voicsh::ipc::protocol::{Command, Response};
use voicsh::ipc::server::IpcServer;
use voicsh::models::catalog::{get_model, list_models, resolve_name};
use voicsh::models::download::{download_model, format_model_info, is_model_installed};

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
                    cli.injection_backend,
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
            handle_models_command(action, cli.config.as_deref()).await?;
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
        Some(voicsh::cli::Commands::Follow { socket }) => {
            handle_follow(socket).await?;
        }
        Some(voicsh::cli::Commands::InstallService) => {
            voicsh::systemd::install_and_activate()?;
        }
        Some(voicsh::cli::Commands::InstallGnomeExtension) => {
            voicsh::gnome_extension::install_gnome_extension()?;
        }
        Some(voicsh::cli::Commands::UninstallGnomeExtension) => {
            voicsh::gnome_extension::uninstall_gnome_extension()?;
        }
        Some(voicsh::cli::Commands::Config { action }) => {
            handle_config_command(action, cli.config.as_deref())?;
        }
        Some(voicsh::cli::Commands::Completions { shell }) => {
            clap_complete::generate(
                shell,
                &mut voicsh::cli::Cli::command(),
                "voicsh",
                &mut std::io::stdout(),
            );
        }
        #[cfg(feature = "benchmark")]
        Some(voicsh::cli::Commands::Benchmark {
            audio,
            models,
            iterations,
            output,
            threads,
        }) => {
            handle_benchmark_command(
                audio,
                models,
                iterations,
                &output,
                cli.no_download,
                cli.verbose,
                threads,
            )
            .await?;
        }
        #[cfg(feature = "benchmark")]
        Some(voicsh::cli::Commands::AutoTune {
            language,
            allow_quantized,
        }) => {
            voicsh::init::run_init(&language, cli.verbose, allow_quantized).await?;
        }
        #[cfg(feature = "benchmark")]
        Some(voicsh::cli::Commands::Init {
            language,
            allow_quantized,
        }) => {
            voicsh::init::run_full_init(&language, cli.verbose, allow_quantized).await?;
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
async fn handle_models_command(
    action: ModelsAction,
    custom_path: Option<&std::path::Path>,
) -> Result<()> {
    match action {
        ModelsAction::List => {
            println!("Available models:");
            for model in list_models() {
                println!("  {}", format_model_info(model));
            }

            // Show remote models (deduplicated against static catalog)
            #[cfg(feature = "model-download")]
            {
                use std::collections::HashSet;
                use voicsh::models::download::format_remote_model;
                use voicsh::models::remote::fetch_remote_models;

                let catalog_names: HashSet<&str> = list_models().iter().map(|m| m.name).collect();

                match fetch_remote_models().await {
                    Ok(remote) => {
                        let extras: Vec<_> = remote
                            .iter()
                            .filter(|m| !catalog_names.contains(m.name.as_str()))
                            .collect();
                        if !extras.is_empty() {
                            println!();
                            println!("Remote models (from HuggingFace):");
                            for m in extras {
                                println!("  {}", format_remote_model(&m.name, m.size_mb));
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  (Could not fetch remote models: {e})");
                    }
                }
            }
        }
        ModelsAction::Install { name } => {
            let path = download_model(&name, true).await?;
            println!("Model '{}' installed successfully", name);
            println!("Location: {}", path.display());
        }
        ModelsAction::Use { name } => {
            let resolved = resolve_name(&name);
            if resolved != name {
                println!("Resolved '{name}' to '{resolved}'");
            }
            if get_model(resolved).is_none() {
                eprintln!("Unknown model: '{name}'");
                eprintln!("Run `voicsh models list` to see available models.");
                std::process::exit(1);
            }

            let config_path = custom_path
                .map(std::path::PathBuf::from)
                .unwrap_or_else(Config::default_path);
            Config::update_model(&config_path, resolved)?;
            println!("Default model set to '{resolved}'");

            if !is_model_installed(resolved) {
                println!(
                    "Note: model not yet downloaded. Run `voicsh models install {resolved}` or it will download on first use."
                );
            }
        }
    }
    Ok(())
}

/// Handle configuration commands.
fn handle_config_command(
    action: ConfigAction,
    custom_path: Option<&std::path::Path>,
) -> Result<()> {
    let config_path = custom_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(Config::default_path);

    match action {
        ConfigAction::Get { key } => {
            let config = Config::load_or_default(&config_path).with_env_overrides();
            match config.get_value_by_path(&key) {
                Ok(value) => println!("{}", value),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        ConfigAction::Set { key, value } => {
            Config::set_value_by_path(&config_path, &key, &value)?;
            println!("Set {} = {}", key, value);
        }
        ConfigAction::List { key, language } => {
            let config = Config::load_or_default(&config_path).with_env_overrides();

            // Parse languages if provided
            let lang_codes: Option<Vec<&str>> = language
                .as_deref()
                .map(|s| s.split(',').map(|l| l.trim()).collect());

            // Validate languages
            if let Some(ref codes) = lang_codes
                && let Err(e) = Config::validate_languages(codes)
            {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }

            match (key.as_deref(), &lang_codes) {
                // Show voice commands for specific languages
                (Some("voice_commands"), Some(codes)) | (None, Some(codes)) => {
                    print!(
                        "{}",
                        Config::display_voice_commands(codes, &config.voice_commands.commands)
                    );
                }
                // Show voice commands section (all configured languages)
                (Some("voice_commands"), None) => {
                    let lang = config.stt.language.as_str();
                    let langs = if lang == "auto" {
                        vec!["en"]
                    } else {
                        vec![lang]
                    };
                    print!(
                        "{}",
                        Config::display_voice_commands(&langs, &config.voice_commands.commands)
                    );
                }
                // Show a specific config section
                (Some(section), None) => match config.display_section(section) {
                    Ok(toml) => println!("{}", toml),
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                },
                // Show full config (original behavior)
                (None, None) => match config.to_display_toml() {
                    Ok(toml) => print!("{}", toml),
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                },
                // --language with a non-voice_commands key doesn't make sense
                (Some(section), Some(_)) => {
                    eprintln!(
                        "Error: --language filter only applies to voice_commands, not '{}'",
                        section
                    );
                    std::process::exit(1);
                }
            }
        }
        ConfigAction::Dump => {
            print!("{}", Config::dump_template());
        }
    }
    Ok(())
}

/// Send IPC command to daemon and handle response.
async fn handle_ipc_command(socket: Option<std::path::PathBuf>, command: Command) -> Result<()> {
    let socket_path = socket.unwrap_or_else(IpcServer::default_socket_path);

    match send_command(&socket_path, command).await {
        Ok(response) => match response {
            Response::Ok { message } => {
                println!("{}", message.green());
            }
            Response::Transcription { text } => {
                println!("{}", text);
            }
            Response::Status {
                recording,
                model_loaded,
                model_name,
                language,
                daemon_version,
                backend,
                device,
            } => {
                let client_version = voicsh::version_string();

                println!("Status:");
                // Version info
                println!("  {}    {}", "Client:".dimmed(), client_version);
                print!("  {}    {}", "Daemon:".dimmed(), daemon_version);
                if client_version != daemon_version {
                    print!(" {}", "(version mismatch!)".yellow());
                }
                println!();
                // Backend + device
                match device {
                    Some(dev) => println!("  {}   {} — {}", "Backend:".dimmed(), backend, dev),
                    None => println!("  {}   {}", "Backend:".dimmed(), backend),
                }
                // Recording
                println!(
                    "  {} {}",
                    "Recording:".dimmed(),
                    if recording { "yes" } else { "no" }
                );
                // Model
                if model_loaded && let Some(name) = model_name {
                    println!("  {}     {}", "Model:".dimmed(), name);
                }
                // Language
                if let Some(lang) = language {
                    println!("  {}  {}", "Language:".dimmed(), lang);
                }
            }
            Response::Languages { languages, current } => {
                println!("Languages (current: {}):", current.green());
                for lang in &languages {
                    if lang == &current {
                        println!("  {} {}", "●".green(), lang);
                    } else {
                        println!("  ○ {}", lang);
                    }
                }
            }
            Response::Models { models, current } => {
                println!("Models (current: {}):", current.green());
                for m in &models {
                    let installed = if m.installed {
                        "installed".green().to_string()
                    } else {
                        "not installed".dimmed().to_string()
                    };
                    let lang = if m.english_only { "en-only" } else { "multi" };
                    if m.name == current {
                        println!(
                            "  {} {} ({}MB, {}, {})",
                            "●".green(),
                            m.name,
                            m.size_mb,
                            lang,
                            installed
                        );
                    } else {
                        println!("  ○ {} ({}MB, {}, {})", m.name, m.size_mb, lang, installed);
                    }
                }
            }
            Response::Error { message } => {
                eprintln!("{}", format!("Error: {}", message).red());
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!(
                "{}",
                format!("Failed to communicate with daemon: {}", e).red()
            );
            eprintln!("Is the daemon running? Start it with: voicsh daemon");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Follow daemon events and render live output.
async fn handle_follow(socket: Option<std::path::PathBuf>) -> Result<()> {
    let socket_path = socket.unwrap_or_else(IpcServer::default_socket_path);

    println!("Following daemon events... (Ctrl+C to stop)");

    match voicsh::ipc::client::follow(&socket_path, |event| {
        voicsh::output::render_event(&event);
    })
    .await
    {
        Ok(()) => {
            voicsh::output::clear_line();
            println!("Daemon connection closed");
        }
        Err(e) => {
            eprintln!("Failed to follow daemon: {}", e);
            eprintln!("Is the daemon running? Start it with: voicsh daemon");
            std::process::exit(1);
        }
    }

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
    threads_override: Option<usize>,
) -> Result<()> {
    use voicsh::benchmark::{
        BenchmarkReport, ResourceMonitor, SystemInfo, benchmark_model, compute_default_threads,
        load_wav_file, print_guidance, print_json_report, print_results,
    };
    use voicsh::models::download::{list_installed_models, model_path};

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
        // Use all installed models (catalog + any extras like quantized variants)
        list_installed_models()
    };

    if model_list.is_empty() {
        eprintln!("Error: No models available for benchmarking");
        eprintln!();
        eprintln!("Install models with:");
        eprintln!("  voicsh models install <model-name>");
        eprintln!();
        eprintln!("Run 'voicsh models list' to see available models.");
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
    let mut system_info = SystemInfo::detect();
    let threads =
        threads_override.unwrap_or_else(|| compute_default_threads(system_info.cpu_threads));
    system_info.whisper_threads = threads;
    system_info.print_report(verbose);
    println!();

    // Run benchmarks
    let monitor = ResourceMonitor::new();
    let mut results = Vec::new();

    for model_name in &model_list {
        // Check if model exists
        let model_exists = model_path(model_name).exists();

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

        let bench_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            benchmark_model(
                model_name,
                "auto", // Use auto language detection
                &audio_samples,
                audio_duration_ms,
                &monitor,
                iterations,
                verbose,
                threads,
            )
        }));

        match bench_result {
            Ok(Ok(result)) => {
                let rtf_ok = result.realtime_factor < 1.0;
                let indicator = if rtf_ok { "ok" } else { "SLOW" };
                println!(
                    "{}ms for {:.1}s audio -> RTF {:.2} ({})",
                    result.elapsed_ms,
                    audio_duration_ms as f64 / 1000.0,
                    result.realtime_factor,
                    indicator,
                );

                if verbose >= 1 {
                    println!(
                        "  \"{}\" [confidence: {:.2}]",
                        result.transcription.trim(),
                        result.confidence
                    );
                }

                results.push(result);
            }
            Ok(Err(e)) => {
                println!("FAILED");
                eprintln!("  Error: {}", e);
            }
            Err(_) => {
                println!("CRASHED");
                eprintln!("  Model panicked during benchmark, skipping.");
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
        print_guidance(&results, &system_info);
    }

    Ok(())
}
