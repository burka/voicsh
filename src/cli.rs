//! Command-line interface for voicsh
//!
//! Provides argument parsing using clap derive macros.

use clap::{Parser, Subcommand};
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
    },

    /// Install systemd user service
    InstallService,
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
        assert!(!cli.no_download);
        assert!(!cli.once);
        assert!(!cli.fan_out);
        assert_eq!(cli.chunk_size, 3); // default: 3 seconds
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
        assert!(result.is_err());
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
        assert!(result.is_err());
    }

    #[test]
    fn test_models_install_requires_name() {
        let result = Cli::try_parse_from(["voicsh", "models", "install"]);
        assert!(result.is_err());
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
}
