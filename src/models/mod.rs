//! Whisper model management.

pub mod catalog;
pub mod correction_catalog;
pub mod download;
#[cfg(feature = "model-download")]
pub mod remote;
