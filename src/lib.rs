//! voicsh - Voice typing for Wayland Linux
//!
//! Offline-first voice-to-text with optional LLM refinement.

pub mod app;
pub mod audio;
pub mod cli;
pub mod config;
pub mod defaults;
pub mod diagnostics;
pub mod error;
pub mod input;
pub mod ipc;
pub mod models;
pub mod pipeline;
pub mod streaming;
pub mod stt;

// Re-export key traits for external consumers
pub use audio::recorder::AudioSource;
pub use input::injector::{CommandExecutor, SystemCommandExecutor, TextInjector};
pub use stt::transcriber::Transcriber;
