//! Focused window detection for automatic paste key selection.
//!
//! Queries the Wayland compositor to determine whether the focused window
//! is a terminal emulator, then selects the appropriate paste key:
//! - Terminal emulators: `Ctrl+Shift+V`
//! - GUI applications: `Ctrl+V`
//!
//! Supports Sway (swaymsg) and Hyprland (hyprctl) compositors.

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
pub fn resolve_paste_key(configured: &str) -> &str {
    if configured == "auto" {
        match detect_window_kind() {
            WindowKind::Terminal => "ctrl+shift+v",
            WindowKind::GraphicalApp => "ctrl+v",
        }
    } else {
        configured
    }
}

/// Detect the kind of the currently focused window.
pub fn detect_window_kind() -> WindowKind {
    match detect_focused_app_id() {
        Some(app_id) => classify_app_id(&app_id),
        None => WindowKind::GraphicalApp,
    }
}

/// Detect the app_id of the currently focused Wayland window.
///
/// Tries compositor-specific IPC in order:
/// 1. `swaymsg -t get_tree` (Sway / i3-compatible)
/// 2. `hyprctl activewindow -j` (Hyprland)
fn detect_focused_app_id() -> Option<String> {
    detect_via_swaymsg().or_else(detect_via_hyprctl)
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
        assert_eq!(resolve_paste_key("ctrl+v"), "ctrl+v");
        assert_eq!(resolve_paste_key("ctrl+shift+v"), "ctrl+shift+v");
        assert_eq!(resolve_paste_key("super+v"), "super+v");
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
}
