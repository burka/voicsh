//! voicsh - Voice typing for Wayland Linux
//!
//! Offline-first voice-to-text with optional LLM refinement.

// Enforce error handling discipline — see CLAUDE.md "Error Handling Rules"
#![warn(clippy::unwrap_used)]
#![warn(clippy::expect_used)]
#![warn(clippy::let_underscore_must_use)]

pub mod audio;
#[cfg(feature = "cli")]
pub mod cli;
pub mod config;
#[cfg(all(feature = "cpal-audio", feature = "model-download"))]
pub mod daemon;
pub mod defaults;
#[cfg(feature = "cli")]
pub mod diagnostics;
pub mod error;
pub mod input;
pub mod ipc;
#[cfg(feature = "model-download")]
pub mod models;
pub mod pipeline;
pub mod streaming;
pub mod stt;

// L4 composition root - needs everything
#[cfg(all(feature = "cpal-audio", feature = "model-download", feature = "cli"))]
pub mod app;

// Core traits (source → process → sink)
pub use audio::recorder::AudioSource;
pub use input::injector::{CommandExecutor, SystemCommandExecutor, TextInjector};
pub use pipeline::sink::{CollectorSink, InjectorSink, StdoutSink, TextSink};
pub use stt::transcriber::Transcriber;

// Pipeline
pub use pipeline::orchestrator::{Pipeline, PipelineConfig, PipelineHandle};

// Error handling
pub use error::{Result, VoicshError};

// Config
pub use config::{Config, InputMethod};

// Station framework (for advanced users)
pub use pipeline::error::{ErrorReporter, StationError};
pub use pipeline::station::Station;
