//! Speech-to-text transcription.

pub mod transcriber;
#[cfg(feature = "whisper")]
pub mod whisper;
