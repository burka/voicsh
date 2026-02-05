//! Speech-to-text transcription.

pub mod fan_out;
pub mod transcriber;
#[cfg(feature = "whisper")]
pub mod whisper;
