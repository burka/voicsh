//! voicsh - Voice typing for Wayland Linux
//!
//! Offline-first voice-to-text with optional LLM refinement.

// Enforce error handling discipline — see CLAUDE.md "Error Handling Rules"
#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]
#![warn(clippy::let_underscore_must_use)]

pub mod audio;
#[cfg(feature = "benchmark")]
pub mod benchmark;
#[cfg(feature = "cli")]
pub mod cli;
pub mod config;
#[cfg(all(feature = "cpal-audio", feature = "model-download"))]
pub mod daemon;
pub mod defaults;
#[cfg(feature = "cli")]
pub mod diagnostics;
pub mod error;
#[cfg(feature = "cli")]
pub mod gnome_extension;
#[cfg(feature = "benchmark")]
pub mod init;
pub mod inject;
pub mod ipc;
#[cfg(feature = "model-download")]
pub mod models;
pub mod output;
pub mod pipeline;
pub mod stt;
#[cfg(feature = "cli")]
pub mod systemd;

// L4 composition root - needs everything
#[cfg(all(feature = "cpal-audio", feature = "model-download", feature = "cli"))]
pub mod app;

// Core traits (source → process → sink)
pub use audio::recorder::AudioSource;
pub use inject::injector::{CommandExecutor, SystemCommandExecutor, TextInjector};
pub use pipeline::sink::{CollectorSink, InjectorSink, StdoutSink, TextSink};
pub use stt::transcriber::Transcriber;

// Pipeline
pub use pipeline::orchestrator::{Pipeline, PipelineConfig, PipelineHandle};

// Error handling
pub use error::{Result, VoicshError};

// Config
pub use config::{Config, InjectionMethod, resolve_hallucination_filters};

// Station framework (for advanced users)
pub use pipeline::error::{ErrorReporter, StationError};
pub use pipeline::station::Station;

/// Build version string with optional git commit hash.
///
/// Returns `"0.0.1+abc1234"` when git hash is available, `"0.0.1"` otherwise.
pub fn version_string() -> String {
    let version = env!("CARGO_PKG_VERSION");
    match option_env!("GIT_HASH") {
        Some(hash) if !hash.is_empty() => format!("{}+{}", version, hash),
        _ => version.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_string_starts_with_cargo_version() {
        let ver = version_string();
        assert!(
            ver.starts_with(env!("CARGO_PKG_VERSION")),
            "version_string should start with CARGO_PKG_VERSION, got: {}",
            ver
        );
    }

    #[test]
    fn version_string_contains_plus_when_git_hash_present() {
        let ver = version_string();
        // In a git repo build, GIT_HASH is set → expect "0.0.1+<hash>"
        // In CI without git, expect plain "0.0.1"
        if option_env!("GIT_HASH").is_some_and(|h| !h.is_empty()) {
            assert!(
                ver.contains('+'),
                "With GIT_HASH set, version should contain '+', got: {}",
                ver
            );
            let hash_part = ver.split('+').nth(1).unwrap_or("");
            assert_eq!(
                hash_part.len(),
                7,
                "Git hash should be 7 chars, got: {}",
                hash_part
            );
        } else {
            assert_eq!(ver, env!("CARGO_PKG_VERSION"));
        }
    }
}
