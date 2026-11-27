//! Speech-to-text transcription.

pub mod fan_out;
pub mod idle_tracker;
#[cfg(all(feature = "whisper", feature = "model-download"))]
pub mod loader;
#[cfg(feature = "parakeet")]
pub mod parakeet;
pub mod transcriber;
#[cfg(feature = "whisper")]
pub mod whisper;
