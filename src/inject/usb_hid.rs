//! USB HID Gadget keyboard backend.
//!
//! Writes HID keyboard reports to `/dev/hidg0` (Linux USB Gadget configfs).
//! Used for Raspberry Pi (or similar) acting as a USB keyboard.

use crate::error::{Result, VoicshError};
use crate::pipeline::sink::TextSink;
use crate::pipeline::types::SinkEvent;
use std::io::Write;
use std::time::Duration;

// ── HID Keyboard Report ────────────────────────────────────────────────

/// Standard USB HID keyboard report (8 bytes).
///
/// Format: `[modifier, reserved, key1, key2, key3, key4, key5, key6]`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HidReport {
    pub modifier: u8,
    pub keys: [u8; 6],
}

impl HidReport {
    pub const EMPTY: [u8; 8] = [0; 8];

    pub fn to_bytes(self) -> [u8; 8] {
        [
            self.modifier,
            0, // reserved
            self.keys[0],
            self.keys[1],
            self.keys[2],
            self.keys[3],
            self.keys[4],
            self.keys[5],
        ]
    }

    pub fn key(scancode: u8, modifier: u8) -> Self {
        Self {
            modifier,
            keys: [scancode, 0, 0, 0, 0, 0],
        }
    }
}

// ── Modifier bits ──────────────────────────────────────────────────────

pub const MOD_NONE: u8 = 0x00;
pub const MOD_LCTRL: u8 = 0x01;
pub const MOD_LSHIFT: u8 = 0x02;
pub const MOD_LALT: u8 = 0x04;
pub const MOD_RALT: u8 = 0x40; // AltGr on international keyboards

// ── USB HID Scancodes (Usage IDs from HID Usage Tables) ────────────────

pub const KEY_NONE: u8 = 0x00;
pub const KEY_A: u8 = 0x04;
pub const KEY_B: u8 = 0x05;
pub const KEY_C: u8 = 0x06;
pub const KEY_D: u8 = 0x07;
pub const KEY_E: u8 = 0x08;
pub const KEY_F: u8 = 0x09;
pub const KEY_G: u8 = 0x0A;
pub const KEY_H: u8 = 0x0B;
pub const KEY_I: u8 = 0x0C;
pub const KEY_J: u8 = 0x0D;
pub const KEY_K: u8 = 0x0E;
pub const KEY_L: u8 = 0x0F;
pub const KEY_M: u8 = 0x10;
pub const KEY_N: u8 = 0x11;
pub const KEY_O: u8 = 0x12;
pub const KEY_P: u8 = 0x13;
pub const KEY_Q: u8 = 0x14;
pub const KEY_R: u8 = 0x15;
pub const KEY_S: u8 = 0x16;
pub const KEY_T: u8 = 0x17;
pub const KEY_U: u8 = 0x18;
pub const KEY_V: u8 = 0x19;
pub const KEY_W: u8 = 0x1A;
pub const KEY_X: u8 = 0x1B;
pub const KEY_Y: u8 = 0x1C;
pub const KEY_Z: u8 = 0x1D;
pub const KEY_1: u8 = 0x1E;
pub const KEY_2: u8 = 0x1F;
pub const KEY_3: u8 = 0x20;
pub const KEY_4: u8 = 0x21;
pub const KEY_5: u8 = 0x22;
pub const KEY_6: u8 = 0x23;
pub const KEY_7: u8 = 0x24;
pub const KEY_8: u8 = 0x25;
pub const KEY_9: u8 = 0x26;
pub const KEY_0: u8 = 0x27;
pub const KEY_ENTER: u8 = 0x28;
pub const KEY_ESC: u8 = 0x29;
pub const KEY_BACKSPACE: u8 = 0x2A;
pub const KEY_TAB: u8 = 0x2B;
pub const KEY_SPACE: u8 = 0x2C;
pub const KEY_MINUS: u8 = 0x2D;
pub const KEY_EQUAL: u8 = 0x2E;
pub const KEY_LBRACKET: u8 = 0x2F;
pub const KEY_RBRACKET: u8 = 0x30;
pub const KEY_BACKSLASH: u8 = 0x31;
pub const KEY_SEMICOLON: u8 = 0x33;
pub const KEY_APOSTROPHE: u8 = 0x34;
pub const KEY_GRAVE: u8 = 0x35;
pub const KEY_COMMA: u8 = 0x36;
pub const KEY_DOT: u8 = 0x37;
pub const KEY_SLASH: u8 = 0x38;

// ── Keyboard Layout ────────────────────────────────────────────────────

/// Keyboard layout determines which scancode+modifier to send for each character.
#[derive(Debug, Clone)]
pub enum KeyboardLayout {
    Us,
    De,
}

impl KeyboardLayout {
    pub fn from_name(name: &str) -> Result<Self> {
        match name.to_lowercase().as_str() {
            "us" | "en" | "en-us" => Ok(Self::Us),
            "de" | "de-de" => Ok(Self::De),
            other => Err(VoicshError::ConfigInvalidValue {
                key: "injection.layout".to_string(),
                message: format!("'{}' is not a valid layout. Valid layouts: us, de", other),
            }),
        }
    }

    /// Map a character to a HID report (scancode + modifier).
    /// Returns None for characters that cannot be typed on this layout.
    pub fn char_to_report(&self, ch: char) -> Option<HidReport> {
        match self {
            Self::Us => us_char_to_report(ch),
            Self::De => de_char_to_report(ch),
        }
    }
}

/// US keyboard layout mapping.
fn us_char_to_report(ch: char) -> Option<HidReport> {
    let (scancode, modifier) = match ch {
        'a'..='z' => (KEY_A + (ch as u8 - b'a'), MOD_NONE),
        'A'..='Z' => (KEY_A + (ch as u8 - b'A'), MOD_LSHIFT),
        '1' => (KEY_1, MOD_NONE),
        '2' => (KEY_2, MOD_NONE),
        '3' => (KEY_3, MOD_NONE),
        '4' => (KEY_4, MOD_NONE),
        '5' => (KEY_5, MOD_NONE),
        '6' => (KEY_6, MOD_NONE),
        '7' => (KEY_7, MOD_NONE),
        '8' => (KEY_8, MOD_NONE),
        '9' => (KEY_9, MOD_NONE),
        '0' => (KEY_0, MOD_NONE),
        '!' => (KEY_1, MOD_LSHIFT),
        '@' => (KEY_2, MOD_LSHIFT),
        '#' => (KEY_3, MOD_LSHIFT),
        '$' => (KEY_4, MOD_LSHIFT),
        '%' => (KEY_5, MOD_LSHIFT),
        '^' => (KEY_6, MOD_LSHIFT),
        '&' => (KEY_7, MOD_LSHIFT),
        '*' => (KEY_8, MOD_LSHIFT),
        '(' => (KEY_9, MOD_LSHIFT),
        ')' => (KEY_0, MOD_LSHIFT),
        ' ' => (KEY_SPACE, MOD_NONE),
        '\n' => (KEY_ENTER, MOD_NONE),
        '\t' => (KEY_TAB, MOD_NONE),
        '-' => (KEY_MINUS, MOD_NONE),
        '_' => (KEY_MINUS, MOD_LSHIFT),
        '=' => (KEY_EQUAL, MOD_NONE),
        '+' => (KEY_EQUAL, MOD_LSHIFT),
        '[' => (KEY_LBRACKET, MOD_NONE),
        '{' => (KEY_LBRACKET, MOD_LSHIFT),
        ']' => (KEY_RBRACKET, MOD_NONE),
        '}' => (KEY_RBRACKET, MOD_LSHIFT),
        '\\' => (KEY_BACKSLASH, MOD_NONE),
        '|' => (KEY_BACKSLASH, MOD_LSHIFT),
        ';' => (KEY_SEMICOLON, MOD_NONE),
        ':' => (KEY_SEMICOLON, MOD_LSHIFT),
        '\'' => (KEY_APOSTROPHE, MOD_NONE),
        '"' => (KEY_APOSTROPHE, MOD_LSHIFT),
        '`' => (KEY_GRAVE, MOD_NONE),
        '~' => (KEY_GRAVE, MOD_LSHIFT),
        ',' => (KEY_COMMA, MOD_NONE),
        '<' => (KEY_COMMA, MOD_LSHIFT),
        '.' => (KEY_DOT, MOD_NONE),
        '>' => (KEY_DOT, MOD_LSHIFT),
        '/' => (KEY_SLASH, MOD_NONE),
        '?' => (KEY_SLASH, MOD_LSHIFT),
        _ => return None,
    };
    Some(HidReport::key(scancode, modifier))
}

// ── German layout scancodes ────────────────────────────────────────────
// German layout swaps Y/Z and has different symbol positions.
// Keys that differ from US: KEY_MINUS = ß, KEY_EQUAL = ´, KEY_LBRACKET = ü,
// KEY_RBRACKET = +, KEY_SEMICOLON = ö, KEY_APOSTROPHE = ä,
// KEY_BACKSLASH = #, KEY_GRAVE = ^, KEY_102ND = <

const KEY_102ND: u8 = 0x64; // non-US key left of Z on ISO keyboards

/// German keyboard layout mapping.
fn de_char_to_report(ch: char) -> Option<HidReport> {
    let (scancode, modifier) = match ch {
        // Letters: German layout swaps Y and Z
        'y' => (KEY_Z, MOD_NONE),
        'z' => (KEY_Y, MOD_NONE),
        'a'..='x' => (KEY_A + (ch as u8 - b'a'), MOD_NONE),
        'Y' => (KEY_Z, MOD_LSHIFT),
        'Z' => (KEY_Y, MOD_LSHIFT),
        'A'..='X' => (KEY_A + (ch as u8 - b'A'), MOD_LSHIFT),
        // Digits (same positions, different shift symbols)
        '0' => (KEY_0, MOD_NONE),
        '1' => (KEY_1, MOD_NONE),
        '2' => (KEY_2, MOD_NONE),
        '3' => (KEY_3, MOD_NONE),
        '4' => (KEY_4, MOD_NONE),
        '5' => (KEY_5, MOD_NONE),
        '6' => (KEY_6, MOD_NONE),
        '7' => (KEY_7, MOD_NONE),
        '8' => (KEY_8, MOD_NONE),
        '9' => (KEY_9, MOD_NONE),
        // Shift+digit symbols on German layout
        '!' => (KEY_1, MOD_LSHIFT),
        '"' => (KEY_2, MOD_LSHIFT),
        '§' => (KEY_3, MOD_LSHIFT),
        '$' => (KEY_4, MOD_LSHIFT),
        '%' => (KEY_5, MOD_LSHIFT),
        '&' => (KEY_6, MOD_LSHIFT),
        '/' => (KEY_7, MOD_LSHIFT),
        '(' => (KEY_8, MOD_LSHIFT),
        ')' => (KEY_9, MOD_LSHIFT),
        '=' => (KEY_0, MOD_LSHIFT),
        // Common punctuation
        ' ' => (KEY_SPACE, MOD_NONE),
        '\n' => (KEY_ENTER, MOD_NONE),
        '\t' => (KEY_TAB, MOD_NONE),
        // German-specific keys
        'ß' => (KEY_MINUS, MOD_NONE),
        '?' => (KEY_MINUS, MOD_LSHIFT),
        'ü' | 'Ü' => {
            let m = if ch == 'Ü' { MOD_LSHIFT } else { MOD_NONE };
            (KEY_LBRACKET, m)
        }
        '+' => (KEY_RBRACKET, MOD_NONE),
        '*' => (KEY_RBRACKET, MOD_LSHIFT),
        'ö' | 'Ö' => {
            let m = if ch == 'Ö' { MOD_LSHIFT } else { MOD_NONE };
            (KEY_SEMICOLON, m)
        }
        'ä' | 'Ä' => {
            let m = if ch == 'Ä' { MOD_LSHIFT } else { MOD_NONE };
            (KEY_APOSTROPHE, m)
        }
        '#' => (KEY_BACKSLASH, MOD_NONE),
        '\'' => (KEY_BACKSLASH, MOD_LSHIFT),
        '^' => (KEY_GRAVE, MOD_NONE),
        '°' => (KEY_GRAVE, MOD_LSHIFT),
        ',' => (KEY_COMMA, MOD_NONE),
        ';' => (KEY_COMMA, MOD_LSHIFT),
        '.' => (KEY_DOT, MOD_NONE),
        ':' => (KEY_DOT, MOD_LSHIFT),
        '-' => (KEY_SLASH, MOD_NONE),
        '_' => (KEY_SLASH, MOD_LSHIFT),
        '<' => (KEY_102ND, MOD_NONE),
        '>' => (KEY_102ND, MOD_LSHIFT),
        // AltGr combinations
        '@' => (KEY_Q, MOD_RALT),
        '~' => (KEY_RBRACKET, MOD_RALT),
        '\\' => (KEY_MINUS, MOD_RALT),
        '[' => (KEY_8, MOD_RALT),
        ']' => (KEY_9, MOD_RALT),
        '{' => (KEY_7, MOD_RALT),
        '}' => (KEY_0, MOD_RALT),
        '|' => (KEY_102ND, MOD_RALT),
        '€' => (KEY_E, MOD_RALT),
        _ => return None,
    };
    Some(HidReport::key(scancode, modifier))
}

// ── Key combo parsing ──────────────────────────────────────────────────

/// Parse a key combo string like "ctrl+BackSpace" into a HID report.
fn parse_key_combo(combo: &str) -> Option<HidReport> {
    let parts: Vec<&str> = combo.split('+').collect();
    let mut modifier = MOD_NONE;
    let mut key_name = "";

    for part in &parts {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => modifier |= MOD_LCTRL,
            "shift" => modifier |= MOD_LSHIFT,
            "alt" => modifier |= MOD_LALT,
            "altgr" => modifier |= MOD_RALT,
            _ => key_name = part,
        }
    }

    let scancode = match key_name.to_lowercase().as_str() {
        "backspace" => KEY_BACKSPACE,
        "enter" | "return" => KEY_ENTER,
        "tab" => KEY_TAB,
        "escape" | "esc" => KEY_ESC,
        "space" => KEY_SPACE,
        s if s.len() == 1 => {
            let ch = s.chars().next()?;
            match ch {
                'a'..='z' => KEY_A + (ch as u8 - b'a'),
                '0' => KEY_0,
                '1'..='9' => KEY_1 + (ch as u8 - b'1'),
                _ => return None,
            }
        }
        _ => return None,
    };

    Some(HidReport::key(scancode, modifier))
}

// ── HID Device Writer trait ────────────────────────────────────────────

/// Trait for writing HID reports. Enables testing without a real device.
pub trait HidWriter: Send + 'static {
    fn write_report(&mut self, report: &[u8]) -> Result<()>;
}

/// Writes HID reports to a real `/dev/hidgN` device file.
pub struct DeviceHidWriter {
    device: std::fs::File,
}

impl DeviceHidWriter {
    pub fn open(path: &str) -> Result<Self> {
        let device = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|e| VoicshError::InjectionFailed {
                message: format!(
                    "Failed to open HID device '{}': {}.\n\
                     Hint: Ensure the USB gadget is configured.\n\
                     Run: voicsh setup-gadget (or see VOICE_KEYBOARD.md)",
                    path, e
                ),
            })?;
        Ok(Self { device })
    }
}

impl HidWriter for DeviceHidWriter {
    fn write_report(&mut self, report: &[u8]) -> Result<()> {
        self.device
            .write_all(report)
            .map_err(|e| VoicshError::InjectionFailed {
                message: format!("Failed to write HID report: {e}"),
            })
    }
}

// ── UsbHidSink ─────────────────────────────────────────────────────────

/// TextSink that injects text via USB HID keyboard reports.
pub struct UsbHidSink<W: HidWriter> {
    writer: W,
    layout: KeyboardLayout,
    /// Delay between key press and release in milliseconds.
    key_delay_ms: u64,
}

impl UsbHidSink<DeviceHidWriter> {
    /// Create a UsbHidSink that writes to a real HID device.
    pub fn open(device_path: &str, layout_name: &str, key_delay_ms: u64) -> Result<Self> {
        let writer = DeviceHidWriter::open(device_path)?;
        let layout = KeyboardLayout::from_name(layout_name)?;
        Ok(Self {
            writer,
            layout,
            key_delay_ms,
        })
    }
}

impl<W: HidWriter> UsbHidSink<W> {
    /// Create a UsbHidSink with a custom writer (for testing).
    pub fn new(writer: W, layout: KeyboardLayout) -> Self {
        Self {
            writer,
            layout,
            key_delay_ms: 2,
        }
    }

    /// Send a HID key press followed by release, with inter-key delay.
    fn press_key(&mut self, report: HidReport) -> Result<()> {
        self.writer.write_report(&report.to_bytes())?;
        self.writer.write_report(&HidReport::EMPTY)?;
        if self.key_delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(self.key_delay_ms));
        }
        Ok(())
    }

    /// Type a single character using the configured layout.
    fn type_char(&mut self, ch: char) -> Result<()> {
        if let Some(report) = self.layout.char_to_report(ch) {
            self.press_key(report)?;
        }
        Ok(())
    }
}

impl<W: HidWriter> TextSink for UsbHidSink<W> {
    fn handle(&mut self, text: &str) -> Result<()> {
        for ch in text.chars() {
            self.type_char(ch)?;
        }
        Ok(())
    }

    fn handle_events(&mut self, events: &[SinkEvent]) -> Result<()> {
        for event in events {
            match event {
                SinkEvent::Text(text) => {
                    self.handle(text)?;
                }
                SinkEvent::KeyCombo(combo) => {
                    if let Some(report) = parse_key_combo(combo) {
                        self.press_key(report)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "usb-hid"
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Mock HID writer that records all reports.
    struct MockHidWriter {
        reports: Arc<Mutex<Vec<[u8; 8]>>>,
    }

    impl MockHidWriter {
        fn new() -> (Self, Arc<Mutex<Vec<[u8; 8]>>>) {
            let reports = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    reports: reports.clone(),
                },
                reports,
            )
        }
    }

    impl HidWriter for MockHidWriter {
        fn write_report(&mut self, report: &[u8]) -> Result<()> {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(report);
            self.reports.lock().unwrap().push(buf);
            Ok(())
        }
    }

    fn make_sink(layout: KeyboardLayout) -> (UsbHidSink<MockHidWriter>, Arc<Mutex<Vec<[u8; 8]>>>) {
        let (writer, reports) = MockHidWriter::new();
        let mut sink = UsbHidSink::new(writer, layout);
        sink.key_delay_ms = 0; // no sleep in tests
        (sink, reports)
    }

    // ── HID Report tests ───────────────────────────────────────────

    #[test]
    fn hid_report_key_produces_correct_bytes() {
        let report = HidReport::key(KEY_A, MOD_LSHIFT);
        let bytes = report.to_bytes();
        assert_eq!(bytes, [MOD_LSHIFT, 0, KEY_A, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn hid_report_empty_is_all_zeros() {
        assert_eq!(HidReport::EMPTY, [0; 8]);
    }

    // ── US layout mapping tests ────────────────────────────────────

    #[test]
    fn us_lowercase_letters_map_correctly() {
        for (i, ch) in ('a'..='z').enumerate() {
            let report = us_char_to_report(ch).unwrap();
            assert_eq!(report.keys[0], KEY_A + i as u8, "Failed for '{ch}'");
            assert_eq!(report.modifier, MOD_NONE, "Failed for '{ch}'");
        }
    }

    #[test]
    fn us_uppercase_letters_have_shift() {
        for (i, ch) in ('A'..='Z').enumerate() {
            let report = us_char_to_report(ch).unwrap();
            assert_eq!(report.keys[0], KEY_A + i as u8, "Failed for '{ch}'");
            assert_eq!(report.modifier, MOD_LSHIFT, "Failed for '{ch}'");
        }
    }

    #[test]
    fn us_digits_map_correctly() {
        let expected = [
            KEY_1, KEY_2, KEY_3, KEY_4, KEY_5, KEY_6, KEY_7, KEY_8, KEY_9, KEY_0,
        ];
        for (i, ch) in "1234567890".chars().enumerate() {
            let report = us_char_to_report(ch).unwrap();
            assert_eq!(report.keys[0], expected[i], "Failed for '{ch}'");
            assert_eq!(report.modifier, MOD_NONE, "Failed for '{ch}'");
        }
    }

    #[test]
    fn us_shift_symbols_map_correctly() {
        let cases = [
            ('!', KEY_1),
            ('@', KEY_2),
            ('#', KEY_3),
            ('$', KEY_4),
            ('%', KEY_5),
            ('^', KEY_6),
            ('&', KEY_7),
            ('*', KEY_8),
            ('(', KEY_9),
            (')', KEY_0),
        ];
        for (ch, expected_key) in cases {
            let report = us_char_to_report(ch).unwrap();
            assert_eq!(report.keys[0], expected_key, "Failed for '{ch}'");
            assert_eq!(report.modifier, MOD_LSHIFT, "Failed for '{ch}'");
        }
    }

    #[test]
    fn us_punctuation_maps_correctly() {
        let cases = [
            (' ', KEY_SPACE, MOD_NONE),
            ('\n', KEY_ENTER, MOD_NONE),
            ('\t', KEY_TAB, MOD_NONE),
            ('-', KEY_MINUS, MOD_NONE),
            ('_', KEY_MINUS, MOD_LSHIFT),
            ('.', KEY_DOT, MOD_NONE),
            (',', KEY_COMMA, MOD_NONE),
            ('/', KEY_SLASH, MOD_NONE),
            ('?', KEY_SLASH, MOD_LSHIFT),
        ];
        for (ch, expected_key, expected_mod) in cases {
            let report = us_char_to_report(ch).unwrap();
            assert_eq!(report.keys[0], expected_key, "Failed for '{ch}'");
            assert_eq!(report.modifier, expected_mod, "Failed for '{ch}'");
        }
    }

    #[test]
    fn us_unknown_char_returns_none() {
        assert!(us_char_to_report('ä').is_none());
        assert!(us_char_to_report('€').is_none());
    }

    // ── German layout mapping tests ────────────────────────────────

    #[test]
    fn de_yz_swap() {
        let y = de_char_to_report('y').unwrap();
        assert_eq!(y.keys[0], KEY_Z, "German 'y' should be on Z key");
        assert_eq!(y.modifier, MOD_NONE);

        let z = de_char_to_report('z').unwrap();
        assert_eq!(z.keys[0], KEY_Y, "German 'z' should be on Y key");
        assert_eq!(z.modifier, MOD_NONE);
    }

    #[test]
    fn de_umlauts_map_correctly() {
        let cases = [
            ('ä', KEY_APOSTROPHE, MOD_NONE),
            ('Ä', KEY_APOSTROPHE, MOD_LSHIFT),
            ('ö', KEY_SEMICOLON, MOD_NONE),
            ('Ö', KEY_SEMICOLON, MOD_LSHIFT),
            ('ü', KEY_LBRACKET, MOD_NONE),
            ('Ü', KEY_LBRACKET, MOD_LSHIFT),
            ('ß', KEY_MINUS, MOD_NONE),
        ];
        for (ch, expected_key, expected_mod) in cases {
            let report = de_char_to_report(ch).unwrap();
            assert_eq!(report.keys[0], expected_key, "Failed for '{ch}'");
            assert_eq!(report.modifier, expected_mod, "Failed for '{ch}'");
        }
    }

    #[test]
    fn de_altgr_symbols() {
        let cases = [
            ('@', KEY_Q, MOD_RALT),
            ('€', KEY_E, MOD_RALT),
            ('{', KEY_7, MOD_RALT),
            ('}', KEY_0, MOD_RALT),
            ('[', KEY_8, MOD_RALT),
            (']', KEY_9, MOD_RALT),
            ('\\', KEY_MINUS, MOD_RALT),
            ('|', KEY_102ND, MOD_RALT),
        ];
        for (ch, expected_key, expected_mod) in cases {
            let report = de_char_to_report(ch).unwrap();
            assert_eq!(report.keys[0], expected_key, "Failed for '{ch}'");
            assert_eq!(report.modifier, expected_mod, "Failed for '{ch}'");
        }
    }

    #[test]
    fn de_shift_digit_symbols() {
        let cases = [
            ('!', KEY_1, MOD_LSHIFT),
            ('"', KEY_2, MOD_LSHIFT),
            ('§', KEY_3, MOD_LSHIFT),
            ('/', KEY_7, MOD_LSHIFT),
            ('(', KEY_8, MOD_LSHIFT),
            (')', KEY_9, MOD_LSHIFT),
            ('=', KEY_0, MOD_LSHIFT),
        ];
        for (ch, expected_key, expected_mod) in cases {
            let report = de_char_to_report(ch).unwrap();
            assert_eq!(report.keys[0], expected_key, "Failed for '{ch}'");
            assert_eq!(report.modifier, expected_mod, "Failed for '{ch}'");
        }
    }

    // ── UsbHidSink tests ───────────────────────────────────────────

    #[test]
    fn sink_handle_types_each_character() {
        let (mut sink, reports) = make_sink(KeyboardLayout::Us);

        sink.handle("Hi").unwrap();

        let reports = reports.lock().unwrap();
        // 'H' = key press + key release, 'i' = key press + key release
        assert_eq!(
            reports.len(),
            4,
            "Expected 4 reports (2 chars x 2), got {}",
            reports.len()
        );

        // 'H' (Shift + h)
        assert_eq!(reports[0][0], MOD_LSHIFT, "H should have shift");
        assert_eq!(reports[0][2], KEY_H, "H scancode");
        assert_eq!(reports[1], HidReport::EMPTY, "Release after H");

        // 'i' (no modifier)
        assert_eq!(reports[2][0], MOD_NONE, "i should have no modifier");
        assert_eq!(reports[2][2], KEY_I, "i scancode");
        assert_eq!(reports[3], HidReport::EMPTY, "Release after i");
    }

    #[test]
    fn sink_handle_skips_unmappable_chars() {
        let (mut sink, reports) = make_sink(KeyboardLayout::Us);

        // 'ä' is not on US layout — should be skipped without error
        sink.handle("aäb").unwrap();

        let reports = reports.lock().unwrap();
        // Only 'a' and 'b' should produce reports (2 chars x 2 = 4)
        assert_eq!(
            reports.len(),
            4,
            "Expected 4 reports, got {}",
            reports.len()
        );
    }

    #[test]
    fn sink_handle_events_text_and_keycombo() {
        let (mut sink, reports) = make_sink(KeyboardLayout::Us);

        let events = vec![
            SinkEvent::Text("a".to_string()),
            SinkEvent::KeyCombo("ctrl+BackSpace".to_string()),
        ];
        sink.handle_events(&events).unwrap();

        let reports = reports.lock().unwrap();
        // 'a' = press + release, ctrl+BackSpace = press + release
        assert_eq!(
            reports.len(),
            4,
            "Expected 4 reports, got {}",
            reports.len()
        );

        // 'a'
        assert_eq!(reports[0][2], KEY_A);

        // ctrl+BackSpace
        assert_eq!(reports[2][0], MOD_LCTRL, "ctrl modifier");
        assert_eq!(reports[2][2], KEY_BACKSPACE, "backspace scancode");
    }

    #[test]
    fn sink_name_is_usb_hid() {
        let (sink, _) = make_sink(KeyboardLayout::Us);
        assert_eq!(sink.name(), "usb-hid");
    }

    #[test]
    fn sink_finish_returns_none() {
        let (mut sink, _) = make_sink(KeyboardLayout::Us);
        assert_eq!(sink.finish(), None);
    }

    // ── Key combo parsing tests ────────────────────────────────────

    #[test]
    fn parse_key_combo_ctrl_backspace() {
        let report = parse_key_combo("ctrl+BackSpace").unwrap();
        assert_eq!(report.modifier, MOD_LCTRL);
        assert_eq!(report.keys[0], KEY_BACKSPACE);
    }

    #[test]
    fn parse_key_combo_ctrl_shift_v() {
        let report = parse_key_combo("ctrl+shift+v").unwrap();
        assert_eq!(report.modifier, MOD_LCTRL | MOD_LSHIFT);
        assert_eq!(report.keys[0], KEY_V);
    }

    #[test]
    fn parse_key_combo_single_key() {
        let report = parse_key_combo("enter").unwrap();
        assert_eq!(report.modifier, MOD_NONE);
        assert_eq!(report.keys[0], KEY_ENTER);
    }

    #[test]
    fn parse_key_combo_unknown_returns_none() {
        assert!(parse_key_combo("ctrl+F13").is_none());
    }

    // ── KeyboardLayout tests ───────────────────────────────────────

    #[test]
    fn keyboard_layout_from_name_valid() {
        assert!(matches!(
            KeyboardLayout::from_name("us"),
            Ok(KeyboardLayout::Us)
        ));
        assert!(matches!(
            KeyboardLayout::from_name("de"),
            Ok(KeyboardLayout::De)
        ));
        assert!(matches!(
            KeyboardLayout::from_name("en"),
            Ok(KeyboardLayout::Us)
        ));
        assert!(matches!(
            KeyboardLayout::from_name("DE"),
            Ok(KeyboardLayout::De)
        ));
    }

    #[test]
    fn keyboard_layout_from_name_invalid() {
        let result = KeyboardLayout::from_name("fr");
        assert!(result.is_err());
    }

    // ── German layout full sentence test ───────────────────────────

    #[test]
    fn de_layout_types_german_sentence() {
        let (mut sink, reports) = make_sink(KeyboardLayout::De);

        sink.handle("Hä?").unwrap();

        let reports = reports.lock().unwrap();
        // H (shift+h) + release + ä + release + ? (shift+ß) + release = 6 reports
        assert_eq!(
            reports.len(),
            6,
            "Expected 6 reports for 'Hä?', got {}",
            reports.len()
        );

        // 'H'
        assert_eq!(reports[0][0], MOD_LSHIFT);
        assert_eq!(reports[0][2], KEY_H);

        // 'ä'
        assert_eq!(reports[2][0], MOD_NONE);
        assert_eq!(reports[2][2], KEY_APOSTROPHE);

        // '?'
        assert_eq!(reports[4][0], MOD_LSHIFT);
        assert_eq!(reports[4][2], KEY_MINUS);
    }
}
