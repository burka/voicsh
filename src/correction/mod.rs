//! Post-ASR error correction.

#[cfg(feature = "error-correction")]
pub mod candle_t5;
pub mod corrector;
#[cfg(any(feature = "error-correction", feature = "symspell"))]
pub mod hybrid;
pub mod prompt;
pub mod station;
#[cfg(feature = "symspell")]
pub mod symspell;
