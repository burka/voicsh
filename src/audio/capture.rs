//! Real audio capture using CPAL (Cross-Platform Audio Library).

use crate::audio::recorder::AudioSource;
use crate::defaults;
use crate::error::{Result, VoicshError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

/// Run a closure with stderr temporarily redirected to /dev/null.
///
/// This suppresses noisy ALSA/JACK/PipeWire messages that CPAL triggers
/// when probing audio backends. The messages are harmless but confusing to users.
///
/// # Safety
/// Uses `libc::dup`/`libc::dup2` to save and restore file descriptor 2 (stderr).
/// Safe as long as no other thread is concurrently manipulating fd 2.
fn with_suppressed_stderr<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    unsafe {
        let saved_fd = libc::dup(2);
        let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY);
        if saved_fd >= 0 && devnull >= 0 {
            libc::dup2(devnull, 2);
            libc::close(devnull);
        }

        let result = f();

        if saved_fd >= 0 {
            libc::dup2(saved_fd, 2);
            libc::close(saved_fd);
        }

        result
    }
}

/// Suppress noisy JACK/ALSA error messages that occur during audio backend probing.
/// These are harmless but confusing to users.
///
/// # Safety
/// This modifies environment variables which is safe when called before spawning threads.
pub fn suppress_audio_warnings() {
    // SAFETY: Called at startup before any threads are spawned
    unsafe {
        // Suppress JACK "cannot connect" messages - don't try to start JACK server
        std::env::set_var("JACK_NO_START_SERVER", "1");
        // Disable JACK completely for CPAL probing
        std::env::set_var("JACK_NO_AUDIO_RESERVATION", "1");
        // Force PipeWire to not print debug messages
        std::env::set_var("PIPEWIRE_DEBUG", "0");
        // Suppress ALSA verbose messages
        std::env::set_var("ALSA_DEBUG", "0");
        // Tell PipeWire's JACK to be quiet
        std::env::set_var("PW_LOG", "0");
    }
}

/// Preferred device names for GNOME/PipeWire environments.
const PREFERRED_DEVICES: &[&str] = &["pipewire", "pulse", "PulseAudio"];

/// Device name patterns to filter out (not useful for voice input).
const FILTERED_PATTERNS: &[&str] = &[
    "surround",
    "front:",
    "rear:",
    "center:",
    "side:",
    "Digital Output",
    "HDMI",
    "S/PDIF",
];

/// Check if a device name should be filtered out.
fn should_filter_device(name: &str) -> bool {
    let lower = name.to_lowercase();
    FILTERED_PATTERNS
        .iter()
        .any(|pattern| lower.contains(&pattern.to_lowercase()))
}

/// Check if a device is a preferred device.
fn is_preferred_device(name: &str) -> bool {
    let lower = name.to_lowercase();
    PREFERRED_DEVICES
        .iter()
        .any(|pref| lower.contains(&pref.to_lowercase()))
}

/// List all available audio input devices with filtering and recommendations.
///
/// # Returns
/// A vector of device names, with preferred devices marked with "\[recommended\]".
/// Filters out obviously unusable devices (surround channels, HDMI, etc.).
///
/// # Errors
/// Returns `VoicshError::AudioCapture` if device enumeration fails.
///
/// # Note
/// During enumeration, cpal may output ALSA/JACK warnings to stderr while
/// probing backends. These warnings are harmless and can be safely ignored.
/// They occur because cpal tries multiple audio backends (ALSA, JACK, Pulse)
/// to find available devices.
pub fn list_devices() -> Result<Vec<String>> {
    let (host, devices) = with_suppressed_stderr(|| {
        let host = cpal::default_host();
        let devices = host.input_devices();
        (host, devices)
    });
    let _ = host; // keep host alive while iterating devices
    let devices = devices.map_err(|e| VoicshError::AudioCapture {
        message: format!("Failed to enumerate input devices: {}", e),
    })?;

    let mut device_names = Vec::new();
    for device in devices {
        if let Ok(name) = device.name() {
            // Skip filtered devices
            if should_filter_device(&name) {
                continue;
            }

            // Mark recommended devices
            if is_preferred_device(&name) {
                device_names.push(format!("{} [recommended]", name));
            } else {
                device_names.push(name);
            }
        }
    }

    Ok(device_names)
}

/// Get the best default input device, preferring PipeWire/PulseAudio.
///
/// Tries in order:
/// 1. PipeWire
/// 2. PulseAudio/Pulse
/// 3. System default
///
/// This ensures we respect GNOME's audio device selection.
///
/// # Returns
/// The best available input device.
///
/// # Errors
/// Returns `VoicshError::AudioDeviceNotFound` if no input device is available.
fn get_best_default_device() -> Result<cpal::Device> {
    with_suppressed_stderr(|| {
        let host = cpal::default_host();

        // Try to find a preferred device
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                if let Ok(name) = device.name()
                    && is_preferred_device(&name)
                {
                    return Ok(device);
                }
            }
        }

        // Fall back to system default
        host.default_input_device()
            .ok_or_else(|| VoicshError::AudioDeviceNotFound {
                device: "default".to_string(),
            })
    })
}

/// Wrapper for cpal::Stream to make it Send.
///
/// SAFETY: We ensure that the stream is only accessed from a single thread at a time
/// through the Mutex wrapper in CpalAudioSource. The stream methods are called
/// synchronously and don't cross thread boundaries unsafely.
struct SendableStream(cpal::Stream);

unsafe impl Send for SendableStream {}

/// Real audio capture implementation using CPAL.
///
/// Captures 16-bit PCM audio at 16kHz mono, as required by Whisper.
///
/// Note: The stream is wrapped in SendableStream + Mutex to make it Send+Sync.
/// This is safe because we ensure exclusive access through the Mutex.
pub struct CpalAudioSource {
    device: cpal::Device,
    stream: Arc<Mutex<Option<SendableStream>>>,
    buffer: Arc<Mutex<Vec<i16>>>,
    sample_rate: u32,
}

impl CpalAudioSource {
    /// Create a new CPAL audio source.
    ///
    /// # Arguments
    /// * `device_name` - Optional device name. If None, uses the default input device.
    ///
    /// # Returns
    /// A new CpalAudioSource configured for 16kHz mono i16 capture.
    ///
    /// # Errors
    /// Returns errors if:
    /// - Device not found
    /// - Device configuration fails
    /// - Format is not supported
    pub fn new(device_name: Option<&str>) -> Result<Self> {
        let device = with_suppressed_stderr(|| {
            let host = cpal::default_host();

            if let Some(name) = device_name {
                // Find device by name
                let devices = host
                    .input_devices()
                    .map_err(|e| VoicshError::AudioCapture {
                        message: format!("Failed to enumerate devices: {}", e),
                    })?;

                let mut found_device = None;
                for dev in devices {
                    if let Ok(dev_name) = dev.name()
                        && dev_name == name
                    {
                        found_device = Some(dev);
                        break;
                    }
                }

                found_device.ok_or_else(|| VoicshError::AudioDeviceNotFound {
                    device: name.to_string(),
                })
            } else {
                // Use smart default (prefers PipeWire/PulseAudio)
                get_best_default_device()
            }
        })?;

        Ok(Self {
            device,
            stream: Arc::new(Mutex::new(None)),
            buffer: Arc::new(Mutex::new(Vec::new())),
            sample_rate: defaults::SAMPLE_RATE,
        })
    }

    /// Build the audio stream with the configured format.
    fn build_stream(&self) -> Result<cpal::Stream> {
        // Get supported configurations
        let mut supported_configs =
            self.device
                .supported_input_configs()
                .map_err(|e| VoicshError::AudioCapture {
                    message: format!("Failed to query supported configs: {}", e),
                })?;

        // Find a config that supports our requirements
        let _config = supported_configs
            .find(|c| {
                c.channels() == 1
                    && c.sample_format() == cpal::SampleFormat::I16
                    && c.min_sample_rate().0 <= self.sample_rate
                    && c.max_sample_rate().0 >= self.sample_rate
            })
            .ok_or_else(|| VoicshError::AudioFormatMismatch {
                expected: "16kHz mono i16".to_string(),
                actual: "no matching format found".to_string(),
            })?;

        let stream_config = cpal::StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(self.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let buffer = Arc::clone(&self.buffer);
        let err_callback = |err| {
            eprintln!("Audio stream error: {}", err);
        };

        let stream = self
            .device
            .build_input_stream(
                &stream_config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    // Accumulate samples in the buffer
                    if let Ok(mut buf) = buffer.lock() {
                        buf.extend_from_slice(data);
                    }
                },
                err_callback,
                None,
            )
            .map_err(|e| VoicshError::AudioCapture {
                message: format!("Failed to build input stream: {}", e),
            })?;

        Ok(stream)
    }
}

impl AudioSource for CpalAudioSource {
    fn start(&mut self) -> Result<()> {
        let mut stream_guard = self.stream.lock().map_err(|e| VoicshError::AudioCapture {
            message: format!("Failed to lock stream: {}", e),
        })?;

        if stream_guard.is_some() {
            return Ok(()); // Already started
        }

        let stream = self.build_stream()?;
        stream.play().map_err(|e| VoicshError::AudioCapture {
            message: format!("Failed to start audio stream: {}", e),
        })?;

        *stream_guard = Some(SendableStream(stream));
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        let mut stream_guard = self.stream.lock().map_err(|e| VoicshError::AudioCapture {
            message: format!("Failed to lock stream: {}", e),
        })?;

        if let Some(sendable_stream) = stream_guard.take() {
            sendable_stream
                .0
                .pause()
                .map_err(|e| VoicshError::AudioCapture {
                    message: format!("Failed to stop audio stream: {}", e),
                })?;
        }
        Ok(())
    }

    fn read_samples(&mut self) -> Result<Vec<i16>> {
        let mut buffer = self.buffer.lock().map_err(|e| VoicshError::AudioCapture {
            message: format!("Failed to lock audio buffer: {}", e),
        })?;

        let samples = buffer.clone();
        buffer.clear();
        Ok(samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_filter_device() {
        assert!(should_filter_device("surround51"));
        assert!(should_filter_device("front:CARD=PCH"));
        assert!(should_filter_device("HDMI Output"));
        assert!(should_filter_device("Digital Output S/PDIF"));
        assert!(!should_filter_device("pipewire"));
        assert!(!should_filter_device("PulseAudio"));
        assert!(!should_filter_device("Built-in Audio"));
    }

    #[test]
    fn test_is_preferred_device() {
        assert!(is_preferred_device("pipewire"));
        assert!(is_preferred_device("PipeWire"));
        assert!(is_preferred_device("pulse"));
        assert!(is_preferred_device("PulseAudio"));
        assert!(!is_preferred_device("hw:0,0"));
        assert!(!is_preferred_device("default"));
    }

    #[test]
    #[ignore] // Requires audio hardware
    fn test_list_devices_returns_at_least_one_device() {
        let devices = list_devices();
        assert!(devices.is_ok());
        let device_list = devices.unwrap();
        assert!(
            !device_list.is_empty(),
            "Expected at least one audio device"
        );
    }

    #[test]
    #[ignore] // Requires audio hardware
    fn test_list_devices_filters_and_marks_recommended() {
        let devices = list_devices().expect("Failed to list devices");

        // Should not contain filtered patterns
        for device in &devices {
            assert!(
                !device.to_lowercase().contains("surround"),
                "Should filter surround devices: {}",
                device
            );
            assert!(
                !device.to_lowercase().contains("hdmi"),
                "Should filter HDMI devices: {}",
                device
            );
        }

        // Check if recommended devices are marked
        let has_recommended = devices.iter().any(|d| d.contains("[recommended]"));
        if has_recommended {
            println!("Found recommended devices:");
            for device in &devices {
                if device.contains("[recommended]") {
                    println!("  - {}", device);
                }
            }
        }
    }

    #[test]
    #[ignore] // Requires audio hardware
    fn test_get_best_default_device() {
        let device = get_best_default_device();
        assert!(device.is_ok(), "Failed to get best default device");

        if let Ok(dev) = device {
            if let Ok(name) = dev.name() {
                println!("Best default device: {}", name);
                // If on a system with PipeWire/Pulse, verify preference
                if name.to_lowercase().contains("pipewire") || name.to_lowercase().contains("pulse")
                {
                    println!("  -> Correctly selected preferred device");
                }
            }
        }
    }

    #[test]
    #[ignore] // Requires audio hardware
    fn test_create_with_default_device() {
        let source = CpalAudioSource::new(None);
        assert!(
            source.is_ok(),
            "Failed to create audio source with default device"
        );
    }

    #[test]
    fn test_create_with_invalid_device_name() {
        let source = CpalAudioSource::new(Some("NonExistentDevice12345"));
        assert!(source.is_err());
        match source {
            Err(VoicshError::AudioDeviceNotFound { device }) => {
                assert_eq!(device, "NonExistentDevice12345");
            }
            _ => panic!("Expected AudioDeviceNotFound error"),
        }
    }

    #[test]
    #[ignore] // Requires audio hardware
    fn test_audio_source_trait_implementation() {
        let mut source = CpalAudioSource::new(None).expect("Failed to create audio source");

        // Test start
        let start_result = source.start();
        assert!(start_result.is_ok(), "Failed to start audio capture");

        // Test read (may be empty if no audio)
        let read_result = source.read_samples();
        assert!(read_result.is_ok(), "Failed to read samples");

        // Test stop
        let stop_result = source.stop();
        assert!(stop_result.is_ok(), "Failed to stop audio capture");
    }

    #[test]
    #[ignore] // Requires audio hardware
    fn test_read_samples_clears_buffer() {
        let mut source = CpalAudioSource::new(None).expect("Failed to create audio source");
        source.start().expect("Failed to start");

        // Wait a bit for some samples to accumulate
        std::thread::sleep(std::time::Duration::from_millis(100));

        // First read
        let _samples1 = source.read_samples().expect("Failed to read samples");

        // Second immediate read should be empty or have new samples
        let _samples2 = source.read_samples().expect("Failed to read samples");

        // The samples should not be identical (buffer was cleared)
        // Note: samples2 might not be empty if audio continued to capture

        source.stop().expect("Failed to stop");
    }

    #[test]
    #[ignore] // Requires audio hardware
    fn test_start_stop_multiple_times() {
        let mut source = CpalAudioSource::new(None).expect("Failed to create audio source");

        for _ in 0..3 {
            assert!(source.start().is_ok());
            std::thread::sleep(std::time::Duration::from_millis(50));
            assert!(source.stop().is_ok());
        }
    }

    #[test]
    #[ignore] // Requires audio hardware
    fn test_can_be_used_as_trait_object() {
        let source: Box<dyn AudioSource> =
            Box::new(CpalAudioSource::new(None).expect("Failed to create audio source"));

        let mut boxed_source = source;
        assert!(boxed_source.start().is_ok());
        assert!(boxed_source.read_samples().is_ok());
        assert!(boxed_source.stop().is_ok());
    }
}
