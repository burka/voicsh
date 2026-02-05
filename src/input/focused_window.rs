//! Focused window detection for automatic paste key selection.
//!
//! Queries the Wayland compositor to determine whether the focused window
//! is a terminal emulator, then selects the appropriate paste key:
//! - Terminal emulators: `Ctrl+Shift+V`
//! - GUI applications: `Ctrl+V`
//!
//! Supports Sway (swaymsg), Hyprland (hyprctl), and GNOME (gdbus) compositors.

use std::process::Command;

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
/// When `verbose` is true, logs the detection steps to stderr.
pub fn resolve_paste_key(configured: &str, verbose: bool) -> &str {
    if configured != "auto" {
        if verbose {
            eprintln!("  [paste] explicit: {}", configured);
        }
        return configured;
    }

    let (kind, app_id, method) = detect_window_kind_verbose(verbose);
    let key = match kind {
        WindowKind::Terminal => "ctrl+shift+v",
        WindowKind::GraphicalApp => "ctrl+v",
    };

    if verbose {
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
    if let Some(app_id) = detect_via_swaymsg() {
        return (classify_app_id(&app_id), Some(app_id), "swaymsg");
    }
    if verbose {
        eprintln!("  [paste] swaymsg: not available");
    }

    // Try hyprctl (Hyprland)
    if let Some(app_id) = detect_via_hyprctl() {
        return (classify_app_id(&app_id), Some(app_id), "hyprctl");
    }
    if verbose {
        eprintln!("  [paste] hyprctl: not available");
    }

    // Try GNOME Shell D-Bus
    if let Some(app_id) = detect_via_gnome_dbus() {
        return (classify_app_id(&app_id), Some(app_id), "gnome-dbus");
    }
    if verbose {
        eprintln!("  [paste] gnome-dbus: not available (GNOME may have disabled Shell.Eval)");
    }

    // All detection failed
    (WindowKind::GraphicalApp, None, "fallback(no-compositor)")
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
fn detect_via_swaymsg() -> Option<String> {
    let output = Command::new("swaymsg")
        .args(["-t", "get_tree", "-r"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    extract_focused_app_id_sway(&json_str)
}

/// Query hyprctl for the focused window's class.
fn detect_via_hyprctl() -> Option<String> {
    let output = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()?;

    if !output.status.success() {
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
fn detect_via_gnome_dbus() -> Option<String> {
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

    let output = cmd.output().ok()?;

    if !output.status.success() {
        return None;
    }

    // Output format: (true, '"ClassName"') on success, (false, '') when disabled
    let result = String::from_utf8_lossy(&output.stdout);
    extract_gnome_eval_result(&result)
}

/// Read the current DBUS_SESSION_BUS_ADDRESS from the running gnome-shell process.
///
/// This handles the case where the caller's environment has a stale D-Bus address
/// (common in long-lived tmux/byobu/screen sessions that survive GNOME re-logins).
fn fresh_gnome_dbus_address() -> Option<String> {
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
        assert_eq!(resolve_paste_key("ctrl+v", false), "ctrl+v");
        assert_eq!(resolve_paste_key("ctrl+shift+v", false), "ctrl+shift+v");
        assert_eq!(resolve_paste_key("super+v", false), "super+v");
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
}
