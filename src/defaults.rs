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
/// "base.en" provides a good balance between accuracy and speed for English transcription.
pub const DEFAULT_MODEL: &str = "base.en";

/// Default language code for transcription.
///
/// "en" targets English language transcription.
pub const DEFAULT_LANGUAGE: &str = "en";
