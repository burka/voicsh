//! Post-ASR error correction using Flan-T5 (English only).

#[cfg(feature = "error-correction")]
pub mod candle_t5;
pub mod corrector;
pub mod prompt;
pub mod station;
