//! Default configuration constants for voicsh.
//!
//! This module provides shared constants used across different configuration types
//! to ensure consistency and eliminate duplication.

/// Default audio sample rate in Hz.
///
/// 16kHz is the standard for speech recognition and provides a good balance
/// between quality and computational efficiency for voice applications.
pub const SAMPLE_RATE: u32 = 16000;

/// Default Voice Activity Detection (VAD) threshold.
///
/// This RMS-based threshold (0.0 to 1.0) determines when audio is considered speech.
/// A value of 0.02 is tuned for typical microphone input levels and provides
/// good sensitivity while filtering out background noise.
pub const VAD_THRESHOLD: f32 = 0.02;

/// Default silence duration in milliseconds before speech is considered ended.
///
/// 1500ms (1.5 seconds) allows for natural pauses in speech without prematurely
/// ending the recording session.
pub const SILENCE_DURATION_MS: u32 = 1500;

/// Default Whisper model name.
///
/// "base" (multilingual) supports auto-detection of any language.
/// Use "base.en" explicitly for English-only optimized transcription.
pub const DEFAULT_MODEL: &str = "base";

/// Default language code for transcription.
///
/// "auto" lets Whisper detect the spoken language automatically.
/// Set to a specific code (e.g., "en", "de") to force a language.
pub const DEFAULT_LANGUAGE: &str = "auto";

/// Language value that triggers automatic language detection.
pub const AUTO_LANGUAGE: &str = "auto";

/// Suffix for English-only model variants.
pub const ENGLISH_ONLY_SUFFIX: &str = ".en";

/// English language code.
pub const ENGLISH_LANGUAGE: &str = "en";

/// Default transcription buffer duration in seconds.
///
/// Controls how many seconds of audio chunks the pipeline will buffer
/// while waiting for transcription. Increase for slower hardware.
pub const BUFFER_SECS: u64 = 10;

/// Minimum RMS energy for a chunk to be worth transcribing.
///
/// Chunks below this are silence/ambient noise — skip Whisper entirely.
/// VAD speech threshold is 0.02; this is set 20× lower to only reject
/// truly silent chunks while allowing anything borderline.
pub const MIN_ENERGY_FOR_TRANSCRIPTION: f32 = 0.001;

/// Pre-speech buffer duration in milliseconds.
///
/// Silence samples kept in a ring buffer while idle, prepended when speech starts.
/// Captures soft onsets (plosives, fricatives) that occur before energy crosses the VAD threshold.
pub const PRE_SPEECH_MS: u32 = 500;

/// Post-speech padding duration in milliseconds.
///
/// Minimum trailing silence included in an emitted chunk.
/// Ensures word endings are not clipped when the gap threshold is very short.
pub const POST_SPEECH_MS: u32 = 150;

/// Report the GPU backend compiled into this build.
///
/// Returns a human-readable name based on the compile-time feature flags.
/// Only one GPU backend can be active at a time; if none is enabled, returns "CPU".
pub fn gpu_backend() -> &'static str {
    if cfg!(feature = "cuda") {
        "CUDA"
    } else if cfg!(feature = "vulkan") {
        "Vulkan"
    } else if cfg!(feature = "hipblas") {
        "HipBLAS (AMD)"
    } else if cfg!(feature = "openblas") {
        "OpenBLAS"
    } else {
        "CPU"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_backend_matches_compiled_feature() {
        let expected = if cfg!(feature = "cuda") {
            "CUDA"
        } else if cfg!(feature = "vulkan") {
            "Vulkan"
        } else if cfg!(feature = "hipblas") {
            "HipBLAS (AMD)"
        } else if cfg!(feature = "openblas") {
            "OpenBLAS"
        } else {
            "CPU"
        };
        assert_eq!(gpu_backend(), expected);
    }
}
