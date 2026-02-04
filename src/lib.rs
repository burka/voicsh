//! voicsh - Voice typing for Wayland Linux
//!
//! Offline-first voice-to-text with optional LLM refinement.

pub mod audio;
pub mod cli;
pub mod config;
pub mod continuous;
pub mod defaults;
pub mod diagnostics;
pub mod error;
pub mod input;
pub mod ipc;
pub mod models;
pub mod pipeline;
pub mod streaming;
pub mod stt;
