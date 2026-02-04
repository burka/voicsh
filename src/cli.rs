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
    pub command: Commands,

    /// Path to configuration file
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Suppress output (quiet mode)
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Verbose output (show audio levels, debug info)
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

/// Available commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start recording, transcribe when done
    Record {
        /// Audio input device (e.g., hw:0)
        #[arg(long, value_name = "DEVICE")]
        device: Option<String>,

        /// Whisper model to use (tiny, base, small, medium, large)
        #[arg(long, value_name = "MODEL")]
        model: Option<String>,

        /// Language code for transcription (e.g., en, es, fr)
        #[arg(long, value_name = "LANG")]
        language: Option<String>,

        /// Prevent automatic model download if configured model is missing
        #[arg(long)]
        no_download: bool,

        /// Exit after first transcription (default: keep recording)
        #[arg(long)]
        once: bool,
    },

    /// List available audio input devices
    Devices,

    /// Manage Whisper models
    Models {
        /// Action to perform
        #[command(subcommand)]
        action: ModelsAction,
    },

    /// Start the voicsh daemon
    Start {
        /// Run in foreground instead of daemonizing
        #[arg(long)]
        foreground: bool,
    },

    /// Stop the voicsh daemon
    Stop,

    /// Toggle recording (daemon mode)
    Toggle,

    /// Show daemon status
    Status,

    /// Check system dependencies
    Check,
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
    fn test_parse_record_simple() {
        let cli = Cli::try_parse_from(["voicsh", "record"]).unwrap();
        match cli.command {
            Commands::Record {
                device,
                model,
                language,
                no_download,
                once,
            } => {
                assert!(device.is_none());
                assert!(model.is_none());
                assert!(language.is_none());
                assert!(!no_download);
                assert!(!once);
            }
            _ => panic!("Expected Record command"),
        }
        assert!(!cli.quiet);
        assert!(cli.config.is_none());
    }

    #[test]
    fn test_parse_record_with_options() {
        let cli = Cli::try_parse_from([
            "voicsh",
            "record",
            "--device",
            "hw:0",
            "--model",
            "base.en",
            "--language",
            "en",
        ])
        .unwrap();

        match cli.command {
            Commands::Record {
                device,
                model,
                language,
                no_download,
                once,
            } => {
                assert_eq!(device.as_deref(), Some("hw:0"));
                assert_eq!(model.as_deref(), Some("base.en"));
                assert_eq!(language.as_deref(), Some("en"));
                assert!(!no_download);
                assert!(!once);
            }
            _ => panic!("Expected Record command"),
        }
    }

    #[test]
    fn test_parse_devices() {
        let cli = Cli::try_parse_from(["voicsh", "devices"]).unwrap();
        match cli.command {
            Commands::Devices => {}
            _ => panic!("Expected Devices command"),
        }
    }

    #[test]
    fn test_parse_start_simple() {
        let cli = Cli::try_parse_from(["voicsh", "start"]).unwrap();
        match cli.command {
            Commands::Start { foreground } => {
                assert!(!foreground);
            }
            _ => panic!("Expected Start command"),
        }
    }

    #[test]
    fn test_parse_start_foreground() {
        let cli = Cli::try_parse_from(["voicsh", "start", "--foreground"]).unwrap();
        match cli.command {
            Commands::Start { foreground } => {
                assert!(foreground);
            }
            _ => panic!("Expected Start command"),
        }
    }

    #[test]
    fn test_parse_stop() {
        let cli = Cli::try_parse_from(["voicsh", "stop"]).unwrap();
        match cli.command {
            Commands::Stop => {}
            _ => panic!("Expected Stop command"),
        }
    }

    #[test]
    fn test_parse_toggle() {
        let cli = Cli::try_parse_from(["voicsh", "toggle"]).unwrap();
        match cli.command {
            Commands::Toggle => {}
            _ => panic!("Expected Toggle command"),
        }
    }

    #[test]
    fn test_parse_status() {
        let cli = Cli::try_parse_from(["voicsh", "status"]).unwrap();
        match cli.command {
            Commands::Status => {}
            _ => panic!("Expected Status command"),
        }
    }

    #[test]
    fn test_parse_global_config() {
        let cli =
            Cli::try_parse_from(["voicsh", "--config", "/path/to/config.toml", "record"]).unwrap();
        assert_eq!(cli.config, Some(PathBuf::from("/path/to/config.toml")));
        match cli.command {
            Commands::Record { .. } => {}
            _ => panic!("Expected Record command"),
        }
    }

    #[test]
    fn test_parse_global_quiet() {
        let cli = Cli::try_parse_from(["voicsh", "--quiet", "devices"]).unwrap();
        assert!(cli.quiet);
        match cli.command {
            Commands::Devices => {}
            _ => panic!("Expected Devices command"),
        }
    }

    #[test]
    fn test_parse_quiet_short_flag() {
        let cli = Cli::try_parse_from(["voicsh", "-q", "status"]).unwrap();
        assert!(cli.quiet);
    }

    #[test]
    fn test_parse_combined_global_options() {
        let cli = Cli::try_parse_from([
            "voicsh",
            "--config",
            "/etc/voicsh.toml",
            "--quiet",
            "start",
            "--foreground",
        ])
        .unwrap();

        assert_eq!(cli.config, Some(PathBuf::from("/etc/voicsh.toml")));
        assert!(cli.quiet);
        match cli.command {
            Commands::Start { foreground } => {
                assert!(foreground);
            }
            _ => panic!("Expected Start command"),
        }
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
            Cli::try_parse_from(["voicsh", "record", "--config", "/tmp/config.toml"]).unwrap();

        assert_eq!(cli.config, Some(PathBuf::from("/tmp/config.toml")));
    }

    #[test]
    fn test_record_with_partial_options() {
        let cli = Cli::try_parse_from(["voicsh", "record", "--model", "base"]).unwrap();

        match cli.command {
            Commands::Record {
                device,
                model,
                language,
                no_download,
                once,
            } => {
                assert!(device.is_none());
                assert_eq!(model.as_deref(), Some("base"));
                assert!(language.is_none());
                assert!(!no_download);
                assert!(!once);
            }
            _ => panic!("Expected Record command"),
        }
    }

    #[test]
    fn test_parse_record_with_no_download() {
        let cli = Cli::try_parse_from(["voicsh", "record", "--no-download"]).unwrap();

        match cli.command {
            Commands::Record {
                device,
                model,
                language,
                no_download,
                once,
            } => {
                assert!(device.is_none());
                assert!(model.is_none());
                assert!(language.is_none());
                assert!(no_download);
                assert!(!once);
            }
            _ => panic!("Expected Record command"),
        }
    }

    #[test]
    fn test_parse_models_list() {
        let cli = Cli::try_parse_from(["voicsh", "models", "list"]).unwrap();
        match cli.command {
            Commands::Models { action } => match action {
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
            Commands::Models { action } => match action {
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
            Commands::Models { action } => match action {
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
            Commands::Check => {}
            _ => panic!("Expected Check command"),
        }
    }

    #[test]
    fn test_parse_record_with_once() {
        let cli = Cli::try_parse_from(["voicsh", "record", "--once"]).unwrap();

        match cli.command {
            Commands::Record {
                device,
                model,
                language,
                no_download,
                once,
            } => {
                assert!(device.is_none());
                assert!(model.is_none());
                assert!(language.is_none());
                assert!(!no_download);
                assert!(once);
            }
            _ => panic!("Expected Record command"),
        }
    }
}
