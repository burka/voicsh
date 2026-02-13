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
use std::path::PathBuf;
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
    /// KEY_BACKSPACE
    pub const BACKSPACE: i32 = 14;
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

/// Path where the portal restore token is cached.
///
/// Stored at `~/.cache/voicsh/portal_restore_token`. When a valid token is
/// present, the portal skips the permission dialog on subsequent sessions.
fn restore_token_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("voicsh")
        .join("portal_restore_token")
}

/// Load a previously saved portal restore token, if any.
fn load_restore_token() -> Option<String> {
    std::fs::read_to_string(restore_token_path())
        .ok()
        .filter(|s| !s.is_empty())
}

/// Save a portal restore token for future sessions.
fn save_restore_token(token: &str) {
    let path = restore_token_path();
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("voicsh: failed to create cache dir: {e}");
        return;
    }
    if let Err(e) = std::fs::write(&path, token) {
        eprintln!("voicsh: failed to save portal restore token: {e}");
    }
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

        // Load saved restore token to skip the permission dialog
        let saved_token = load_restore_token();

        proxy
            .select_devices(
                &session,
                DeviceType::Keyboard.into(),
                saved_token.as_deref(),
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

        // Save restore token so subsequent sessions skip the dialog
        if let Some(token) = response.restore_token() {
            save_restore_token(token);
        }

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
///
/// If the D-Bus session goes stale (common after hours/days in long-lived tmux
/// sessions), `simulate_paste` will attempt one automatic reconnect before
/// falling back to wtype/ydotool.
pub struct PortalSession {
    key_sender: std::sync::Mutex<Arc<dyn KeySender>>,
    connector: Box<dyn PortalConnector>,
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
        if let Some(fresh_addr) = crate::inject::focused_window::fresh_gnome_dbus_address() {
            unsafe {
                std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &fresh_addr);
            }
        }
        Self::with_connector(Box::new(AshpdConnector)).await
    }

    pub(crate) async fn with_connector(connector: Box<dyn PortalConnector>) -> Result<Self> {
        let handle = tokio::runtime::Handle::current();
        let key_sender = connector.connect().await?;
        Ok(Self {
            key_sender: std::sync::Mutex::new(key_sender),
            connector,
            handle,
        })
    }

    /// Simulate a paste key combo via the portal.
    ///
    /// Parses the paste_key string (e.g. "ctrl+v", "ctrl+shift+v") and sends
    /// the corresponding key press/release events through the portal.
    ///
    /// If the key sequence fails (stale D-Bus session), refreshes the D-Bus
    /// address and reconnects once before returning the error. This handles
    /// long-lived tmux/screen sessions that outlive GNOME logins.
    ///
    /// This is synchronous (blocks on the tokio runtime) so it can be called
    /// from pipeline station threads that are not tokio worker threads.
    pub fn simulate_paste(&self, paste_key: &str) -> Result<()> {
        let key_sequence = parse_paste_key(paste_key)?;
        let sender = self
            .key_sender
            .lock()
            .map_err(|e| VoicshError::InjectionFailed {
                message: format!("Portal session lock poisoned: {e}"),
            })?
            .clone();

        let first_err = match self
            .handle
            .block_on(send_key_sequence(sender.as_ref(), &key_sequence))
        {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };

        // Only retry on injection failures (D-Bus errors), not parse errors
        if !matches!(first_err, VoicshError::InjectionFailed { .. }) {
            return Err(first_err);
        }

        eprintln!("voicsh: portal key injection failed, attempting reconnect...");

        // Refresh D-Bus address from running gnome-shell
        if let Some(fresh_addr) = crate::inject::focused_window::fresh_gnome_dbus_address() {
            unsafe {
                std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &fresh_addr);
            }
        }

        match self.handle.block_on(self.connector.connect()) {
            Ok(new_sender) => {
                if let Ok(mut guard) = self.key_sender.lock() {
                    *guard = new_sender.clone();
                }
                eprintln!("voicsh: portal reconnected, retrying paste");
                self.handle
                    .block_on(send_key_sequence(new_sender.as_ref(), &key_sequence))
            }
            Err(reconnect_err) => {
                eprintln!("voicsh: portal reconnect failed: {reconnect_err}");
                Err(first_err)
            }
        }
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
            "backspace" => keycodes::BACKSPACE,
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

/// Test-support types for constructing a `PortalSession` without D-Bus.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// No-op key sender for tests that only need a valid `PortalSession`.
    pub struct NoOpKeySender;

    #[async_trait::async_trait]
    impl KeySender for NoOpKeySender {
        async fn press_key(&self, _code: i32) -> Result<()> {
            Ok(())
        }
        async fn release_key(&self, _code: i32) -> Result<()> {
            Ok(())
        }
    }

    /// Connector that always succeeds with a `NoOpKeySender`.
    pub struct NoOpConnector;

    #[async_trait::async_trait]
    impl PortalConnector for NoOpConnector {
        async fn connect(&self) -> Result<Arc<dyn KeySender>> {
            Ok(Arc::new(NoOpKeySender))
        }
    }
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

    /// Configurable connector for testing bootstrap and reconnection paths.
    ///
    /// Uses a VecDeque so multiple `connect()` calls return different results
    /// (first call = initial connect, second call = reconnect attempt, etc.).
    struct TestConnector {
        results: std::sync::Mutex<std::collections::VecDeque<Result<Arc<dyn KeySender>>>>,
    }

    impl TestConnector {
        fn success(sender: Arc<dyn KeySender>) -> Self {
            let mut q = std::collections::VecDeque::new();
            q.push_back(Ok(sender));
            Self {
                results: std::sync::Mutex::new(q),
            }
        }

        fn failure(message: &str) -> Self {
            let mut q = std::collections::VecDeque::new();
            q.push_back(Err(VoicshError::Other(message.to_string())));
            Self {
                results: std::sync::Mutex::new(q),
            }
        }

        fn sequence(results: Vec<Result<Arc<dyn KeySender>>>) -> Self {
            Self {
                results: std::sync::Mutex::new(results.into()),
            }
        }
    }

    #[async_trait::async_trait]
    impl PortalConnector for TestConnector {
        async fn connect(&self) -> Result<Arc<dyn KeySender>> {
            self.results
                .lock()
                .unwrap()
                .pop_front()
                .expect("TestConnector: no more results queued")
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
    fn parse_ctrl_backspace() {
        let codes = parse_paste_key("ctrl+BackSpace").unwrap();
        assert_eq!(codes, vec![keycodes::LEFT_CTRL, keycodes::BACKSPACE]);
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
        let session = PortalSession::with_connector(Box::new(connector))
            .await
            .unwrap();

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
        let session = PortalSession::with_connector(Box::new(connector))
            .await
            .unwrap();

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
        let session = PortalSession::with_connector(Box::new(connector))
            .await
            .unwrap();

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
        let result = PortalSession::with_connector(Box::new(connector)).await;
        assert!(result.is_err());
        match result {
            Err(VoicshError::Other(msg)) => assert!(msg.contains("unavailable"), "Got: {msg}"),
            _ => panic!("Expected Other error with unavailable message"),
        }
    }

    #[tokio::test]
    async fn test_with_connector_session_failed() {
        let connector = TestConnector::failure("Portal session creation failed");
        let result = PortalSession::with_connector(Box::new(connector)).await;
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
        let result = PortalSession::with_connector(Box::new(connector)).await;
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
        let result = PortalSession::with_connector(Box::new(connector)).await;
        assert!(result.is_err());
        match result {
            Err(VoicshError::Other(msg)) => assert!(msg.contains("start rejected"), "Got: {msg}"),
            _ => panic!("Expected Other error with start rejected message"),
        }
    }

    #[tokio::test]
    async fn test_with_connector_no_keyboard() {
        let connector = TestConnector::failure("keyboard not included");
        let result = PortalSession::with_connector(Box::new(connector)).await;
        assert!(result.is_err());
        match result {
            Err(VoicshError::Other(msg)) => {
                assert!(msg.contains("keyboard not included"), "Got: {msg}")
            }
            _ => panic!("Expected Other error with keyboard not included message"),
        }
    }

    #[test]
    fn test_restore_token_path_is_in_cache_dir() {
        let path = restore_token_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("voicsh"),
            "Path should contain 'voicsh': {path_str}"
        );
        assert!(
            path_str.ends_with("portal_restore_token"),
            "Path should end with 'portal_restore_token': {path_str}"
        );
    }

    #[test]
    fn test_save_and_load_restore_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("portal_restore_token");

        // Write token to temp path
        std::fs::write(&path, "test-token-abc123").unwrap();
        let loaded = std::fs::read_to_string(&path).unwrap();
        assert_eq!(loaded, "test-token-abc123");
    }

    #[test]
    fn test_load_restore_token_returns_none_for_missing_file() {
        // load_restore_token reads from a fixed path — if the file doesn't
        // exist (likely in CI), it should return None without error.
        let result = load_restore_token();
        // We can't assert None since the file may exist on dev machines,
        // but it must not panic.
        if let Some(token) = &result {
            assert!(!token.is_empty(), "Loaded token should not be empty");
        }
    }

    /// A KeySender that fails on the first N calls then succeeds.
    struct FailThenSucceedKeySender {
        fail_count: std::sync::Mutex<usize>,
        calls: std::sync::Mutex<Vec<(String, i32)>>,
    }

    impl FailThenSucceedKeySender {
        fn new(fail_count: usize) -> Self {
            Self {
                fail_count: std::sync::Mutex::new(fail_count),
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl KeySender for FailThenSucceedKeySender {
        async fn press_key(&self, code: i32) -> Result<()> {
            let mut count = self.fail_count.lock().unwrap();
            if *count > 0 {
                *count -= 1;
                return Err(VoicshError::InjectionFailed {
                    message: "stale D-Bus session".to_string(),
                });
            }
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

    #[tokio::test]
    async fn test_reconnect_on_stale_session() {
        // First sender fails (stale session), reconnect provides a working sender
        let stale_sender: Arc<dyn KeySender> = Arc::new(FailThenSucceedKeySender::new(1));
        let fresh_sender = Arc::new(RecordingKeySender::new());
        let fresh_sender_clone = fresh_sender.clone();

        let connector = TestConnector::sequence(vec![
            Ok(stale_sender),
            Ok(fresh_sender.clone() as Arc<dyn KeySender>),
        ]);
        let session = PortalSession::with_connector(Box::new(connector))
            .await
            .unwrap();

        let result = tokio::task::spawn_blocking(move || session.simulate_paste("ctrl+v"))
            .await
            .unwrap();

        assert!(result.is_ok(), "Expected reconnect to succeed: {result:?}");
        // The fresh sender should have received the key events after reconnect
        let calls = fresh_sender_clone.calls();
        assert_eq!(calls.len(), 4, "Expected 4 key events after reconnect");
        assert_eq!(calls[0], ("press".to_string(), keycodes::LEFT_CTRL));
        assert_eq!(calls[1], ("press".to_string(), keycodes::V));
        assert_eq!(calls[2], ("release".to_string(), keycodes::V));
        assert_eq!(calls[3], ("release".to_string(), keycodes::LEFT_CTRL));
    }

    #[tokio::test]
    async fn test_reconnect_failure_returns_original_error() {
        // First sender fails, reconnect also fails — should return the original error
        let stale_sender: Arc<dyn KeySender> = Arc::new(FailThenSucceedKeySender::new(1));

        let connector = TestConnector::sequence(vec![
            Ok(stale_sender),
            Err(VoicshError::Other("reconnect failed".to_string())),
        ]);
        let session = PortalSession::with_connector(Box::new(connector))
            .await
            .unwrap();

        let result = tokio::task::spawn_blocking(move || session.simulate_paste("ctrl+v"))
            .await
            .unwrap();

        assert!(result.is_err());
        match result {
            Err(VoicshError::InjectionFailed { message }) => {
                // Should be the original injection error, not the reconnect error
                assert!(
                    message.contains("stale D-Bus session"),
                    "Expected original error, got: {message}"
                );
            }
            other => panic!("Expected InjectionFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_no_reconnect_on_parse_error() {
        // Parse errors (non-InjectionFailed) should NOT trigger reconnect
        let recorder = Arc::new(RecordingKeySender::new());
        let connector = TestConnector::success(recorder);
        let session = PortalSession::with_connector(Box::new(connector))
            .await
            .unwrap();

        let result = tokio::task::spawn_blocking(move || session.simulate_paste("alt+v"))
            .await
            .unwrap();

        assert!(result.is_err());
        match result {
            Err(VoicshError::InjectionFailed { message }) => {
                assert!(
                    message.contains("Unknown key"),
                    "Expected parse error, got: {message}"
                );
            }
            other => panic!("Expected InjectionFailed parse error, got: {other:?}"),
        }
    }
}
