//! Audio capture and voice activity detection.

#[cfg(feature = "cpal-audio")]
pub mod capture;
pub mod recorder;
pub mod vad;
