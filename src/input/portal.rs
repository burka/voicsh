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

/// Evdev keycodes for paste key simulation.
/// These are standard Linux input event codes (from linux/input-event-codes.h).
mod keycodes {
    /// KEY_LEFTCTRL
    pub const LEFT_CTRL: i32 = 29;
    /// KEY_LEFTSHIFT
    pub const LEFT_SHIFT: i32 = 42;
    /// KEY_V
    pub const V: i32 = 47;
}

/// Active RemoteDesktop portal session for keyboard input injection.
///
/// Holds the D-Bus proxy and session handle. The session remains active
/// as long as this struct lives (the portal closes it on Drop via Session).
///
/// Created via `try_new()` in an async context; `simulate_paste()` is sync
/// (uses `Handle::block_on`) so it can be called from pipeline station threads.
pub struct PortalSession {
    proxy: RemoteDesktop<'static>,
    session: Session<'static, RemoteDesktop<'static>>,
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
        let handle = tokio::runtime::Handle::current();

        // Fix stale D-Bus in long-lived tmux/byobu/screen sessions:
        // our DBUS_SESSION_BUS_ADDRESS may point to a dead bus from a
        // previous GNOME login. Refresh it from the running gnome-shell.
        if let Some(fresh_addr) = crate::input::focused_window::fresh_gnome_dbus_address() {
            // SAFETY: called at startup before pipeline threads are spawned,
            // so no concurrent env reads.
            unsafe {
                std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &fresh_addr);
            }
        }

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
            .map_err(|e| VoicshError::Other(format!("Portal device selection failed: {e}")))?;

        // Start the session. On first run this shows a permission dialog.
        // With PersistMode::ExplicitlyRevoked, subsequent runs skip it.
        let _response = proxy
            .start(&session, None)
            .await
            .map_err(|e| VoicshError::Other(format!("Portal session start failed: {e}")))?
            .response()
            .map_err(|e| VoicshError::Other(format!("Portal session start rejected: {e}")))?;

        Ok(Self {
            proxy,
            session,
            handle,
        })
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
        self.handle.block_on(self.send_key_sequence(&key_sequence))
    }

    /// Send a sequence of keycodes as press-all then release-all-reversed.
    async fn send_key_sequence(&self, codes: &[i32]) -> Result<()> {
        // Press all keys in order
        for &code in codes {
            self.proxy
                .notify_keyboard_keycode(&self.session, code, KeyState::Pressed)
                .await
                .map_err(|e| VoicshError::InjectionFailed {
                    message: format!("Portal key press failed: {e}"),
                })?;
        }

        // Release all keys in reverse order
        for &code in codes.iter().rev() {
            self.proxy
                .notify_keyboard_keycode(&self.session, code, KeyState::Released)
                .await
                .map_err(|e| VoicshError::InjectionFailed {
                    message: format!("Portal key release failed: {e}"),
                })?;
        }

        Ok(())
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
}
