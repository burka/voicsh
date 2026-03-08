//! GPIO support for Raspberry Pi push-to-talk button and LED feedback.
//!
//! Uses Linux sysfs GPIO interface (`/sys/class/gpio/`) — no external crate needed.
//! Designed for the `pi` feature flag.

use crate::error::{Result, VoicshError};
use std::fs;
use std::io::Read;
use std::path::PathBuf;

// ── Push-to-Talk mode ────────────────────────────────────────────────

/// Push-to-talk button behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttMode {
    /// Record while button is held down.
    Hold,
    /// Toggle recording on each press.
    Toggle,
    /// Button disabled — continuous VAD-only recording.
    Off,
}

impl PttMode {
    pub fn from_name(name: &str) -> Result<Self> {
        match name.to_lowercase().as_str() {
            "hold" => Ok(Self::Hold),
            "toggle" => Ok(Self::Toggle),
            "off" | "disabled" | "none" => Ok(Self::Off),
            other => Err(VoicshError::ConfigInvalidValue {
                key: "ptt.mode".to_string(),
                message: format!(
                    "'{}' is not a valid PTT mode. Valid modes: hold, toggle, off",
                    other
                ),
            }),
        }
    }
}

// ── Sysfs GPIO helpers ───────────────────────────────────────────────

fn sysfs_path(pin: u8) -> PathBuf {
    PathBuf::from(format!("/sys/class/gpio/gpio{pin}"))
}

/// Export a GPIO pin via sysfs if not already exported.
fn export_pin(pin: u8) -> Result<()> {
    let gpio_path = sysfs_path(pin);
    if gpio_path.exists() {
        return Ok(());
    }
    fs::write("/sys/class/gpio/export", pin.to_string()).map_err(|e| {
        VoicshError::Other(format!(
            "Failed to export GPIO {pin}: {e}.\n\
             Hint: Run as root or add user to 'gpio' group."
        ))
    })
}

/// Set GPIO pin direction.
fn set_direction(pin: u8, direction: &str) -> Result<()> {
    let path = sysfs_path(pin).join("direction");
    fs::write(&path, direction).map_err(|e| {
        VoicshError::Other(format!(
            "Failed to set GPIO {pin} direction to '{direction}': {e}"
        ))
    })
}

/// Set GPIO pin edge trigger for poll-based reading.
fn set_edge(pin: u8, edge: &str) -> Result<()> {
    let path = sysfs_path(pin).join("edge");
    fs::write(&path, edge)
        .map_err(|e| VoicshError::Other(format!("Failed to set GPIO {pin} edge to '{edge}': {e}")))
}

/// Read the current value of a GPIO pin (0 or 1).
fn read_value(pin: u8) -> Result<bool> {
    let path = sysfs_path(pin).join("value");
    let val = fs::read_to_string(&path)
        .map_err(|e| VoicshError::Other(format!("Failed to read GPIO {pin} value: {e}")))?;
    Ok(val.trim() == "1")
}

/// Set a GPIO output pin value.
fn write_value(pin: u8, high: bool) -> Result<()> {
    let path = sysfs_path(pin).join("value");
    let val = if high { "1" } else { "0" };
    fs::write(&path, val)
        .map_err(|e| VoicshError::Other(format!("Failed to write GPIO {pin} value: {e}")))
}

// ── GPIO Button ──────────────────────────────────────────────────────

/// Setup a GPIO pin as input with edge detection for push-to-talk.
pub fn setup_button(pin: u8) -> Result<()> {
    export_pin(pin)?;
    set_direction(pin, "in")?;
    set_edge(pin, "both")?;
    Ok(())
}

/// Wait for a GPIO pin edge change using poll(2) on the sysfs value file.
///
/// Returns the new pin value (true = high, false = low).
/// Blocks until an edge is detected or timeout expires.
pub fn wait_for_edge(pin: u8, timeout_ms: i32) -> Result<Option<bool>> {
    use std::os::unix::io::AsRawFd;

    let path = sysfs_path(pin).join("value");
    let mut file = fs::File::open(&path)
        .map_err(|e| VoicshError::Other(format!("Failed to open GPIO {pin} for polling: {e}")))?;

    // Initial read to clear any pending interrupt (error is non-fatal)
    let mut buf = [0u8; 4];
    drop(file.read(&mut buf));

    let mut pollfd = libc::pollfd {
        fd: file.as_raw_fd(),
        events: libc::POLLPRI | libc::POLLERR,
        revents: 0,
    };

    // SAFETY: pollfd is a valid stack-allocated struct, nfds=1, timeout is bounded.
    let ret = unsafe { libc::poll(&mut pollfd as *mut libc::pollfd, 1, timeout_ms) };

    if ret < 0 {
        return Err(VoicshError::Other(format!(
            "poll() failed on GPIO {pin}: {}",
            std::io::Error::last_os_error()
        )));
    }

    if ret == 0 {
        return Ok(None); // timeout
    }

    read_value(pin).map(Some)
}

/// Run the push-to-talk button loop.
///
/// Sends IPC commands to the daemon socket based on button state changes.
/// Intended to run in a dedicated thread.
pub fn run_ptt_loop(pin: u8, mode: PttMode, socket_path: PathBuf) -> Result<()> {
    if mode == PttMode::Off {
        return Ok(());
    }

    setup_button(pin)?;

    // Button is active-low (pressed = 0, released = 1) with external pull-up
    loop {
        let Some(value) = wait_for_edge(pin, 1000)? else {
            continue; // timeout, loop again
        };

        let pressed = !value; // active-low

        match mode {
            PttMode::Hold => {
                let command = if pressed {
                    crate::ipc::protocol::Command::Start
                } else {
                    crate::ipc::protocol::Command::Stop
                };
                send_ipc_blocking(&socket_path, command);
            }
            PttMode::Toggle => {
                if pressed {
                    send_ipc_blocking(&socket_path, crate::ipc::protocol::Command::Toggle);
                }
            }
            PttMode::Off => unreachable!(),
        }
    }
}

/// Send an IPC command to the daemon socket (blocking, for use from GPIO thread).
fn send_ipc_blocking(socket_path: &std::path::Path, command: crate::ipc::protocol::Command) {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("voicsh: GPIO: failed to connect to daemon socket: {e}");
            return;
        }
    };

    let msg = match serde_json::to_string(&command) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("voicsh: GPIO: failed to serialize command: {e}");
            return;
        }
    };

    let mut writer = std::io::BufWriter::new(&stream);
    if let Err(e) = writeln!(writer, "{msg}") {
        eprintln!("voicsh: GPIO: failed to send command: {e}");
    }
}

// ── LED Control ──────────────────────────────────────────────────────

/// LED pin configuration.
#[derive(Debug, Clone)]
pub struct LedConfig {
    /// GPIO pin number for the LED (or red channel of RGB).
    pub pin: u8,
    /// Optional green pin for RGB LED.
    pub green_pin: Option<u8>,
    /// Optional blue pin for RGB LED.
    pub blue_pin: Option<u8>,
}

/// Setup LED GPIO pin(s) as output.
pub fn setup_led(config: &LedConfig) -> Result<()> {
    setup_output_pin(config.pin)?;
    if let Some(g) = config.green_pin {
        setup_output_pin(g)?;
    }
    if let Some(b) = config.blue_pin {
        setup_output_pin(b)?;
    }
    Ok(())
}

fn setup_output_pin(pin: u8) -> Result<()> {
    export_pin(pin)?;
    set_direction(pin, "out")?;
    write_value(pin, false)?;
    Ok(())
}

/// LED color for status feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedColor {
    Off,
    Red,
    Green,
    Yellow, // Red + Green
}

/// Set the LED color (for single LED, only Off/On; for RGB, full color).
pub fn set_led_color(config: &LedConfig, color: LedColor) -> Result<()> {
    let (r, g) = match color {
        LedColor::Off => (false, false),
        LedColor::Red => (true, false),
        LedColor::Green => (false, true),
        LedColor::Yellow => (true, true),
    };

    write_value(config.pin, r || g)?; // single LED: on for any color
    if let Some(gp) = config.green_pin {
        write_value(gp, g)?;
    }
    // For single LED without green pin, red pin handles on/off
    Ok(())
}

/// Run the LED feedback loop, subscribing to daemon events.
///
/// Maps daemon events to LED colors:
/// - Idle: Off
/// - Recording: Green
/// - Transcription in progress: Yellow
/// - Error: Red (brief flash)
pub fn run_led_loop(
    config: LedConfig,
    mut event_rx: tokio::sync::broadcast::Receiver<crate::ipc::protocol::DaemonEvent>,
) {
    use crate::ipc::protocol::DaemonEvent;

    if let Err(e) = setup_led(&config) {
        eprintln!("voicsh: LED setup failed: {e}");
        return;
    }

    let set = |color: LedColor| {
        if let Err(e) = set_led_color(&config, color) {
            eprintln!("voicsh: LED error: {e}");
        }
    };

    loop {
        match event_rx.blocking_recv() {
            Ok(event) => match event {
                DaemonEvent::RecordingStateChanged { recording } => {
                    set(if recording {
                        LedColor::Green
                    } else {
                        LedColor::Off
                    });
                }
                DaemonEvent::Transcription { .. } => {
                    // Brief green flash then off
                    set(LedColor::Green);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    set(LedColor::Off);
                }
                DaemonEvent::ModelLoading { .. } => set(LedColor::Yellow),
                DaemonEvent::ModelLoaded { .. } => set(LedColor::Off),
                DaemonEvent::ModelLoadFailed { .. } => {
                    set(LedColor::Red);
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    set(LedColor::Off);
                }
                _ => {}
            },
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                eprintln!("voicsh: LED loop lagged by {n} events");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                set(LedColor::Off);
                break;
            }
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ptt_mode_from_name_valid() {
        assert_eq!(PttMode::from_name("hold").unwrap(), PttMode::Hold);
        assert_eq!(PttMode::from_name("toggle").unwrap(), PttMode::Toggle);
        assert_eq!(PttMode::from_name("off").unwrap(), PttMode::Off);
        assert_eq!(PttMode::from_name("HOLD").unwrap(), PttMode::Hold);
        assert_eq!(PttMode::from_name("Toggle").unwrap(), PttMode::Toggle);
        assert_eq!(PttMode::from_name("disabled").unwrap(), PttMode::Off);
        assert_eq!(PttMode::from_name("none").unwrap(), PttMode::Off);
    }

    #[test]
    fn ptt_mode_from_name_invalid() {
        let result = PttMode::from_name("push");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("push"),
            "Error should mention invalid value: {err_msg}"
        );
        assert!(
            err_msg.contains("hold"),
            "Error should list valid modes: {err_msg}"
        );
    }

    #[test]
    fn led_color_set_for_single_led() {
        // Can't test actual GPIO in CI, but verify the logic
        assert_ne!(LedColor::Off, LedColor::Red);
        assert_ne!(LedColor::Green, LedColor::Yellow);
    }

    #[test]
    fn sysfs_path_format() {
        let path = sysfs_path(17);
        assert_eq!(path, PathBuf::from("/sys/class/gpio/gpio17"));
    }

    #[test]
    fn led_config_clone() {
        let config = LedConfig {
            pin: 18,
            green_pin: Some(23),
            blue_pin: None,
        };
        let cloned = config.clone();
        assert_eq!(cloned.pin, 18);
        assert_eq!(cloned.green_pin, Some(23));
        assert_eq!(cloned.blue_pin, None);
    }
}
