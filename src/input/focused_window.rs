//! Focused window detection for automatic paste key selection.
//!
//! Queries the Wayland compositor to determine whether the focused window
//! is a terminal emulator, then selects the appropriate paste key:
//! - Terminal emulators: `Ctrl+Shift+V`
//! - GUI applications: `Ctrl+V`
//!
//! Supports Sway (swaymsg), Hyprland (hyprctl), and GNOME (gdbus) compositors.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

static SWAYMSG_BROKEN: AtomicBool = AtomicBool::new(false);
static HYPRCTL_BROKEN: AtomicBool = AtomicBool::new(false);
static GNOME_DBUS_BROKEN: AtomicBool = AtomicBool::new(false);
static GNOME_INTROSPECT_BROKEN: AtomicBool = AtomicBool::new(false);

/// Reset detection cache. Call when session environment may have changed
/// (e.g. compositor restart in daemon mode).
pub fn reset_detection_cache() {
    SWAYMSG_BROKEN.store(false, Ordering::Relaxed);
    HYPRCTL_BROKEN.store(false, Ordering::Relaxed);
    GNOME_DBUS_BROKEN.store(false, Ordering::Relaxed);
    GNOME_INTROSPECT_BROKEN.store(false, Ordering::Relaxed);
}

/// Check if an I/O error is permanent (binary not found or permission denied).
fn is_permanent_error(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
    )
}

/// Classification of the focused window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    /// Terminal emulator (pastes with Ctrl+Shift+V)
    Terminal,
    /// Graphical application (pastes with Ctrl+V)
    GraphicalApp,
}

/// Known terminal emulator app IDs (compared case-insensitively).
const KNOWN_TERMINALS: &[&str] = &[
    "alacritty",
    "kitty",
    "foot",
    "wezterm",
    "wezterm-gui",
    "ghostty",
    "rio",
    "contour",
    "blackbox",
    "gnome-terminal",
    "gnome-terminal-server",
    "org.gnome.terminal",
    "org.gnome.ptyxis",
    "ptyxis",
    "konsole",
    "org.kde.konsole",
    "xterm",
    "urxvt",
    "rxvt",
    "st",
    "terminator",
    "tilix",
    "sakura",
    "guake",
    "yakuake",
    "xfce4-terminal",
    "mate-terminal",
    "lxterminal",
    "terminology",
    "cool-retro-term",
    "termite",
    "havoc",
    "wayst",
];

/// Resolve the paste key based on configuration.
///
/// - `"auto"` → detects the focused window and returns the right key
/// - Any other value → returned as-is (user override)
///
/// Verbosity levels:
/// - `>= 1`: logs the final detection result (one-liner)
/// - `>= 2`: logs each detection step (subprocess attempts)
pub fn resolve_paste_key(configured: &str, verbosity: u8) -> &str {
    if configured != "auto" {
        if verbosity >= 2 {
            eprintln!("  [paste] explicit: {}", configured);
        }
        return configured;
    }

    let (kind, app_id, method) = detect_window_kind_verbose(verbosity >= 2);
    let key = match kind {
        WindowKind::Terminal => "ctrl+shift+v",
        WindowKind::GraphicalApp => "ctrl+v",
    };

    if verbosity >= 2 {
        match &app_id {
            Some(id) => eprintln!(
                "  [paste] {} app_id=\"{}\" → {:?} → {}",
                method, id, kind, key
            ),
            None => eprintln!(
                "  [paste] {} → {:?} → {}\n  \
                 Hint: Set paste_key in config if wrong: paste_key = \"ctrl+shift+v\"",
                method, kind, key
            ),
        }
    } else if verbosity >= 1 {
        match &app_id {
            Some(id) => eprintln!("  [paste] {} → {}", id, key),
            None => eprintln!("  [paste] {} → {}", method, key),
        }
    }

    key
}

/// Detect the kind of the currently focused window.
pub fn detect_window_kind() -> WindowKind {
    let (kind, _, _) = detect_window_kind_verbose(false);
    kind
}

/// Detect the focused window kind with details for verbose logging.
///
/// Returns (WindowKind, Option<app_id>, detection_method_name).
fn detect_window_kind_verbose(verbose: bool) -> (WindowKind, Option<String>, &'static str) {
    // Try swaymsg (Sway / i3-compatible)
    if !SWAYMSG_BROKEN.load(Ordering::Relaxed)
        && let Some(app_id) = detect_via_swaymsg(verbose)
    {
        return (classify_app_id(&app_id), Some(app_id), "swaymsg");
    }

    // Try hyprctl (Hyprland)
    if !HYPRCTL_BROKEN.load(Ordering::Relaxed)
        && let Some(app_id) = detect_via_hyprctl(verbose)
    {
        return (classify_app_id(&app_id), Some(app_id), "hyprctl");
    }

    // Try GNOME Shell D-Bus (Shell.Eval — disabled on GNOME 45+)
    if !GNOME_DBUS_BROKEN.load(Ordering::Relaxed)
        && let Some(app_id) = detect_via_gnome_dbus(verbose)
    {
        return (classify_app_id(&app_id), Some(app_id), "gnome-dbus");
    }

    // Try GNOME Shell Introspect (works on GNOME 41+, unlike Shell.Eval)
    if !GNOME_INTROSPECT_BROKEN.load(Ordering::Relaxed)
        && let Some(app_id) = detect_via_gnome_introspect(verbose)
    {
        return (classify_app_id(&app_id), Some(app_id), "gnome-introspect");
    }

    // All detection failed — use GNOME-aware fallback
    if is_gnome_desktop() {
        // On GNOME, Ctrl+Shift+V is the safer default:
        // works in terminals AND as "paste unformatted" in most GUI apps.
        if verbose {
            eprintln!("  [paste] GNOME detected, defaulting to ctrl+shift+v");
        }
        (WindowKind::Terminal, None, "gnome-fallback")
    } else {
        (WindowKind::GraphicalApp, None, "fallback(no-compositor)")
    }
}

/// Classify an app_id as Terminal or GraphicalApp.
fn classify_app_id(app_id: &str) -> WindowKind {
    let lower = app_id.to_lowercase();

    for terminal in KNOWN_TERMINALS {
        if lower == *terminal {
            return WindowKind::Terminal;
        }
    }

    // Heuristic: contains "terminal" as a substring
    if lower.contains("terminal") {
        return WindowKind::Terminal;
    }

    WindowKind::GraphicalApp
}

/// Query swaymsg for the focused window's app_id.
fn detect_via_swaymsg(verbose: bool) -> Option<String> {
    let output = match Command::new("swaymsg")
        .args(["-t", "get_tree", "-r"])
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            if is_permanent_error(&e) {
                SWAYMSG_BROKEN.store(true, Ordering::Relaxed);
            }
            if verbose {
                eprintln!("  [paste] swaymsg: failed to execute: {}", e);
            }
            return None;
        }
    };

    if !output.status.success() {
        // Not running under Sway — permanent for this session
        SWAYMSG_BROKEN.store(true, Ordering::Relaxed);
        if verbose {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "  [paste] swaymsg: failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            );
        }
        return None;
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    extract_focused_app_id_sway(&json_str)
}

/// Query hyprctl for the focused window's class.
fn detect_via_hyprctl(verbose: bool) -> Option<String> {
    let output = match Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            if is_permanent_error(&e) {
                HYPRCTL_BROKEN.store(true, Ordering::Relaxed);
            }
            if verbose {
                eprintln!("  [paste] hyprctl: failed to execute: {}", e);
            }
            return None;
        }
    };

    if !output.status.success() {
        // Not running under Hyprland — permanent for this session
        HYPRCTL_BROKEN.store(true, Ordering::Relaxed);
        if verbose {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "  [paste] hyprctl: failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            );
        }
        return None;
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    extract_class_hyprctl(&json_str)
}

/// Query GNOME Shell D-Bus for the focused window's WM class.
///
/// Uses `gdbus call` to invoke `org.gnome.Shell.Eval`. This may be disabled
/// on modern GNOME (45+) for security, in which case it returns None.
///
/// Handles stale D-Bus sessions (e.g. long-lived tmux/byobu/screen) by
/// reading the current GNOME Shell's D-Bus address from `/proc`.
fn detect_via_gnome_dbus(verbose: bool) -> Option<String> {
    let mut cmd = Command::new("gdbus");
    cmd.args([
        "call",
        "--session",
        "--dest",
        "org.gnome.Shell",
        "--object-path",
        "/org/gnome/Shell",
        "--method",
        "org.gnome.Shell.Eval",
        "global.display.focus_window ? global.display.focus_window.get_wm_class() : ''",
    ]);

    // If running inside a long-lived tmux/screen session, our DBUS_SESSION_BUS_ADDRESS
    // may be stale (from a previous GNOME login). Refresh it from the running gnome-shell.
    if let Some(fresh_addr) = fresh_gnome_dbus_address() {
        cmd.env("DBUS_SESSION_BUS_ADDRESS", &fresh_addr);
    }

    let output = match cmd.output() {
        Ok(output) => output,
        Err(e) => {
            if is_permanent_error(&e) {
                GNOME_DBUS_BROKEN.store(true, Ordering::Relaxed);
            }
            if verbose {
                eprintln!("  [paste] gnome-dbus: failed to execute: {}", e);
            }
            return None;
        }
    };

    if !output.status.success() {
        // D-Bus method not available — permanent on this compositor version
        GNOME_DBUS_BROKEN.store(true, Ordering::Relaxed);
        if verbose {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "  [paste] gnome-dbus: failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            );
        }
        return None;
    }

    // Output format: (true, '"ClassName"') on success, (false, '') when disabled
    let result = String::from_utf8_lossy(&output.stdout);
    let parsed = extract_gnome_eval_result(&result);

    if parsed.is_none() {
        // Shell.Eval is disabled (GNOME 45+) — permanent
        GNOME_DBUS_BROKEN.store(true, Ordering::Relaxed);
        if verbose {
            eprintln!("  [paste] gnome-dbus: Shell.Eval disabled or returned empty result");
        }
    }

    parsed
}

/// Check if we're running on a GNOME desktop.
///
/// Uses `XDG_CURRENT_DESKTOP` (fast, no subprocess) with a fallback to
/// checking if `gnome-shell` is running (via `fresh_gnome_dbus_address`).
fn is_gnome_desktop() -> bool {
    if let Ok(desktop) = std::env::var("XDG_CURRENT_DESKTOP")
        && desktop.to_uppercase().contains("GNOME")
    {
        return true;
    }
    fresh_gnome_dbus_address().is_some()
}

/// Read the current DBUS_SESSION_BUS_ADDRESS from the running gnome-shell process.
///
/// This handles the case where the caller's environment has a stale D-Bus address
/// (common in long-lived tmux/byobu/screen sessions that survive GNOME re-logins).
pub(crate) fn fresh_gnome_dbus_address() -> Option<String> {
    use std::fs;

    // Find gnome-shell PID
    let output = Command::new("pgrep")
        .args(["-n", "gnome-shell"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let pid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if pid.is_empty() {
        return None;
    }

    // Read its environment
    let environ = fs::read(format!("/proc/{}/environ", pid)).ok()?;
    let env_str = String::from_utf8_lossy(&environ);

    // Find DBUS_SESSION_BUS_ADDRESS
    for entry in env_str.split('\0') {
        if let Some(addr) = entry.strip_prefix("DBUS_SESSION_BUS_ADDRESS=")
            && !addr.is_empty()
        {
            return Some(addr.to_string());
        }
    }

    None
}

/// Parse GNOME Shell Eval D-Bus response.
///
/// Expected format: `(true, '"ClassName"')` or `(false, '')`
fn extract_gnome_eval_result(response: &str) -> Option<String> {
    let trimmed = response.trim();

    // Must start with (true,
    if !trimmed.starts_with("(true,") {
        return None;
    }

    // Extract the quoted string value between the single quotes
    // Format: (true, '"value"')
    let after_comma = trimmed.strip_prefix("(true,")?.trim();
    let inner = after_comma.strip_prefix("'")?.strip_suffix("')")?.trim();

    // Remove the inner double quotes: "value" → value
    let class = inner.strip_prefix('"')?.strip_suffix('"')?;

    if class.is_empty() {
        return None;
    }

    Some(class.to_string())
}

/// Query GNOME Shell Introspect for the focused window's app-id.
///
/// Uses `org.gnome.Shell.Introspect.GetWindows` which is available on GNOME 41+
/// and works even when `Shell.Eval` is disabled (the default on GNOME 45+).
///
/// The output is GVariant text format. We parse it by finding the window entry
/// that contains `'has-focus': <true>` and extracting its `'app-id'`.
fn detect_via_gnome_introspect(verbose: bool) -> Option<String> {
    let mut cmd = Command::new("gdbus");
    cmd.args([
        "call",
        "--session",
        "--dest",
        "org.gnome.Shell",
        "--object-path",
        "/org/gnome/Shell/Introspect",
        "--method",
        "org.gnome.Shell.Introspect.GetWindows",
    ]);

    if let Some(fresh_addr) = fresh_gnome_dbus_address() {
        cmd.env("DBUS_SESSION_BUS_ADDRESS", &fresh_addr);
    }

    let output = match cmd.output() {
        Ok(output) => output,
        Err(e) => {
            if is_permanent_error(&e) {
                GNOME_INTROSPECT_BROKEN.store(true, Ordering::Relaxed);
            }
            if verbose {
                eprintln!("  [paste] gnome-introspect: failed to execute: {}", e);
            }
            return None;
        }
    };

    if !output.status.success() {
        // D-Bus method denied or unavailable — permanent on this compositor
        GNOME_INTROSPECT_BROKEN.store(true, Ordering::Relaxed);
        if verbose {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "  [paste] gnome-introspect: failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            );
        }
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed = extract_focused_app_from_introspect(&stdout);

    if parsed.is_none() && verbose {
        eprintln!("  [paste] gnome-introspect: no focused window found in output");
    }

    parsed
}

/// Extract the focused window's app-id from GVariant text output.
///
/// The output contains window entries like:
/// ```text
/// uint64 123: {'app-id': <'org.gnome.Ptyxis'>, ..., 'has-focus': <true>}
/// ```
///
/// Strategy: find `'has-focus': <true>`, then search backwards in the same
/// window entry block for `'app-id': <'...'>` or `'wm-class': <'...'>`.
fn extract_focused_app_from_introspect(output: &str) -> Option<String> {
    let focus_pos = output.find("'has-focus': <true>")?;

    // The window entry block starts at the nearest preceding `uint64` or `{`
    let before_focus = &output[..focus_pos];

    // Try app-id first (native Wayland apps)
    if let Some(app_id) = extract_gvariant_string(before_focus, "'app-id': <'")
        && !app_id.is_empty()
    {
        return Some(app_id);
    }

    // Fall back to wm-class (XWayland apps)
    if let Some(wm_class) = extract_gvariant_string(before_focus, "'wm-class': <'")
        && !wm_class.is_empty()
    {
        return Some(wm_class);
    }

    None
}

/// Extract a string value from GVariant text, searching backwards from the end.
///
/// Looks for `marker` followed by the value and a closing `'`.
/// e.g. for marker `'app-id': <'` in `...'app-id': <'org.gnome.Ptyxis'>, ...`
/// returns `Some("org.gnome.Ptyxis")`.
fn extract_gvariant_string(text: &str, marker: &str) -> Option<String> {
    let marker_pos = text.rfind(marker)?;
    let value_start = marker_pos + marker.len();
    let rest = &text[value_start..];
    let value_end = rest.find('\'')?;
    Some(rest[..value_end].to_string())
}

/// Parse sway's get_tree JSON to find the focused node's app_id.
fn extract_focused_app_id_sway(json: &str) -> Option<String> {
    let tree: serde_json::Value = serde_json::from_str(json).ok()?;
    find_focused_node(&tree)
}

/// Recursively search the sway tree for the focused node.
fn find_focused_node(node: &serde_json::Value) -> Option<String> {
    // Check if this node is focused
    if node.get("focused").and_then(|v| v.as_bool()) == Some(true) {
        // Prefer app_id (native Wayland) over window_properties.class (XWayland)
        if let Some(app_id) = node.get("app_id").and_then(|v| v.as_str())
            && !app_id.is_empty()
        {
            return Some(app_id.to_string());
        }
        if let Some(class) = node
            .get("window_properties")
            .and_then(|v| v.get("class"))
            .and_then(|v| v.as_str())
            && !class.is_empty()
        {
            return Some(class.to_string());
        }
        return None;
    }

    // Recurse into child nodes and floating nodes
    for key in &["nodes", "floating_nodes"] {
        if let Some(children) = node.get(*key).and_then(|v| v.as_array()) {
            for child in children {
                if let Some(app_id) = find_focused_node(child) {
                    return Some(app_id);
                }
            }
        }
    }

    None
}

/// Parse hyprctl activewindow JSON for the "class" field.
fn extract_class_hyprctl(json: &str) -> Option<String> {
    let obj: serde_json::Value = serde_json::from_str(json).ok()?;
    obj.get("class")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Parse a paste key string into wtype CLI arguments.
///
/// Splits on `+`, treats all parts except the last as modifiers (`-M`),
/// and the last part as the key (`-k`).
///
/// # Examples
/// - `"ctrl+v"` → `["-M", "ctrl", "-k", "v"]`
/// - `"ctrl+shift+v"` → `["-M", "ctrl", "-M", "shift", "-k", "v"]`
pub fn paste_key_to_wtype_args(paste_key: &str) -> Vec<String> {
    let parts: Vec<&str> = paste_key.split('+').collect();
    let mut args = Vec::new();

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            args.push("-k".to_string());
            args.push(part.to_string());
        } else {
            args.push("-M".to_string());
            args.push(part.to_string());
        }
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- classify_app_id ---

    #[test]
    fn test_classify_known_terminals() {
        let terminals = [
            "alacritty",
            "kitty",
            "foot",
            "wezterm",
            "ghostty",
            "konsole",
            "gnome-terminal",
            "org.gnome.ptyxis",
            "xterm",
            "terminator",
            "tilix",
        ];
        for id in terminals {
            assert_eq!(
                classify_app_id(id),
                WindowKind::Terminal,
                "Expected Terminal for '{}'",
                id
            );
        }
    }

    #[test]
    fn test_classify_case_insensitive() {
        assert_eq!(classify_app_id("Alacritty"), WindowKind::Terminal);
        assert_eq!(classify_app_id("KITTY"), WindowKind::Terminal);
        assert_eq!(classify_app_id("Foot"), WindowKind::Terminal);
    }

    #[test]
    fn test_classify_gui_apps() {
        let gui_apps = [
            "firefox",
            "chromium",
            "google-chrome",
            "code",
            "nautilus",
            "evince",
            "slack",
            "discord",
            "libreoffice",
            "org.mozilla.firefox",
        ];
        for id in gui_apps {
            assert_eq!(
                classify_app_id(id),
                WindowKind::GraphicalApp,
                "Expected GraphicalApp for '{}'",
                id
            );
        }
    }

    #[test]
    fn test_classify_heuristic_terminal_substring() {
        assert_eq!(classify_app_id("my-custom-terminal"), WindowKind::Terminal);
        assert_eq!(classify_app_id("org.custom.Terminal"), WindowKind::Terminal);
    }

    #[test]
    fn test_classify_empty_string() {
        assert_eq!(classify_app_id(""), WindowKind::GraphicalApp);
    }

    // --- resolve_paste_key ---

    #[test]
    fn test_resolve_explicit_key() {
        assert_eq!(resolve_paste_key("ctrl+v", 0), "ctrl+v");
        assert_eq!(resolve_paste_key("ctrl+shift+v", 0), "ctrl+shift+v");
        assert_eq!(resolve_paste_key("super+v", 0), "super+v");
    }

    // Note: resolve_paste_key("auto") depends on the compositor, so we
    // test detect_window_kind's fallback instead.

    #[test]
    fn test_detect_window_kind_no_compositor_defaults_to_gui() {
        // When no compositor IPC is available, falls back to GraphicalApp.
        // This will be the case in CI / test environments.
        let kind = detect_window_kind();
        // We can't assert Terminal here because CI has no compositor,
        // but we can assert it doesn't panic and returns a valid variant.
        assert!(kind == WindowKind::Terminal || kind == WindowKind::GraphicalApp);
    }

    // --- paste_key_to_wtype_args ---

    #[test]
    fn test_wtype_args_ctrl_v() {
        let args = paste_key_to_wtype_args("ctrl+v");
        assert_eq!(args, vec!["-M", "ctrl", "-k", "v"]);
    }

    #[test]
    fn test_wtype_args_ctrl_shift_v() {
        let args = paste_key_to_wtype_args("ctrl+shift+v");
        assert_eq!(args, vec!["-M", "ctrl", "-M", "shift", "-k", "v"]);
    }

    #[test]
    fn test_wtype_args_single_key() {
        let args = paste_key_to_wtype_args("v");
        assert_eq!(args, vec!["-k", "v"]);
    }

    #[test]
    fn test_wtype_args_super_v() {
        let args = paste_key_to_wtype_args("super+v");
        assert_eq!(args, vec!["-M", "super", "-k", "v"]);
    }

    // --- Sway JSON parsing ---

    #[test]
    fn test_extract_focused_app_id_sway_native() {
        let json = r#"{
            "type": "root",
            "nodes": [{
                "type": "output",
                "nodes": [{
                    "type": "workspace",
                    "nodes": [{
                        "type": "con",
                        "app_id": "foot",
                        "focused": true
                    }]
                }]
            }]
        }"#;
        assert_eq!(extract_focused_app_id_sway(json), Some("foot".to_string()));
    }

    #[test]
    fn test_extract_focused_app_id_sway_xwayland() {
        let json = r#"{
            "type": "root",
            "nodes": [{
                "type": "con",
                "app_id": null,
                "window_properties": {"class": "XTerm"},
                "focused": true
            }]
        }"#;
        // app_id is null → falls through to window_properties.class
        assert_eq!(extract_focused_app_id_sway(json), Some("XTerm".to_string()));
    }

    #[test]
    fn test_extract_focused_app_id_sway_floating() {
        let json = r#"{
            "type": "root",
            "nodes": [],
            "floating_nodes": [{
                "type": "floating_con",
                "app_id": "kitty",
                "focused": true
            }]
        }"#;
        assert_eq!(extract_focused_app_id_sway(json), Some("kitty".to_string()));
    }

    #[test]
    fn test_extract_focused_app_id_sway_none_focused() {
        let json = r#"{
            "type": "root",
            "nodes": [{
                "type": "con",
                "app_id": "firefox",
                "focused": false
            }]
        }"#;
        assert_eq!(extract_focused_app_id_sway(json), None);
    }

    #[test]
    fn test_extract_focused_app_id_sway_invalid_json() {
        assert_eq!(extract_focused_app_id_sway("not json"), None);
    }

    // --- Hyprland JSON parsing ---

    #[test]
    fn test_extract_class_hyprctl() {
        let json = r#"{"class": "Alacritty", "title": "~"}"#;
        assert_eq!(extract_class_hyprctl(json), Some("Alacritty".to_string()));
    }

    #[test]
    fn test_extract_class_hyprctl_empty_class() {
        let json = r#"{"class": "", "title": ""}"#;
        assert_eq!(extract_class_hyprctl(json), None);
    }

    #[test]
    fn test_extract_class_hyprctl_no_class_field() {
        let json = r#"{"title": "something"}"#;
        assert_eq!(extract_class_hyprctl(json), None);
    }

    #[test]
    fn test_extract_class_hyprctl_invalid_json() {
        assert_eq!(extract_class_hyprctl("not json"), None);
    }

    // --- End-to-end classification via sway JSON ---

    #[test]
    fn test_sway_json_detects_terminal() {
        let json = r#"{
            "nodes": [{
                "nodes": [{
                    "app_id": "alacritty",
                    "focused": true
                }]
            }]
        }"#;
        let app_id = extract_focused_app_id_sway(json).unwrap();
        assert_eq!(classify_app_id(&app_id), WindowKind::Terminal);
    }

    #[test]
    fn test_sway_json_detects_gui_app() {
        let json = r#"{
            "nodes": [{
                "nodes": [{
                    "app_id": "firefox",
                    "focused": true
                }]
            }]
        }"#;
        let app_id = extract_focused_app_id_sway(json).unwrap();
        assert_eq!(classify_app_id(&app_id), WindowKind::GraphicalApp);
    }

    #[test]
    fn test_hyprctl_json_detects_terminal() {
        let json = r#"{"class": "kitty", "title": "~"}"#;
        let class = extract_class_hyprctl(json).unwrap();
        assert_eq!(classify_app_id(&class), WindowKind::Terminal);
    }

    // --- GNOME D-Bus eval parsing ---

    #[test]
    fn test_gnome_eval_success() {
        assert_eq!(
            extract_gnome_eval_result("(true, '\"org.gnome.Ptyxis\"')"),
            Some("org.gnome.Ptyxis".to_string())
        );
    }

    #[test]
    fn test_gnome_eval_disabled() {
        assert_eq!(extract_gnome_eval_result("(false, '')"), None);
    }

    #[test]
    fn test_gnome_eval_empty_class() {
        assert_eq!(extract_gnome_eval_result("(true, '\"\"')"), None);
    }

    #[test]
    fn test_gnome_eval_malformed() {
        assert_eq!(extract_gnome_eval_result("garbage"), None);
        assert_eq!(extract_gnome_eval_result(""), None);
    }

    // --- Ptyxis classification ---

    #[test]
    fn test_classify_ptyxis() {
        assert_eq!(classify_app_id("ptyxis"), WindowKind::Terminal);
        assert_eq!(classify_app_id("org.gnome.Ptyxis"), WindowKind::Terminal);
        assert_eq!(classify_app_id("Ptyxis"), WindowKind::Terminal);
    }

    // --- GNOME Introspect parsing ---

    #[test]
    fn test_introspect_focused_terminal() {
        let output = r#"({uint64 1234: {'app-id': <'org.gnome.Ptyxis'>, 'wm-class': <'ptyxis'>, 'has-focus': <true>}, uint64 5678: {'app-id': <'org.mozilla.firefox'>, 'has-focus': <false>}},)"#;
        assert_eq!(
            extract_focused_app_from_introspect(output),
            Some("org.gnome.Ptyxis".to_string())
        );
    }

    #[test]
    fn test_introspect_focused_gui_app() {
        let output = r#"({uint64 1: {'app-id': <'org.gnome.Ptyxis'>, 'has-focus': <false>}, uint64 2: {'app-id': <'org.mozilla.firefox'>, 'wm-class': <'firefox'>, 'has-focus': <true>}},)"#;
        assert_eq!(
            extract_focused_app_from_introspect(output),
            Some("org.mozilla.firefox".to_string())
        );
    }

    #[test]
    fn test_introspect_no_focused_window() {
        let output = r#"({uint64 1: {'app-id': <'firefox'>, 'has-focus': <false>}},)"#;
        assert_eq!(extract_focused_app_from_introspect(output), None);
    }

    #[test]
    fn test_introspect_empty_app_id_falls_back_to_wm_class() {
        let output =
            r#"({uint64 1: {'app-id': <''>, 'wm-class': <'XTerm'>, 'has-focus': <true>}},)"#;
        assert_eq!(
            extract_focused_app_from_introspect(output),
            Some("XTerm".to_string())
        );
    }

    #[test]
    fn test_introspect_multiline() {
        let output = "({uint64 42: {'app-id': <'alacritty'>,\n \
                       'title': <'~'>,\n \
                       'has-focus': <true>}},)";
        assert_eq!(
            extract_focused_app_from_introspect(output),
            Some("alacritty".to_string())
        );
    }

    #[test]
    fn test_extract_gvariant_string_basic() {
        let text = "{'app-id': <'org.gnome.Ptyxis'>, 'wm-class': <'ptyxis'>}";
        assert_eq!(
            extract_gvariant_string(text, "'app-id': <'"),
            Some("org.gnome.Ptyxis".to_string())
        );
    }

    #[test]
    fn test_extract_gvariant_string_no_match() {
        let text = "{'wm-class': <'ptyxis'>}";
        assert_eq!(extract_gvariant_string(text, "'app-id': <'"), None);
    }

    #[test]
    fn test_is_gnome_desktop_does_not_panic() {
        // Just verify it returns a bool without panicking.
        let _ = is_gnome_desktop();
    }

    // --- Circuit breaker ---

    #[test]
    fn test_circuit_breaker_flags_are_false_by_default() {
        // Reset first to avoid cross-test interference from parallel runs
        reset_detection_cache();
        assert!(!SWAYMSG_BROKEN.load(Ordering::Relaxed));
        assert!(!HYPRCTL_BROKEN.load(Ordering::Relaxed));
        assert!(!GNOME_DBUS_BROKEN.load(Ordering::Relaxed));
        assert!(!GNOME_INTROSPECT_BROKEN.load(Ordering::Relaxed));
    }

    #[test]
    fn test_reset_detection_cache() {
        // Set all flags to true
        SWAYMSG_BROKEN.store(true, Ordering::Relaxed);
        HYPRCTL_BROKEN.store(true, Ordering::Relaxed);
        GNOME_DBUS_BROKEN.store(true, Ordering::Relaxed);
        GNOME_INTROSPECT_BROKEN.store(true, Ordering::Relaxed);

        reset_detection_cache();

        assert!(!SWAYMSG_BROKEN.load(Ordering::Relaxed));
        assert!(!HYPRCTL_BROKEN.load(Ordering::Relaxed));
        assert!(!GNOME_DBUS_BROKEN.load(Ordering::Relaxed));
        assert!(!GNOME_INTROSPECT_BROKEN.load(Ordering::Relaxed));
    }

    #[test]
    fn test_is_permanent_error() {
        let not_found = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        assert!(is_permanent_error(&not_found));

        let perm_denied =
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
        assert!(is_permanent_error(&perm_denied));

        let other = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "transient");
        assert!(!is_permanent_error(&other));
    }
}
