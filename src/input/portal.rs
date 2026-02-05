//! XDG Desktop Portal RemoteDesktop session for keyboard input simulation.
//!
//! Uses the `org.freedesktop.portal.RemoteDesktop` D-Bus interface via `ashpd`
//! to inject keystrokes on compositors that support it (GNOME 45+, KDE 6.1+).
//!
//! This bypasses the need for `wtype` (which fails on GNOME/Mutter) and
//! `ydotool` (which requires a daemon and uinput permissions).

use crate::error::{Result, VoicshError};
use ashpd::desktop::PersistMode;
use ashpd::desktop::Session;
use ashpd::desktop::remote_desktop::{DeviceType, KeyState, RemoteDesktop};
use std::sync::Arc;

/// Evdev keycodes for paste key simulation.
/// These are standard Linux input event codes (from linux/input-event-codes.h).
pub(crate) mod keycodes {
    /// KEY_LEFTCTRL
    pub const LEFT_CTRL: i32 = 29;
    /// KEY_LEFTSHIFT
    pub const LEFT_SHIFT: i32 = 42;
    /// KEY_V
    pub const V: i32 = 47;
}

/// Trait for sending individual key events, enabling mock D-Bus in tests.
#[async_trait::async_trait]
pub(crate) trait KeySender: Send + Sync {
    async fn press_key(&self, code: i32) -> Result<()>;
    async fn release_key(&self, code: i32) -> Result<()>;
}

/// Abstracts the D-Bus portal bootstrap sequence.
#[async_trait::async_trait]
pub(crate) trait PortalConnector: Send + Sync {
    /// Create session, select devices, start, verify keyboard → KeySender.
    async fn connect(&self) -> Result<Arc<dyn KeySender>>;
}

/// Real D-Bus KeySender wrapping ashpd RemoteDesktop proxy + session.
struct PortalKeySender {
    proxy: RemoteDesktop<'static>,
    session: Session<'static, RemoteDesktop<'static>>,
}

#[async_trait::async_trait]
impl KeySender for PortalKeySender {
    async fn press_key(&self, code: i32) -> Result<()> {
        self.proxy
            .notify_keyboard_keycode(&self.session, code, KeyState::Pressed)
            .await
            .map_err(|e| VoicshError::InjectionFailed {
                message: format!("Portal key press failed: {e}"),
            })
    }

    async fn release_key(&self, code: i32) -> Result<()> {
        self.proxy
            .notify_keyboard_keycode(&self.session, code, KeyState::Released)
            .await
            .map_err(|e| VoicshError::InjectionFailed {
                message: format!("Portal key release failed: {e}"),
            })
    }
}

/// Send a sequence of keycodes as press-all then release-all-reversed.
///
/// Small delays between events ensure the compositor registers the
/// modifier+key combo (without delays, all events may arrive in the
/// same input frame and the combo isn't recognized).
async fn send_key_sequence(sender: &dyn KeySender, codes: &[i32]) -> Result<()> {
    let delay = std::time::Duration::from_millis(5);

    // Press all keys in order (modifier first, then key)
    for &code in codes {
        sender.press_key(code).await?;
        tokio::time::sleep(delay).await;
    }

    // Release all keys in reverse order (key first, then modifier)
    for &code in codes.iter().rev() {
        sender.release_key(code).await?;
        tokio::time::sleep(delay).await;
    }

    Ok(())
}

/// Production connector using ashpd D-Bus RemoteDesktop portal.
struct AshpdConnector;

#[async_trait::async_trait]
impl PortalConnector for AshpdConnector {
    async fn connect(&self) -> Result<Arc<dyn KeySender>> {
        let proxy = RemoteDesktop::new()
            .await
            .map_err(|e| VoicshError::Other(format!("Portal RemoteDesktop unavailable: {e}")))?;

        let session = proxy
            .create_session()
            .await
            .map_err(|e| VoicshError::Other(format!("Portal session creation failed: {e}")))?;

        proxy
            .select_devices(
                &session,
                DeviceType::Keyboard.into(),
                None,
                PersistMode::ExplicitlyRevoked,
            )
            .await
            .map_err(|e| VoicshError::Other(format!("Portal device selection failed: {e}")))?
            .response()
            .map_err(|e| VoicshError::Other(format!("Portal device selection rejected: {e}")))?;

        let response = proxy
            .start(&session, None)
            .await
            .map_err(|e| VoicshError::Other(format!("Portal session start failed: {e}")))?
            .response()
            .map_err(|e| VoicshError::Other(format!("Portal session start rejected: {e}")))?;

        let devices = response.devices();
        if !devices.contains(DeviceType::Keyboard) {
            return Err(VoicshError::Other(format!(
                "Portal granted devices {:?} but keyboard not included",
                devices
            )));
        }

        Ok(Arc::new(PortalKeySender { proxy, session }))
    }
}

/// Active RemoteDesktop portal session for keyboard input injection.
///
/// Holds a `KeySender` (real D-Bus or mock) and a tokio `Handle`.
/// The session remains active as long as this struct lives.
///
/// Created via `try_new()` in an async context; `simulate_paste()` is sync
/// (uses `Handle::block_on`) so it can be called from pipeline station threads.
pub struct PortalSession {
    key_sender: Arc<dyn KeySender>,
    handle: tokio::runtime::Handle,
}

impl PortalSession {
    /// Attempt to create a new portal RemoteDesktop session.
    ///
    /// This will:
    /// 1. Connect to the portal D-Bus service
    /// 2. Create a session requesting keyboard access
    /// 3. Start the session (may show a one-time permission dialog on GNOME)
    ///
    /// Uses `PersistMode::ExplicitlyRevoked` so the user only sees the
    /// permission dialog once (until they revoke it in system settings).
    ///
    /// Returns `Err` if the portal is unavailable or the user denies access.
    pub async fn try_new() -> Result<Self> {
        // Fix stale D-Bus in long-lived tmux/byobu/screen sessions
        if let Some(fresh_addr) = crate::input::focused_window::fresh_gnome_dbus_address() {
            unsafe {
                std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &fresh_addr);
            }
        }
        Self::with_connector(&AshpdConnector).await
    }

    async fn with_connector(connector: &dyn PortalConnector) -> Result<Self> {
        let handle = tokio::runtime::Handle::current();
        let key_sender = connector.connect().await?;
        Ok(Self { key_sender, handle })
    }

    /// Simulate a paste key combo via the portal.
    ///
    /// Parses the paste_key string (e.g. "ctrl+v", "ctrl+shift+v") and sends
    /// the corresponding key press/release events through the portal.
    ///
    /// This is synchronous (blocks on the tokio runtime) so it can be called
    /// from pipeline station threads that are not tokio worker threads.
    pub fn simulate_paste(&self, paste_key: &str) -> Result<()> {
        let key_sequence = parse_paste_key(paste_key)?;
        self.handle
            .block_on(send_key_sequence(self.key_sender.as_ref(), &key_sequence))
    }
}

/// Parse a paste key string into a sequence of evdev keycodes.
///
/// Supports: "ctrl+v", "ctrl+shift+v"
fn parse_paste_key(paste_key: &str) -> Result<Vec<i32>> {
    let parts: Vec<&str> = paste_key.split('+').collect();
    let mut codes = Vec::with_capacity(parts.len());

    for part in &parts {
        let code = match part.to_lowercase().as_str() {
            "ctrl" | "control" => keycodes::LEFT_CTRL,
            "shift" => keycodes::LEFT_SHIFT,
            "v" => keycodes::V,
            other => {
                return Err(VoicshError::InjectionFailed {
                    message: format!("Unknown key in paste combo: '{other}'"),
                });
            }
        };
        codes.push(code);
    }

    if codes.is_empty() {
        return Err(VoicshError::InjectionFailed {
            message: "Empty paste key string".to_string(),
        });
    }

    Ok(codes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock KeySender that records all press/release calls for test verification.
    struct RecordingKeySender {
        calls: std::sync::Mutex<Vec<(String, i32)>>,
    }

    impl RecordingKeySender {
        fn new() -> Self {
            Self {
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(String, i32)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl KeySender for RecordingKeySender {
        async fn press_key(&self, code: i32) -> Result<()> {
            self.calls.lock().unwrap().push(("press".to_string(), code));
            Ok(())
        }

        async fn release_key(&self, code: i32) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(("release".to_string(), code));
            Ok(())
        }
    }

    /// Configurable connector for testing bootstrap error paths.
    struct TestConnector {
        result: std::sync::Mutex<Option<Result<Arc<dyn KeySender>>>>,
    }

    impl TestConnector {
        fn success(sender: Arc<dyn KeySender>) -> Self {
            Self {
                result: std::sync::Mutex::new(Some(Ok(sender))),
            }
        }

        fn failure(message: &str) -> Self {
            Self {
                result: std::sync::Mutex::new(Some(Err(VoicshError::Other(message.to_string())))),
            }
        }
    }

    #[async_trait::async_trait]
    impl PortalConnector for TestConnector {
        async fn connect(&self) -> Result<Arc<dyn KeySender>> {
            self.result.lock().unwrap().take().unwrap()
        }
    }

    #[test]
    fn parse_ctrl_v() {
        let codes = parse_paste_key("ctrl+v").unwrap();
        assert_eq!(codes, vec![keycodes::LEFT_CTRL, keycodes::V]);
    }

    #[test]
    fn parse_ctrl_shift_v() {
        let codes = parse_paste_key("ctrl+shift+v").unwrap();
        assert_eq!(
            codes,
            vec![keycodes::LEFT_CTRL, keycodes::LEFT_SHIFT, keycodes::V]
        );
    }

    #[test]
    fn parse_case_insensitive() {
        let codes = parse_paste_key("Ctrl+Shift+V").unwrap();
        assert_eq!(
            codes,
            vec![keycodes::LEFT_CTRL, keycodes::LEFT_SHIFT, keycodes::V]
        );
    }

    #[test]
    fn parse_control_alias() {
        let codes = parse_paste_key("control+v").unwrap();
        assert_eq!(codes, vec![keycodes::LEFT_CTRL, keycodes::V]);
    }

    #[test]
    fn parse_unknown_key_fails() {
        let result = parse_paste_key("alt+v");
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_fails() {
        let result = parse_paste_key("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_trailing_plus() {
        let result = parse_paste_key("ctrl+v+");
        assert!(result.is_err());
        if let Err(VoicshError::InjectionFailed { message }) = result {
            // Empty string after last '+' should be treated as unknown key
            assert!(message.contains("Unknown key") || message.contains("''"));
        } else {
            panic!("Expected InjectionFailed error");
        }
    }

    #[test]
    fn test_parse_leading_plus() {
        let result = parse_paste_key("+ctrl+v");
        assert!(result.is_err());
        if let Err(VoicshError::InjectionFailed { message }) = result {
            // Empty string before first '+' should be treated as unknown key
            assert!(message.contains("Unknown key") || message.contains("''"));
        } else {
            panic!("Expected InjectionFailed error");
        }
    }

    #[test]
    fn test_parse_double_plus() {
        let result = parse_paste_key("ctrl++v");
        assert!(result.is_err());
        if let Err(VoicshError::InjectionFailed { message }) = result {
            // Empty string between '++' should be treated as unknown key
            assert!(message.contains("Unknown key") || message.contains("''"));
        } else {
            panic!("Expected InjectionFailed error");
        }
    }

    #[test]
    fn test_parse_only_plus() {
        let result = parse_paste_key("+");
        assert!(result.is_err());
        // All parts empty, should error with unknown key
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_whitespace_in_key() {
        let result = parse_paste_key("ctrl + v");
        assert!(result.is_err());
        if let Err(VoicshError::InjectionFailed { message }) = result {
            // " ctrl " won't match "ctrl", should error with " ctrl " as unknown
            assert!(message.contains("Unknown key"));
        } else {
            panic!("Expected InjectionFailed error");
        }
    }

    #[test]
    fn test_parse_single_modifier() {
        let codes = parse_paste_key("ctrl").unwrap();
        assert_eq!(codes, vec![keycodes::LEFT_CTRL]);
    }

    #[test]
    fn test_parse_single_key_v() {
        let codes = parse_paste_key("v").unwrap();
        assert_eq!(codes, vec![keycodes::V]);
    }

    #[test]
    fn test_parse_shift_only() {
        let codes = parse_paste_key("shift").unwrap();
        assert_eq!(codes, vec![keycodes::LEFT_SHIFT]);
    }

    #[test]
    fn test_parse_all_keys_combined() {
        // Test order preservation: shift first, then ctrl, then v
        let codes = parse_paste_key("shift+ctrl+v").unwrap();
        assert_eq!(
            codes,
            vec![keycodes::LEFT_SHIFT, keycodes::LEFT_CTRL, keycodes::V]
        );
    }

    #[test]
    fn test_parse_error_message_contains_key() {
        let result = parse_paste_key("alt+v");
        assert!(result.is_err());
        // Extract error and verify it contains the unknown key name
        match result {
            Err(VoicshError::InjectionFailed { message }) => {
                assert!(
                    message.contains("alt"),
                    "Error message should contain 'alt' but got: {}",
                    message
                );
            }
            _ => panic!("Expected InjectionFailed error variant"),
        }
    }

    #[test]
    fn portal_session_is_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<PortalSession>();
        assert_sync::<PortalSession>();
    }

    #[test]
    fn keycodes_match_linux_evdev() {
        assert_eq!(keycodes::LEFT_CTRL, 29);
        assert_eq!(keycodes::LEFT_SHIFT, 42);
        assert_eq!(keycodes::V, 47);
    }

    #[tokio::test]
    async fn test_send_key_sequence_press_release_order() {
        let sender = RecordingKeySender::new();
        send_key_sequence(&sender, &[keycodes::LEFT_CTRL, keycodes::V])
            .await
            .unwrap();

        let calls = sender.calls();
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[0], ("press".to_string(), 29)); // press ctrl
        assert_eq!(calls[1], ("press".to_string(), 47)); // press v
        assert_eq!(calls[2], ("release".to_string(), 47)); // release v
        assert_eq!(calls[3], ("release".to_string(), 29)); // release ctrl
    }

    #[tokio::test]
    async fn test_send_key_sequence_empty() {
        let sender = RecordingKeySender::new();
        send_key_sequence(&sender, &[]).await.unwrap();
        assert!(sender.calls().is_empty());
    }

    #[tokio::test]
    async fn test_send_key_sequence_press_error() {
        struct FailingKeySender;

        #[async_trait::async_trait]
        impl KeySender for FailingKeySender {
            async fn press_key(&self, _code: i32) -> Result<()> {
                Err(VoicshError::InjectionFailed {
                    message: "press failed".to_string(),
                })
            }
            async fn release_key(&self, _code: i32) -> Result<()> {
                Ok(())
            }
        }

        let result = send_key_sequence(&FailingKeySender, &[29]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_key_sequence_release_error() {
        struct ReleaseFailKeySender;

        #[async_trait::async_trait]
        impl KeySender for ReleaseFailKeySender {
            async fn press_key(&self, _code: i32) -> Result<()> {
                Ok(())
            }
            async fn release_key(&self, _code: i32) -> Result<()> {
                Err(VoicshError::InjectionFailed {
                    message: "release failed".to_string(),
                })
            }
        }

        let result = send_key_sequence(&ReleaseFailKeySender, &[29]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_simulate_paste_with_mock() {
        let recorder = Arc::new(RecordingKeySender::new());
        let connector = TestConnector::success(recorder.clone());
        let session = PortalSession::with_connector(&connector).await.unwrap();

        // Use spawn_blocking since simulate_paste calls block_on
        let recorder_clone = recorder.clone();
        let result = tokio::task::spawn_blocking(move || session.simulate_paste("ctrl+v"))
            .await
            .unwrap();

        assert!(result.is_ok());
        let calls = recorder_clone.calls();
        assert_eq!(calls.len(), 4); // press ctrl, press v, release v, release ctrl
    }

    #[tokio::test]
    async fn test_simulate_paste_invalid_key() {
        let recorder = Arc::new(RecordingKeySender::new());
        let connector = TestConnector::success(recorder.clone());
        let session = PortalSession::with_connector(&connector).await.unwrap();

        let result = tokio::task::spawn_blocking(move || session.simulate_paste("alt+v"))
            .await
            .unwrap();

        assert!(result.is_err());
        // Should fail at parse_paste_key, no key events sent
        assert!(recorder.calls().is_empty());
    }

    #[tokio::test]
    async fn test_send_key_sequence_three_keys() {
        let sender = RecordingKeySender::new();
        send_key_sequence(
            &sender,
            &[keycodes::LEFT_CTRL, keycodes::LEFT_SHIFT, keycodes::V],
        )
        .await
        .unwrap();

        let calls = sender.calls();
        assert_eq!(calls.len(), 6); // 3 press + 3 release
        // Press order: ctrl, shift, v
        assert_eq!(calls[0].1, keycodes::LEFT_CTRL);
        assert_eq!(calls[1].1, keycodes::LEFT_SHIFT);
        assert_eq!(calls[2].1, keycodes::V);
        // Release order: v, shift, ctrl (reverse)
        assert_eq!(calls[3].1, keycodes::V);
        assert_eq!(calls[4].1, keycodes::LEFT_SHIFT);
        assert_eq!(calls[5].1, keycodes::LEFT_CTRL);
    }

    #[tokio::test]
    async fn test_with_connector_success() {
        let sender = Arc::new(RecordingKeySender::new());
        let connector = TestConnector::success(sender.clone());
        let session = PortalSession::with_connector(&connector).await.unwrap();

        // Verify session works by simulating a paste
        let result = tokio::task::spawn_blocking(move || session.simulate_paste("ctrl+v"))
            .await
            .unwrap();
        assert!(result.is_ok());
        let calls = sender.calls();
        assert_eq!(calls.len(), 4); // press ctrl, press v, release v, release ctrl
    }

    #[tokio::test]
    async fn test_with_connector_unavailable() {
        let connector = TestConnector::failure("Portal RemoteDesktop unavailable");
        let result = PortalSession::with_connector(&connector).await;
        assert!(result.is_err());
        match result {
            Err(VoicshError::Other(msg)) => assert!(msg.contains("unavailable"), "Got: {msg}"),
            _ => panic!("Expected Other error with unavailable message"),
        }
    }

    #[tokio::test]
    async fn test_with_connector_session_failed() {
        let connector = TestConnector::failure("Portal session creation failed");
        let result = PortalSession::with_connector(&connector).await;
        assert!(result.is_err());
        match result {
            Err(VoicshError::Other(msg)) => {
                assert!(msg.contains("session creation failed"), "Got: {msg}")
            }
            _ => panic!("Expected Other error with session creation failed message"),
        }
    }

    #[tokio::test]
    async fn test_with_connector_device_rejected() {
        let connector = TestConnector::failure("Portal device selection rejected");
        let result = PortalSession::with_connector(&connector).await;
        assert!(result.is_err());
        match result {
            Err(VoicshError::Other(msg)) => {
                assert!(msg.contains("device selection rejected"), "Got: {msg}")
            }
            _ => panic!("Expected Other error with device selection rejected message"),
        }
    }

    #[tokio::test]
    async fn test_with_connector_start_rejected() {
        let connector = TestConnector::failure("Portal session start rejected");
        let result = PortalSession::with_connector(&connector).await;
        assert!(result.is_err());
        match result {
            Err(VoicshError::Other(msg)) => assert!(msg.contains("start rejected"), "Got: {msg}"),
            _ => panic!("Expected Other error with start rejected message"),
        }
    }

    #[tokio::test]
    async fn test_with_connector_no_keyboard() {
        let connector = TestConnector::failure("keyboard not included");
        let result = PortalSession::with_connector(&connector).await;
        assert!(result.is_err());
        match result {
            Err(VoicshError::Other(msg)) => {
                assert!(msg.contains("keyboard not included"), "Got: {msg}")
            }
            _ => panic!("Expected Other error with keyboard not included message"),
        }
    }
}
