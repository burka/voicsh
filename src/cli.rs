//! Command-line interface for voicsh
//!
//! Provides argument parsing using clap derive macros.

use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

/// Voice typing for Wayland Linux
#[derive(Parser, Debug)]
#[command(name = "voicsh", version, about = "Voice typing for Wayland Linux")]
pub struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to configuration file
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Suppress output (quiet mode)
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Verbose output (-v: meter + results, -vv: full diagnostics)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Audio input device (e.g., hw:0)
    #[arg(long, value_name = "DEVICE")]
    pub device: Option<String>,

    /// Whisper model (default: base, multilingual). Use base.en for English-only optimized
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,

    /// Language code for transcription (default: auto-detect). Examples: auto, en, de, es, fr
    #[arg(long, value_name = "LANG")]
    pub language: Option<String>,

    /// Injection backend override (auto, portal, wtype, ydotool)
    #[arg(long, value_name = "BACKEND")]
    pub injection_backend: Option<String>,

    /// Prevent automatic model download if configured model is missing
    #[arg(long)]
    pub no_download: bool,

    /// Exit after first transcription (default: keep recording)
    #[arg(long)]
    pub once: bool,

    /// Run English-optimized and multilingual models in parallel, pick best result
    #[arg(long)]
    pub fan_out: bool,

    /// Chunk duration in seconds for progressive transcription
    #[arg(long, short = 'c', value_name = "SECONDS", default_value = "3")]
    pub chunk_size: u32,

    /// Transcription buffer duration (default: 10s). Examples: 30s, 5m, 1h30m
    #[arg(long, short = 'b', value_name = "DURATION", default_value = "10s", value_parser = parse_buffer_secs)]
    pub buffer: u64,
}

/// Parse a buffer duration string into seconds.
///
/// Supports any duration format accepted by `humantime`: bare numbers (seconds),
/// single-unit (`30s`, `5m`, `2h`), and compound (`1h30m`, `2m30s`).
fn parse_buffer_secs(s: &str) -> Result<u64, String> {
    let s = s.trim();
    // Bare number → seconds
    if let Ok(secs) = s.parse::<u64>() {
        return Ok(secs);
    }
    humantime::parse_duration(s)
        .map(|d| d.as_secs())
        .map_err(|e| e.to_string())
}

/// Available commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// List available audio input devices
    Devices,

    /// Manage Whisper models
    Models {
        /// Action to perform
        #[command(subcommand)]
        action: ModelsAction,
    },

    /// Check system dependencies
    Check,

    /// Start the daemon (foreground process for systemd)
    Daemon {
        /// Path to Unix socket (default: $XDG_RUNTIME_DIR/voicsh.sock)
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
    },

    /// Start recording via IPC
    Start {
        /// Path to Unix socket (default: $XDG_RUNTIME_DIR/voicsh.sock)
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
    },

    /// Stop recording and inject transcription via IPC
    Stop {
        /// Path to Unix socket (default: $XDG_RUNTIME_DIR/voicsh.sock)
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
    },

    /// Toggle recording on/off via IPC
    Toggle {
        /// Path to Unix socket (default: $XDG_RUNTIME_DIR/voicsh.sock)
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
    },

    /// Get daemon status via IPC
    Status {
        /// Path to Unix socket (default: $XDG_RUNTIME_DIR/voicsh.sock)
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
    },

    /// Follow daemon events (live volume meter, recording state, transcriptions)
    Follow {
        /// Path to Unix socket (default: $XDG_RUNTIME_DIR/voicsh.sock)
        #[arg(long, value_name = "PATH")]
        socket: Option<PathBuf>,
    },

    /// Benchmark transcription performance across models
    #[cfg(feature = "benchmark")]
    Benchmark {
        /// WAV file to benchmark (defaults to test fixture if available)
        #[arg(long, value_name = "FILE")]
        audio: Option<PathBuf>,

        /// Models to test (comma-separated, default: all installed)
        #[arg(long, value_name = "MODELS")]
        models: Option<String>,

        /// Number of iterations to average (default: 1)
        #[arg(long, short = 'n', value_name = "N", default_value = "1")]
        iterations: usize,

        /// Output format: table (default) or json
        #[arg(long, short = 'o', value_name = "FORMAT", default_value = "table")]
        output: String,

        /// Number of CPU threads for inference (default: auto)
        #[arg(long, short = 't', value_name = "THREADS")]
        threads: Option<usize>,
    },

    /// Auto-tune: benchmark hardware, download optimal model, configure
    #[cfg(feature = "benchmark")]
    AutoTune {
        /// Language for transcription (default: auto). Examples: auto, en, de, es, fr
        #[arg(long, value_name = "LANG", default_value = "auto")]
        language: String,

        /// Include quantized models as candidates (smaller/faster but lower precision)
        #[arg(long)]
        allow_quantized: bool,
    },

    /// Install systemd user service
    InstallService,

    /// Install GNOME Shell extension and systemd user service
    InstallGnomeExtension,

    /// Uninstall GNOME Shell extension and systemd user service
    UninstallGnomeExtension,

    /// View and modify configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },

    /// Detect environment, configure injection backend, and auto-tune model
    #[cfg(feature = "benchmark")]
    Init {
        /// Language for transcription (default: auto)
        #[arg(long, value_name = "LANG", default_value = "auto")]
        language: String,
        /// Include quantized models as candidates
        #[arg(long)]
        allow_quantized: bool,
    },
}

/// Configuration management actions
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Get a configuration value by key (e.g., stt.model)
    Get {
        /// Dotted key path (e.g., stt.model, audio.sample_rate)
        key: String,
    },
    /// Set a configuration value by key
    Set {
        /// Dotted key path (e.g., stt.model, audio.sample_rate)
        key: String,
        /// Value to set
        value: String,
    },
    /// List current configuration values (optionally filtered by section or language)
    List {
        /// Config section to show (e.g., stt, audio, voice_commands)
        key: Option<String>,
        /// Filter voice commands by language (comma-separated, e.g., en,de)
        #[arg(long, value_name = "LANG")]
        language: Option<String>,
    },
    /// Dump a commented configuration template
    Dump,
}

/// Model management actions
#[derive(Subcommand, Debug)]
pub enum ModelsAction {
    /// List available models
    List,
    /// Download and install a model
    Install {
        /// Model name (e.g., base.en, small.en, tiny)
        name: String,
    },
    /// Set the default STT model
    Use {
        /// Model name (e.g., tiny.en, base, large-v3-turbo)
        name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_default_command() {
        let cli = Cli::try_parse_from(["voicsh"]).unwrap();
        assert!(cli.command.is_none());
        assert!(cli.device.is_none());
        assert!(cli.model.is_none());
        assert!(cli.language.is_none());
        assert!(cli.injection_backend.is_none());
        assert!(!cli.no_download);
        assert!(!cli.once);
        assert!(!cli.fan_out);
        assert_eq!(cli.chunk_size, 3); // default: 3 seconds
        assert_eq!(cli.buffer, 10); // default: 10 seconds
        assert!(!cli.quiet);
        assert_eq!(cli.verbose, 0);
        assert!(cli.config.is_none());
    }

    #[test]
    fn test_parse_verbose_single() {
        let cli = Cli::try_parse_from(["voicsh", "-v"]).unwrap();
        assert_eq!(cli.verbose, 1);
    }

    #[test]
    fn test_parse_verbose_double() {
        let cli = Cli::try_parse_from(["voicsh", "-vv"]).unwrap();
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn test_parse_verbose_repeated_flags() {
        let cli = Cli::try_parse_from(["voicsh", "-v", "-v"]).unwrap();
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn test_parse_with_options() {
        let cli = Cli::try_parse_from([
            "voicsh",
            "--device",
            "hw:0",
            "--model",
            "base.en",
            "--language",
            "en",
        ])
        .unwrap();

        assert_eq!(cli.device.as_deref(), Some("hw:0"));
        assert_eq!(cli.model.as_deref(), Some("base.en"));
        assert_eq!(cli.language.as_deref(), Some("en"));
        assert!(!cli.no_download);
        assert!(!cli.once);
        assert!(!cli.fan_out);
    }

    #[test]
    fn test_parse_devices() {
        let cli = Cli::try_parse_from(["voicsh", "devices"]).unwrap();
        match cli.command {
            Some(Commands::Devices) => {}
            _ => panic!("Expected Devices command"),
        }
    }

    #[test]
    fn test_parse_global_config() {
        let cli = Cli::try_parse_from(["voicsh", "--config", "/path/to/config.toml"]).unwrap();
        assert_eq!(cli.config, Some(PathBuf::from("/path/to/config.toml")));
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_parse_global_quiet() {
        let cli = Cli::try_parse_from(["voicsh", "--quiet", "devices"]).unwrap();
        assert!(cli.quiet);
        match cli.command {
            Some(Commands::Devices) => {}
            _ => panic!("Expected Devices command"),
        }
    }

    #[test]
    fn test_parse_quiet_short_flag() {
        let cli = Cli::try_parse_from(["voicsh", "-q"]).unwrap();
        assert!(cli.quiet);
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_invalid_command_returns_error() {
        let result = Cli::try_parse_from(["voicsh", "invalid"]);
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn test_help_flag() {
        // --help causes clap to exit with Ok status and print help
        // We can't easily test the output, but we can verify it's recognized
        let result = Cli::try_parse_from(["voicsh", "--help"]);
        // Clap returns an error for --help but with DisplayHelp kind
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn test_version_flag() {
        let result = Cli::try_parse_from(["voicsh", "--version"]);
        // Clap returns an error for --version but with DisplayVersion kind
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn test_global_options_after_command() {
        // Global options should work before or after the command
        let cli =
            Cli::try_parse_from(["voicsh", "devices", "--config", "/tmp/config.toml"]).unwrap();

        assert_eq!(cli.config, Some(PathBuf::from("/tmp/config.toml")));
    }

    #[test]
    fn test_partial_options() {
        let cli = Cli::try_parse_from(["voicsh", "--model", "base"]).unwrap();

        assert!(cli.device.is_none());
        assert_eq!(cli.model.as_deref(), Some("base"));
        assert!(cli.language.is_none());
        assert!(!cli.no_download);
        assert!(!cli.once);
        assert!(!cli.fan_out);
    }

    #[test]
    fn test_no_download() {
        let cli = Cli::try_parse_from(["voicsh", "--no-download"]).unwrap();

        assert!(cli.device.is_none());
        assert!(cli.model.is_none());
        assert!(cli.language.is_none());
        assert!(cli.no_download);
        assert!(!cli.once);
        assert!(!cli.fan_out);
    }

    #[test]
    fn test_parse_models_list() {
        let cli = Cli::try_parse_from(["voicsh", "models", "list"]).unwrap();
        match cli.command {
            Some(Commands::Models { action }) => match action {
                ModelsAction::List => {}
                _ => panic!("Expected List action"),
            },
            _ => panic!("Expected Models command"),
        }
    }

    #[test]
    fn test_parse_models_install() {
        let cli = Cli::try_parse_from(["voicsh", "models", "install", "base.en"]).unwrap();
        match cli.command {
            Some(Commands::Models { action }) => match action {
                ModelsAction::Install { name } => {
                    assert_eq!(name, "base.en");
                }
                _ => panic!("Expected Install action"),
            },
            _ => panic!("Expected Models command"),
        }
    }

    #[test]
    fn test_parse_models_install_different_model() {
        let cli = Cli::try_parse_from(["voicsh", "models", "install", "tiny"]).unwrap();
        match cli.command {
            Some(Commands::Models { action }) => match action {
                ModelsAction::Install { name } => {
                    assert_eq!(name, "tiny");
                }
                _ => panic!("Expected Install action"),
            },
            _ => panic!("Expected Models command"),
        }
    }

    #[test]
    fn test_models_requires_subcommand() {
        let result = Cli::try_parse_from(["voicsh", "models"]);
        let err = result.unwrap_err();
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn test_models_install_requires_name() {
        let result = Cli::try_parse_from(["voicsh", "models", "install"]);
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("required") || msg.contains("name"),
            "Expected missing required argument error, got: {msg}"
        );
    }

    #[test]
    fn test_parse_models_use() {
        let cli = Cli::try_parse_from(["voicsh", "models", "use", "tiny.en"]).unwrap();
        match cli.command {
            Some(Commands::Models { action }) => match action {
                ModelsAction::Use { name } => {
                    assert_eq!(name, "tiny.en");
                }
                _ => panic!("Expected Use action"),
            },
            _ => panic!("Expected Models command"),
        }
    }

    #[test]
    fn test_models_use_requires_name() {
        let result = Cli::try_parse_from(["voicsh", "models", "use"]);
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn test_parse_check() {
        let cli = Cli::try_parse_from(["voicsh", "check"]).unwrap();
        match cli.command {
            Some(Commands::Check) => {}
            _ => panic!("Expected Check command"),
        }
    }

    #[test]
    fn test_once() {
        let cli = Cli::try_parse_from(["voicsh", "--once"]).unwrap();

        assert!(cli.device.is_none());
        assert!(cli.model.is_none());
        assert!(cli.language.is_none());
        assert!(!cli.no_download);
        assert!(cli.once);
        assert!(!cli.fan_out);
    }

    #[test]
    fn test_fan_out() {
        let cli = Cli::try_parse_from(["voicsh", "--fan-out"]).unwrap();
        assert!(cli.fan_out);
    }

    #[test]
    fn test_chunk_size() {
        let cli = Cli::try_parse_from(["voicsh", "--chunk-size", "5"]).unwrap();
        assert_eq!(cli.chunk_size, 5);
    }

    #[test]
    fn test_chunk_size_short() {
        let cli = Cli::try_parse_from(["voicsh", "-c", "2"]).unwrap();
        assert_eq!(cli.chunk_size, 2);
    }

    #[test]
    fn test_parse_daemon() {
        let cli = Cli::try_parse_from(["voicsh", "daemon"]).unwrap();
        match cli.command {
            Some(Commands::Daemon { socket }) => {
                assert!(socket.is_none());
            }
            _ => panic!("Expected Daemon command"),
        }
    }

    #[test]
    fn test_parse_daemon_with_socket() {
        let cli = Cli::try_parse_from(["voicsh", "daemon", "--socket", "/tmp/test.sock"]).unwrap();
        match cli.command {
            Some(Commands::Daemon { socket }) => {
                assert_eq!(socket, Some(PathBuf::from("/tmp/test.sock")));
            }
            _ => panic!("Expected Daemon command"),
        }
    }

    #[test]
    fn test_parse_start() {
        let cli = Cli::try_parse_from(["voicsh", "start"]).unwrap();
        match cli.command {
            Some(Commands::Start { socket }) => {
                assert!(socket.is_none());
            }
            _ => panic!("Expected Start command"),
        }
    }

    #[test]
    fn test_parse_stop() {
        let cli = Cli::try_parse_from(["voicsh", "stop"]).unwrap();
        match cli.command {
            Some(Commands::Stop { socket }) => {
                assert!(socket.is_none());
            }
            _ => panic!("Expected Stop command"),
        }
    }

    #[test]
    fn test_parse_toggle() {
        let cli = Cli::try_parse_from(["voicsh", "toggle"]).unwrap();
        match cli.command {
            Some(Commands::Toggle { socket }) => {
                assert!(socket.is_none());
            }
            _ => panic!("Expected Toggle command"),
        }
    }

    #[test]
    fn test_parse_status() {
        let cli = Cli::try_parse_from(["voicsh", "status"]).unwrap();
        match cli.command {
            Some(Commands::Status { socket }) => {
                assert!(socket.is_none());
            }
            _ => panic!("Expected Status command"),
        }
    }

    #[test]
    fn test_parse_install_service() {
        let cli = Cli::try_parse_from(["voicsh", "install-service"]).unwrap();
        match cli.command {
            Some(Commands::InstallService) => {}
            _ => panic!("Expected InstallService command"),
        }
    }

    #[test]
    fn test_parse_install_gnome_extension() {
        let cli = Cli::try_parse_from(["voicsh", "install-gnome-extension"]).unwrap();
        match cli.command {
            Some(Commands::InstallGnomeExtension) => {}
            _ => panic!("Expected InstallGnomeExtension command"),
        }
    }

    #[test]
    fn test_parse_uninstall_gnome_extension() {
        let cli = Cli::try_parse_from(["voicsh", "uninstall-gnome-extension"]).unwrap();
        match cli.command {
            Some(Commands::UninstallGnomeExtension) => {}
            _ => panic!("Expected UninstallGnomeExtension command"),
        }
    }

    #[test]
    #[cfg(feature = "benchmark")]
    fn test_parse_benchmark() {
        let cli = Cli::try_parse_from(["voicsh", "benchmark"]).unwrap();
        match cli.command {
            Some(Commands::Benchmark {
                audio,
                models,
                iterations,
                output,
                threads,
            }) => {
                assert!(audio.is_none());
                assert!(models.is_none());
                assert_eq!(iterations, 1);
                assert_eq!(output, "table");
                assert!(threads.is_none());
            }
            _ => panic!("Expected Benchmark command"),
        }
    }

    #[test]
    #[cfg(feature = "benchmark")]
    fn test_parse_benchmark_with_all_options() {
        let cli = Cli::try_parse_from([
            "voicsh",
            "benchmark",
            "--audio",
            "test.wav",
            "--models",
            "tiny.en,base.en",
            "--iterations",
            "3",
            "--output",
            "json",
            "--threads",
            "8",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Benchmark {
                audio,
                models,
                iterations,
                output,
                threads,
            }) => {
                assert_eq!(audio, Some(PathBuf::from("test.wav")));
                assert_eq!(models.as_deref(), Some("tiny.en,base.en"));
                assert_eq!(iterations, 3);
                assert_eq!(output, "json");
                assert_eq!(threads, Some(8));
            }
            _ => panic!("Expected Benchmark command"),
        }
    }

    #[test]
    #[cfg(feature = "benchmark")]
    fn test_parse_auto_tune() {
        let cli = Cli::try_parse_from(["voicsh", "auto-tune"]).unwrap();
        match cli.command {
            Some(Commands::AutoTune {
                language,
                allow_quantized,
            }) => {
                assert_eq!(language, "auto");
                assert!(!allow_quantized);
            }
            _ => panic!("Expected AutoTune command"),
        }
    }

    #[test]
    #[cfg(feature = "benchmark")]
    fn test_parse_auto_tune_with_language() {
        let cli = Cli::try_parse_from(["voicsh", "auto-tune", "--language", "de"]).unwrap();
        match cli.command {
            Some(Commands::AutoTune {
                language,
                allow_quantized,
            }) => {
                assert_eq!(language, "de");
                assert!(!allow_quantized);
            }
            _ => panic!("Expected AutoTune command"),
        }
    }

    #[test]
    #[cfg(feature = "benchmark")]
    fn test_parse_auto_tune_allow_quantized() {
        let cli = Cli::try_parse_from([
            "voicsh",
            "auto-tune",
            "--allow-quantized",
            "--language",
            "en",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::AutoTune {
                language,
                allow_quantized,
            }) => {
                assert_eq!(language, "en");
                assert!(allow_quantized);
            }
            _ => panic!("Expected AutoTune command"),
        }
    }

    #[test]
    #[cfg(feature = "benchmark")]
    fn test_parse_init() {
        let cli = Cli::try_parse_from(["voicsh", "init"]).unwrap();
        match cli.command {
            Some(Commands::Init {
                language,
                allow_quantized,
            }) => {
                assert_eq!(language, "auto");
                assert!(!allow_quantized);
            }
            _ => panic!("Expected Init command"),
        }
    }

    #[test]
    fn test_parse_start_with_socket() {
        let cli = Cli::try_parse_from(["voicsh", "start", "--socket", "/tmp/test.sock"]).unwrap();
        match cli.command {
            Some(Commands::Start { socket }) => {
                assert_eq!(socket, Some(PathBuf::from("/tmp/test.sock")));
            }
            _ => panic!("Expected Start command"),
        }
    }

    #[test]
    fn test_parse_toggle_with_socket() {
        let cli = Cli::try_parse_from(["voicsh", "toggle", "--socket", "/tmp/test.sock"]).unwrap();
        match cli.command {
            Some(Commands::Toggle { socket }) => {
                assert_eq!(socket, Some(PathBuf::from("/tmp/test.sock")));
            }
            _ => panic!("Expected Toggle command"),
        }
    }

    #[test]
    fn test_parse_status_with_socket() {
        let cli = Cli::try_parse_from(["voicsh", "status", "--socket", "/tmp/test.sock"]).unwrap();
        match cli.command {
            Some(Commands::Status { socket }) => {
                assert_eq!(socket, Some(PathBuf::from("/tmp/test.sock")));
            }
            _ => panic!("Expected Status command"),
        }
    }

    #[test]
    fn test_parse_follow() {
        let cli = Cli::try_parse_from(["voicsh", "follow"]).unwrap();
        match cli.command {
            Some(Commands::Follow { socket }) => {
                assert!(socket.is_none());
            }
            _ => panic!("Expected Follow command"),
        }
    }

    #[test]
    fn test_parse_follow_with_socket() {
        let cli = Cli::try_parse_from(["voicsh", "follow", "--socket", "/tmp/test.sock"]).unwrap();
        match cli.command {
            Some(Commands::Follow { socket }) => {
                assert_eq!(socket, Some(PathBuf::from("/tmp/test.sock")));
            }
            _ => panic!("Expected Follow command"),
        }
    }

    // ── Buffer parsing tests ─────────────────────────────────────────────

    #[test]
    fn test_parse_buffer_secs_bare_number() {
        assert_eq!(parse_buffer_secs("10").unwrap(), 10);
        assert_eq!(parse_buffer_secs("0").unwrap(), 0);
        assert_eq!(parse_buffer_secs("300").unwrap(), 300);
    }

    #[test]
    fn test_parse_buffer_secs_with_s_suffix() {
        assert_eq!(parse_buffer_secs("10s").unwrap(), 10);
        assert_eq!(parse_buffer_secs("20s").unwrap(), 20);
        assert_eq!(parse_buffer_secs("0s").unwrap(), 0);
    }

    #[test]
    fn test_parse_buffer_secs_with_m_suffix() {
        assert_eq!(parse_buffer_secs("1m").unwrap(), 60);
        assert_eq!(parse_buffer_secs("5m").unwrap(), 300);
        assert_eq!(parse_buffer_secs("0m").unwrap(), 0);
    }

    #[test]
    fn test_parse_buffer_secs_hours() {
        assert_eq!(parse_buffer_secs("1h").unwrap(), 3600);
        assert_eq!(parse_buffer_secs("2h").unwrap(), 7200);
    }

    #[test]
    fn test_parse_buffer_secs_compound() {
        assert_eq!(parse_buffer_secs("1h30m").unwrap(), 5400);
        assert_eq!(parse_buffer_secs("2m30s").unwrap(), 150);
        assert_eq!(parse_buffer_secs("1h2m3s").unwrap(), 3723);
    }

    #[test]
    fn test_parse_buffer_secs_verbose_units() {
        assert_eq!(parse_buffer_secs("5minutes").unwrap(), 300);
        assert_eq!(parse_buffer_secs("30seconds").unwrap(), 30);
        assert_eq!(parse_buffer_secs("1hour").unwrap(), 3600);
    }

    #[test]
    fn test_parse_buffer_secs_invalid() {
        let err = parse_buffer_secs("abc").unwrap_err();
        assert!(
            err.contains("invalid") || err.contains("expected") || err.contains("unknown"),
            "Expected parse error for 'abc', got: {err}"
        );
        let err = parse_buffer_secs("10x").unwrap_err();
        assert!(
            err.contains("invalid") || err.contains("expected") || err.contains("unknown"),
            "Expected parse error for '10x', got: {err}"
        );
        let err = parse_buffer_secs("").unwrap_err();
        assert!(
            err.contains("invalid") || err.contains("expected") || err.contains("empty"),
            "Expected parse error for empty string, got: {err}"
        );
        let err = parse_buffer_secs("-5").unwrap_err();
        assert!(
            err.contains("invalid") || err.contains("expected") || err.contains("unknown"),
            "Expected parse error for '-5', got: {err}"
        );
    }

    #[test]
    fn test_buffer_cli_arg_default() {
        let cli = Cli::try_parse_from(["voicsh"]).unwrap();
        assert_eq!(cli.buffer, 10);
    }

    #[test]
    fn test_buffer_cli_arg_short() {
        let cli = Cli::try_parse_from(["voicsh", "-b", "20s"]).unwrap();
        assert_eq!(cli.buffer, 20);
    }

    #[test]
    fn test_buffer_cli_arg_long() {
        let cli = Cli::try_parse_from(["voicsh", "--buffer", "5m"]).unwrap();
        assert_eq!(cli.buffer, 300);
    }

    #[test]
    fn test_buffer_cli_arg_bare_number() {
        let cli = Cli::try_parse_from(["voicsh", "-b", "30"]).unwrap();
        assert_eq!(cli.buffer, 30);
    }

    // ── Config command tests ────────────────────────────────────────────

    #[test]
    fn test_parse_config_get() {
        let cli = Cli::try_parse_from(["voicsh", "config", "get", "stt.model"]).unwrap();
        match cli.command {
            Some(Commands::Config { action }) => match action {
                ConfigAction::Get { key } => {
                    assert_eq!(key, "stt.model");
                }
                _ => panic!("Expected Get action"),
            },
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_parse_config_set() {
        let cli =
            Cli::try_parse_from(["voicsh", "config", "set", "stt.model", "small.en"]).unwrap();
        match cli.command {
            Some(Commands::Config { action }) => match action {
                ConfigAction::Set { key, value } => {
                    assert_eq!(key, "stt.model");
                    assert_eq!(value, "small.en");
                }
                _ => panic!("Expected Set action"),
            },
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_parse_config_list() {
        let cli = Cli::try_parse_from(["voicsh", "config", "list"]).unwrap();
        match cli.command {
            Some(Commands::Config { action }) => match action {
                ConfigAction::List { key, language } => {
                    assert!(key.is_none(), "No key should be set");
                    assert!(language.is_none(), "No language should be set");
                }
                _ => panic!("Expected List action"),
            },
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_parse_config_list_with_key() {
        let cli = Cli::try_parse_from(["voicsh", "config", "list", "stt"]).unwrap();
        match cli.command {
            Some(Commands::Config { action }) => match action {
                ConfigAction::List { key, language } => {
                    assert_eq!(key.as_deref(), Some("stt"));
                    assert!(language.is_none());
                }
                _ => panic!("Expected List action"),
            },
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_parse_config_list_with_language() {
        let cli = Cli::try_parse_from(["voicsh", "config", "list", "--language=ko"]).unwrap();
        match cli.command {
            Some(Commands::Config { action }) => match action {
                ConfigAction::List { key, language } => {
                    assert!(key.is_none());
                    assert_eq!(language.as_deref(), Some("ko"));
                }
                _ => panic!("Expected List action"),
            },
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_parse_config_list_with_key_and_language() {
        let cli = Cli::try_parse_from([
            "voicsh",
            "config",
            "list",
            "voice_commands",
            "--language=en,de",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Config { action }) => match action {
                ConfigAction::List { key, language } => {
                    assert_eq!(key.as_deref(), Some("voice_commands"));
                    assert_eq!(language.as_deref(), Some("en,de"));
                }
                _ => panic!("Expected List action"),
            },
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_parse_config_dump() {
        let cli = Cli::try_parse_from(["voicsh", "config", "dump"]).unwrap();
        match cli.command {
            Some(Commands::Config { action }) => match action {
                ConfigAction::Dump => {}
                _ => panic!("Expected Dump action"),
            },
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_config_requires_subcommand() {
        let result = Cli::try_parse_from(["voicsh", "config"]);
        let err = result.unwrap_err();
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn test_config_get_requires_key() {
        let result = Cli::try_parse_from(["voicsh", "config", "get"]);
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("required") || msg.contains("key"),
            "Expected missing required argument error, got: {msg}"
        );
    }

    #[test]
    fn test_config_set_requires_key_and_value() {
        let result = Cli::try_parse_from(["voicsh", "config", "set"]);
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("required") || msg.contains("key"),
            "Expected missing required argument error, got: {msg}"
        );
        let result = Cli::try_parse_from(["voicsh", "config", "set", "stt.model"]);
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("required") || msg.contains("value"),
            "Expected missing required argument error, got: {msg}"
        );
    }

    #[test]
    fn test_parse_injection_backend() {
        let cli = Cli::try_parse_from(["voicsh", "--injection-backend", "wtype"]).unwrap();
        assert_eq!(cli.injection_backend.as_deref(), Some("wtype"));
    }

    #[test]
    fn test_parse_injection_backend_portal() {
        let cli = Cli::try_parse_from(["voicsh", "--injection-backend", "portal"]).unwrap();
        assert_eq!(cli.injection_backend.as_deref(), Some("portal"));
    }
}
