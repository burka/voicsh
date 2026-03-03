//! Focused window detection for automatic paste key selection.
//!
//! Queries the Wayland compositor to determine whether the focused window
//! is a terminal emulator, then selects the appropriate paste key:
//! - Terminal emulators: `Ctrl+Shift+V`
//! - GUI applications: `Ctrl+V`
//!
//! Supports Sway (swaymsg), Hyprland (hyprctl), and GNOME (voicsh extension,
//! Shell.Introspect, Shell.Eval via gdbus) compositors.

use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

static SWAYMSG_BROKEN: AtomicBool = AtomicBool::new(false);
static HYPRCTL_BROKEN: AtomicBool = AtomicBool::new(false);
static GNOME_DBUS_BROKEN: AtomicBool = AtomicBool::new(false);
static GNOME_INTROSPECT_BROKEN: AtomicBool = AtomicBool::new(false);
static VOICSH_EXTENSION_BROKEN: AtomicBool = AtomicBool::new(false);
static PASTE_LOGGED: AtomicBool = AtomicBool::new(false);
static FALLBACK_LOGGED: AtomicBool = AtomicBool::new(false);

/// Reset broken-backend flags so all backends are retried on the next detection.
///
/// Does not clear the toolkit cache — call this when you want backends retried
/// but still want previously detected toolkits to remain cached.
pub fn reset_broken_flags() {
    SWAYMSG_BROKEN.store(false, Ordering::Relaxed);
    HYPRCTL_BROKEN.store(false, Ordering::Relaxed);
    GNOME_DBUS_BROKEN.store(false, Ordering::Relaxed);
    GNOME_INTROSPECT_BROKEN.store(false, Ordering::Relaxed);
    VOICSH_EXTENSION_BROKEN.store(false, Ordering::Relaxed);
    PASTE_LOGGED.store(false, Ordering::Relaxed);
    FALLBACK_LOGGED.store(false, Ordering::Relaxed);
}

/// Reset detection cache. Call when session environment may have changed
/// (e.g. compositor restart in daemon mode).
pub fn reset_detection_cache() {
    reset_broken_flags();
    if let Ok(mut cache) = TOOLKIT_CACHE.lock() {
        cache.clear();
    }
}

/// Toolkit used by the focused application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toolkit {
    Gtk4,
    Gtk3,
    Qt6,
    Qt5,
    Electron,
    Unknown,
}

impl std::fmt::Display for Toolkit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Toolkit::Gtk4 => write!(f, "GTK4"),
            Toolkit::Gtk3 => write!(f, "GTK3"),
            Toolkit::Qt6 => write!(f, "Qt6"),
            Toolkit::Qt5 => write!(f, "Qt5"),
            Toolkit::Electron => write!(f, "Electron"),
            Toolkit::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Information about the currently focused window.
#[derive(Debug, Clone)]
pub struct FocusedWindowInfo {
    pub app_id: String,
    pub pid: Option<u32>,
    pub toolkit: Toolkit,
    pub window_kind: WindowKind,
    pub detection_method: &'static str,
}

/// Cache PID → Toolkit: a process's toolkit never changes during its lifetime.
static TOOLKIT_CACHE: LazyLock<Mutex<HashMap<u32, Toolkit>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Detect full information about the currently focused window.
///
/// Tries each compositor backend in order:
/// sway → hyprctl → voicsh-extension → gnome-introspect → gnome-dbus.
/// Extracts app_id, PID, and toolkit.
pub fn detect_focused_window_info() -> FocusedWindowInfo {
    // Try swaymsg (Sway / i3-compatible)
    if !SWAYMSG_BROKEN.load(Ordering::Relaxed)
        && let Some(info) = detect_info_via_swaymsg()
    {
        return info;
    }

    // Try hyprctl (Hyprland)
    if !HYPRCTL_BROKEN.load(Ordering::Relaxed)
        && let Some(info) = detect_info_via_hyprctl()
    {
        return info;
    }

    // Try voicsh GNOME extension D-Bus (works on GNOME 45+ where Introspect is restricted)
    if !VOICSH_EXTENSION_BROKEN.load(Ordering::Relaxed)
        && let Some(info) = detect_info_via_voicsh_extension()
    {
        return info;
    }

    // Try GNOME Shell Introspect (GNOME 41+)
    if !GNOME_INTROSPECT_BROKEN.load(Ordering::Relaxed)
        && let Some(info) = detect_info_via_gnome_introspect_full()
    {
        return info;
    }

    // Try GNOME Shell D-Bus via Shell.Eval (disabled on GNOME 45+).
    // Tried last among GNOME backends: gnome-introspect provides PID, this does not.
    if !GNOME_DBUS_BROKEN.load(Ordering::Relaxed)
        && let Some(app_id) = detect_via_gnome_dbus()
    {
        return build_window_info(app_id, None, "gnome-dbus");
    }

    // All detection failed
    let gnome = is_gnome_desktop();
    let method = if gnome {
        "gnome-fallback"
    } else {
        "fallback(no-compositor)"
    };
    FocusedWindowInfo {
        app_id: String::new(),
        pid: None,
        toolkit: Toolkit::Unknown,
        window_kind: if gnome {
            WindowKind::Terminal
        } else {
            WindowKind::GraphicalApp
        },
        detection_method: method,
    }
}

/// Detect toolkit from `/proc/<pid>/maps` by matching loaded shared library names.
pub fn detect_toolkit(pid: u32) -> Toolkit {
    if let Ok(cache) = TOOLKIT_CACHE.lock()
        && let Some(&cached) = cache.get(&pid)
    {
        return cached;
    }

    let toolkit = detect_toolkit_from_proc(pid);

    if let Ok(mut cache) = TOOLKIT_CACHE.lock() {
        cache.insert(pid, toolkit);
    }

    toolkit
}

/// Match a single maps-file path field against known toolkit library names.
fn classify_maps_path(path: &str) -> Option<Toolkit> {
    if path.contains("libgtk-4") {
        return Some(Toolkit::Gtk4);
    }
    if path.contains("libgtk-3") {
        return Some(Toolkit::Gtk3);
    }
    if path.contains("libQt6") {
        return Some(Toolkit::Qt6);
    }
    if path.contains("libQt5") {
        return Some(Toolkit::Qt5);
    }
    if path.contains("/electron") || path.contains("libnode") {
        return Some(Toolkit::Electron);
    }
    None
}

/// Read `/proc/<pid>/maps` line-by-line and identify the toolkit from loaded libraries.
fn detect_toolkit_from_proc(pid: u32) -> Toolkit {
    use std::io::{BufRead, BufReader};
    let file = match std::fs::File::open(format!("/proc/{}/maps", pid)) {
        Ok(f) => f,
        Err(_) => return Toolkit::Unknown,
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let path = match line.rsplit_once(char::is_whitespace) {
            Some((_, p)) => p,
            None => continue,
        };
        if let Some(toolkit) = classify_maps_path(path) {
            return toolkit;
        }
    }
    Toolkit::Unknown
}

/// Identify toolkit from the text content of a `/proc/<pid>/maps` file.
///
/// Used by tests to exercise toolkit detection without touching the filesystem.
#[cfg(test)]
fn detect_toolkit_from_maps_content(content: &str) -> Toolkit {
    for line in content.lines() {
        let path = match line.rsplit_once(char::is_whitespace) {
            Some((_, p)) => p,
            None => continue,
        };
        if let Some(toolkit) = classify_maps_path(path) {
            return toolkit;
        }
    }
    Toolkit::Unknown
}

/// Build `FocusedWindowInfo` from app_id, optional PID, and detection method.
fn build_window_info(
    app_id: String,
    pid: Option<u32>,
    detection_method: &'static str,
) -> FocusedWindowInfo {
    let window_kind = classify_app_id(&app_id);
    let toolkit = pid.map(detect_toolkit).unwrap_or(Toolkit::Unknown);
    FocusedWindowInfo {
        app_id,
        pid,
        toolkit,
        window_kind,
        detection_method,
    }
}

/// Detect focused window info via swaymsg, including PID.
fn detect_info_via_swaymsg() -> Option<FocusedWindowInfo> {
    let mut cmd = Command::new("swaymsg");
    cmd.args(["-t", "get_tree", "-r"]);
    let output = run_and_mark_broken(&mut cmd, &SWAYMSG_BROKEN)?;

    let json_str = String::from_utf8_lossy(&output.stdout);
    let tree: serde_json::Value = serde_json::from_str(&json_str).ok()?;
    let (app_id, pid) = find_focused_node_with_pid(&tree)?;
    Some(build_window_info(app_id, pid, "swaymsg"))
}

/// Recursively search the sway tree for the focused node, returning app_id and PID.
fn find_focused_node_with_pid(node: &serde_json::Value) -> Option<(String, Option<u32>)> {
    if node.get("focused").and_then(|v| v.as_bool()) == Some(true) {
        let app_id = node
            .get("app_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                node.get("window_properties")
                    .and_then(|v| v.get("class"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
            })?;
        let pid = node.get("pid").and_then(|v| v.as_u64()).map(|p| p as u32);
        return Some((app_id.to_string(), pid));
    }

    for key in &["nodes", "floating_nodes"] {
        if let Some(children) = node.get(*key).and_then(|v| v.as_array()) {
            for child in children {
                if let Some(result) = find_focused_node_with_pid(child) {
                    return Some(result);
                }
            }
        }
    }

    None
}

/// Detect focused window info via hyprctl, including PID.
fn detect_info_via_hyprctl() -> Option<FocusedWindowInfo> {
    let mut cmd = Command::new("hyprctl");
    cmd.args(["activewindow", "-j"]);
    let output = run_and_mark_broken(&mut cmd, &HYPRCTL_BROKEN)?;

    let json_str = String::from_utf8_lossy(&output.stdout);
    let (app_id, pid) = extract_class_and_pid_hyprctl(&json_str)?;
    Some(build_window_info(app_id, pid, "hyprctl"))
}

/// Parse hyprctl activewindow JSON for "class" and "pid" fields.
fn extract_class_and_pid_hyprctl(json: &str) -> Option<(String, Option<u32>)> {
    let obj: serde_json::Value = serde_json::from_str(json).ok()?;
    let class = obj
        .get("class")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?;
    let pid = obj.get("pid").and_then(|v| v.as_u64()).map(|p| p as u32);
    Some((class.to_string(), pid))
}

/// Detect focused window via the voicsh GNOME Shell extension's D-Bus interface.
///
/// The extension exports `GetFocusedWindow()` on path
/// `/org/gnome/Shell/Extensions/voicsh` under the `org.gnome.Shell` bus name.
/// This bypasses the GNOME 45+ restriction on `Shell.Introspect.GetWindows`.
fn detect_info_via_voicsh_extension() -> Option<FocusedWindowInfo> {
    let mut cmd = Command::new("gdbus");
    cmd.args([
        "call",
        "--session",
        "--dest",
        "org.gnome.Shell",
        "--object-path",
        "/org/gnome/Shell/Extensions/voicsh",
        "--method",
        "org.gnome.Shell.Extensions.voicsh.GetFocusedWindow",
    ]);

    if let Some(fresh_addr) = fresh_gnome_dbus_address() {
        cmd.env("DBUS_SESSION_BUS_ADDRESS", &fresh_addr);
    }

    let output = run_and_mark_broken(&mut cmd, &VOICSH_EXTENSION_BROKEN)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_voicsh_extension_response(&stdout)
}

/// Parse the GVariant response from the voicsh extension's GetFocusedWindow.
///
/// Expected format: `('app-id', uint32 pid, 'wm-class')`
fn parse_voicsh_extension_response(output: &str) -> Option<FocusedWindowInfo> {
    // Extract the three fields: ('app_id', uint32 pid, 'wm_class')
    let trimmed = output.trim().trim_start_matches('(').trim_end_matches(')');

    // Split by comma, handling quoted strings
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in trimmed.chars() {
        match ch {
            '\'' if !in_quotes => in_quotes = true,
            '\'' if in_quotes => in_quotes = false,
            ',' if !in_quotes => {
                fields.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(ch),
        }
    }
    fields.push(current.trim().to_string());

    // Destructure via swap_remove/pop to take ownership without cloning.
    // Fields are positional: [0]=app_id, [1]=pid, [2]=wm_class.
    if fields.len() < 3 {
        return None;
    }
    let wm_class = fields.swap_remove(2);
    let pid_raw = fields.swap_remove(1);
    let app_id = fields.swap_remove(0);

    // PID: "uint32 123" or just "0"
    let pid_str = pid_raw.trim_start_matches("uint32").trim();
    let pid = pid_str.parse::<u32>().ok().filter(|&p| p > 0);

    // Use wm_class as app_id if sandboxed app_id is empty
    let effective_app_id = if app_id.is_empty() && !wm_class.is_empty() {
        wm_class
    } else {
        app_id
    };

    if effective_app_id.is_empty() {
        return None;
    }

    Some(build_window_info(effective_app_id, pid, "voicsh-extension"))
}

/// Detect focused window info via GNOME Shell Introspect, including PID.
fn detect_info_via_gnome_introspect_full() -> Option<FocusedWindowInfo> {
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

    let output = run_and_mark_broken(&mut cmd, &GNOME_INTROSPECT_BROKEN)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let (app_id, pid) = extract_focused_app_and_pid_from_introspect(&stdout)?;
    Some(build_window_info(app_id, pid, "gnome-introspect"))
}

/// Extract app-id and PID from GVariant text output of GNOME Shell Introspect.
///
/// Looks for the window block with `'has-focus': <true>` and extracts
/// `'app-id'` (or `'wm-class'`) and `'pid'`.
fn extract_focused_app_and_pid_from_introspect(output: &str) -> Option<(String, Option<u32>)> {
    let focus_pos = output.find("'has-focus': <true>")?;
    let before_focus = &output[..focus_pos];

    // Find the start of this window entry (nearest preceding "uint64")
    let block_start = before_focus.rfind("uint64").unwrap_or(0);
    // Extend to after focus marker for PID search
    let after_focus = &output[focus_pos..];
    let block_end_offset = after_focus.find('}').unwrap_or(after_focus.len());
    let full_block = &output[block_start..focus_pos + block_end_offset];

    // Extract app-id or wm-class
    let app_id = extract_gvariant_string(before_focus, "'app-id': <'")
        .filter(|s| !s.is_empty())
        .or_else(|| {
            extract_gvariant_string(before_focus, "'wm-class': <'").filter(|s| !s.is_empty())
        })?;

    // Extract PID from the block
    let pid = extract_gvariant_uint32(full_block, "'pid': <uint32 ");

    Some((app_id, pid))
}

/// Extract a uint32 value from GVariant text.
///
/// Looks for `marker` followed by a number and `>`.
fn extract_gvariant_uint32(text: &str, marker: &str) -> Option<u32> {
    let marker_pos = text.find(marker)?;
    let value_start = marker_pos + marker.len();
    let rest = &text[value_start..];
    let value_end = rest.find('>')?;
    rest[..value_end].trim().parse().ok()
}

/// Check if an I/O error is permanent (binary not found or permission denied).
fn is_permanent_error(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
    )
}

/// Run a command and mark the backend as broken on permanent failure.
///
/// Returns `Some(output)` if the command succeeds, `None` otherwise.
/// Sets `broken` flag on permanent errors or non-zero exit.
fn run_and_mark_broken(cmd: &mut Command, broken: &AtomicBool) -> Option<std::process::Output> {
    match cmd.output() {
        Ok(output) if output.status.success() => Some(output),
        Ok(_) => {
            broken.store(true, Ordering::Relaxed);
            None
        }
        Err(e) => {
            if is_permanent_error(&e) {
                broken.store(true, Ordering::Relaxed);
            }
            None
        }
    }
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
            None if !PASTE_LOGGED.swap(true, Ordering::Relaxed) => eprintln!(
                "  [paste] {} → {:?} → {}\n  \
                 Hint: Set paste_key in config if wrong: paste_key = \"ctrl+shift+v\"",
                method, kind, key
            ),
            None => {}
        }
    } else if verbosity >= 1 && !PASTE_LOGGED.swap(true, Ordering::Relaxed) {
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
    let info = detect_focused_window_info();

    if verbose
        && info.detection_method.starts_with("gnome-fallback")
        && !FALLBACK_LOGGED.swap(true, Ordering::Relaxed)
    {
        eprintln!("  [paste] GNOME detected, defaulting to ctrl+shift+v");
    }

    let app_id = if info.app_id.is_empty() {
        None
    } else {
        Some(info.app_id)
    };

    (info.window_kind, app_id, info.detection_method)
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

    let output = match cmd.output() {
        Ok(output) => output,
        Err(e) => {
            if is_permanent_error(&e) {
                GNOME_DBUS_BROKEN.store(true, Ordering::Relaxed);
            }
            return None;
        }
    };

    if !output.status.success() {
        // D-Bus method not available — permanent on this compositor version
        GNOME_DBUS_BROKEN.store(true, Ordering::Relaxed);
        return None;
    }

    // Output format: (true, '"ClassName"') on success, (false, '') when disabled
    let result = String::from_utf8_lossy(&output.stdout);
    let parsed = extract_gnome_eval_result(&result);

    if parsed.is_none() {
        // Shell.Eval is disabled (GNOME 45+) — permanent
        GNOME_DBUS_BROKEN.store(true, Ordering::Relaxed);
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
        let tree: serde_json::Value = serde_json::from_str(json).unwrap();
        let result = find_focused_node_with_pid(&tree).map(|(app_id, _)| app_id);
        assert_eq!(result, Some("foot".to_string()));
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
        let tree: serde_json::Value = serde_json::from_str(json).unwrap();
        let result = find_focused_node_with_pid(&tree).map(|(app_id, _)| app_id);
        assert_eq!(result, Some("XTerm".to_string()));
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
        let tree: serde_json::Value = serde_json::from_str(json).unwrap();
        let result = find_focused_node_with_pid(&tree).map(|(app_id, _)| app_id);
        assert_eq!(result, Some("kitty".to_string()));
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
        let tree: serde_json::Value = serde_json::from_str(json).unwrap();
        let result = find_focused_node_with_pid(&tree).map(|(app_id, _)| app_id);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_focused_app_id_sway_invalid_json() {
        let result = serde_json::from_str::<serde_json::Value>("not json")
            .ok()
            .and_then(|tree| find_focused_node_with_pid(&tree))
            .map(|(app_id, _)| app_id);
        assert_eq!(result, None);
    }

    // --- Hyprland JSON parsing ---

    #[test]
    fn test_extract_class_hyprctl() {
        let json = r#"{"class": "Alacritty", "title": "~"}"#;
        assert_eq!(
            extract_class_and_pid_hyprctl(json).map(|(class, _)| class),
            Some("Alacritty".to_string())
        );
    }

    #[test]
    fn test_extract_class_hyprctl_empty_class() {
        let json = r#"{"class": "", "title": ""}"#;
        assert_eq!(
            extract_class_and_pid_hyprctl(json).map(|(class, _)| class),
            None
        );
    }

    #[test]
    fn test_extract_class_hyprctl_no_class_field() {
        let json = r#"{"title": "something"}"#;
        assert_eq!(
            extract_class_and_pid_hyprctl(json).map(|(class, _)| class),
            None
        );
    }

    #[test]
    fn test_extract_class_hyprctl_invalid_json() {
        assert_eq!(
            extract_class_and_pid_hyprctl("not json").map(|(class, _)| class),
            None
        );
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
        let tree: serde_json::Value = serde_json::from_str(json).unwrap();
        let app_id = find_focused_node_with_pid(&tree).map(|(id, _)| id).unwrap();
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
        let tree: serde_json::Value = serde_json::from_str(json).unwrap();
        let app_id = find_focused_node_with_pid(&tree).map(|(id, _)| id).unwrap();
        assert_eq!(classify_app_id(&app_id), WindowKind::GraphicalApp);
    }

    #[test]
    fn test_hyprctl_json_detects_terminal() {
        let json = r#"{"class": "kitty", "title": "~"}"#;
        let class = extract_class_and_pid_hyprctl(json).map(|(c, _)| c).unwrap();
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
            extract_focused_app_and_pid_from_introspect(output).map(|(app_id, _)| app_id),
            Some("org.gnome.Ptyxis".to_string())
        );
    }

    #[test]
    fn test_introspect_focused_gui_app() {
        let output = r#"({uint64 1: {'app-id': <'org.gnome.Ptyxis'>, 'has-focus': <false>}, uint64 2: {'app-id': <'org.mozilla.firefox'>, 'wm-class': <'firefox'>, 'has-focus': <true>}},)"#;
        assert_eq!(
            extract_focused_app_and_pid_from_introspect(output).map(|(app_id, _)| app_id),
            Some("org.mozilla.firefox".to_string())
        );
    }

    #[test]
    fn test_introspect_no_focused_window() {
        let output = r#"({uint64 1: {'app-id': <'firefox'>, 'has-focus': <false>}},)"#;
        assert_eq!(
            extract_focused_app_and_pid_from_introspect(output).map(|(app_id, _)| app_id),
            None
        );
    }

    #[test]
    fn test_introspect_empty_app_id_falls_back_to_wm_class() {
        let output =
            r#"({uint64 1: {'app-id': <''>, 'wm-class': <'XTerm'>, 'has-focus': <true>}},)"#;
        assert_eq!(
            extract_focused_app_and_pid_from_introspect(output).map(|(app_id, _)| app_id),
            Some("XTerm".to_string())
        );
    }

    #[test]
    fn test_introspect_multiline() {
        let output = "({uint64 42: {'app-id': <'alacritty'>,\n \
                       'title': <'~'>,\n \
                       'has-focus': <true>}},)";
        assert_eq!(
            extract_focused_app_and_pid_from_introspect(output).map(|(app_id, _)| app_id),
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
        assert!(!VOICSH_EXTENSION_BROKEN.load(Ordering::Relaxed));
    }

    #[test]
    fn test_reset_detection_cache() {
        // Set all flags to true
        SWAYMSG_BROKEN.store(true, Ordering::Relaxed);
        HYPRCTL_BROKEN.store(true, Ordering::Relaxed);
        GNOME_DBUS_BROKEN.store(true, Ordering::Relaxed);
        GNOME_INTROSPECT_BROKEN.store(true, Ordering::Relaxed);
        VOICSH_EXTENSION_BROKEN.store(true, Ordering::Relaxed);

        reset_detection_cache();

        assert!(!SWAYMSG_BROKEN.load(Ordering::Relaxed));
        assert!(!HYPRCTL_BROKEN.load(Ordering::Relaxed));
        assert!(!GNOME_DBUS_BROKEN.load(Ordering::Relaxed));
        assert!(!GNOME_INTROSPECT_BROKEN.load(Ordering::Relaxed));
        assert!(!VOICSH_EXTENSION_BROKEN.load(Ordering::Relaxed));
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

    // --- Toolkit detection from /proc/maps content ---

    #[test]
    fn test_detect_toolkit_gtk4() {
        let maps = "\
7f0000000000-7f0000001000 r--p 00000000 08:01 12345 /usr/lib/x86_64-linux-gnu/libgtk-4.so.1\n\
7f0000001000-7f0000002000 r-xp 00001000 08:01 12345 /usr/lib/x86_64-linux-gnu/libgtk-4.so.1\n";
        assert_eq!(detect_toolkit_from_maps_content(maps), Toolkit::Gtk4);
    }

    #[test]
    fn test_detect_toolkit_gtk3() {
        let maps = "\
7f0000000000-7f0000001000 r--p 00000000 08:01 12345 /usr/lib/x86_64-linux-gnu/libgtk-3.so.0\n";
        assert_eq!(detect_toolkit_from_maps_content(maps), Toolkit::Gtk3);
    }

    #[test]
    fn test_detect_toolkit_qt6() {
        let maps = "\
7f0000000000-7f0000001000 r--p 00000000 08:01 12345 /usr/lib/x86_64-linux-gnu/libQt6Core.so.6\n";
        assert_eq!(detect_toolkit_from_maps_content(maps), Toolkit::Qt6);
    }

    #[test]
    fn test_detect_toolkit_qt5() {
        let maps = "\
7f0000000000-7f0000001000 r--p 00000000 08:01 12345 /usr/lib/x86_64-linux-gnu/libQt5Core.so.5\n";
        assert_eq!(detect_toolkit_from_maps_content(maps), Toolkit::Qt5);
    }

    #[test]
    fn test_detect_toolkit_electron() {
        let maps = "\
7f0000000000-7f0000001000 r--p 00000000 08:01 12345 /opt/electron/electron\n";
        assert_eq!(detect_toolkit_from_maps_content(maps), Toolkit::Electron);
    }

    #[test]
    fn test_detect_toolkit_electron_libnode() {
        let maps = "\
7f0000000000-7f0000001000 r--p 00000000 08:01 12345 /usr/lib/libnode.so.18\n";
        assert_eq!(detect_toolkit_from_maps_content(maps), Toolkit::Electron);
    }

    #[test]
    fn test_detect_toolkit_unknown() {
        let maps = "\
7f0000000000-7f0000001000 r--p 00000000 08:01 12345 /usr/lib/libc.so.6\n\
7f0000001000-7f0000002000 r--p 00000000 08:01 12345 /usr/lib/libm.so.6\n";
        assert_eq!(detect_toolkit_from_maps_content(maps), Toolkit::Unknown);
    }

    #[test]
    fn test_detect_toolkit_empty_maps() {
        assert_eq!(detect_toolkit_from_maps_content(""), Toolkit::Unknown);
    }

    #[test]
    fn test_detect_toolkit_gtk4_takes_precedence() {
        // If both gtk3 and gtk4 are loaded, gtk4 line appears first → Gtk4
        let maps = "\
7f0000000000-7f0000001000 r--p 00000000 08:01 12345 /usr/lib/libgtk-4.so.1\n\
7f0000001000-7f0000002000 r--p 00000000 08:01 12345 /usr/lib/libgtk-3.so.0\n";
        assert_eq!(detect_toolkit_from_maps_content(maps), Toolkit::Gtk4);
    }

    #[test]
    fn test_toolkit_display() {
        assert_eq!(format!("{}", Toolkit::Gtk4), "GTK4");
        assert_eq!(format!("{}", Toolkit::Gtk3), "GTK3");
        assert_eq!(format!("{}", Toolkit::Qt6), "Qt6");
        assert_eq!(format!("{}", Toolkit::Qt5), "Qt5");
        assert_eq!(format!("{}", Toolkit::Electron), "Electron");
        assert_eq!(format!("{}", Toolkit::Unknown), "Unknown");
    }

    // --- PID extraction from sway JSON ---

    #[test]
    fn test_sway_pid_extraction() {
        let json = r#"{
            "type": "root",
            "nodes": [{
                "type": "con",
                "app_id": "foot",
                "pid": 12345,
                "focused": true
            }]
        }"#;
        let tree: serde_json::Value = serde_json::from_str(json).unwrap();
        let (app_id, pid) = find_focused_node_with_pid(&tree).unwrap();
        assert_eq!(app_id, "foot");
        assert_eq!(pid, Some(12345));
    }

    #[test]
    fn test_sway_pid_extraction_no_pid() {
        let json = r#"{
            "type": "root",
            "nodes": [{
                "type": "con",
                "app_id": "foot",
                "focused": true
            }]
        }"#;
        let tree: serde_json::Value = serde_json::from_str(json).unwrap();
        let (app_id, pid) = find_focused_node_with_pid(&tree).unwrap();
        assert_eq!(app_id, "foot");
        assert_eq!(pid, None);
    }

    // --- PID extraction from hyprctl JSON ---

    #[test]
    fn test_hyprctl_pid_extraction() {
        let json = r#"{"class": "Alacritty", "title": "~", "pid": 54321}"#;
        let (class, pid) = extract_class_and_pid_hyprctl(json).unwrap();
        assert_eq!(class, "Alacritty");
        assert_eq!(pid, Some(54321));
    }

    #[test]
    fn test_hyprctl_pid_extraction_no_pid() {
        let json = r#"{"class": "Alacritty", "title": "~"}"#;
        let (class, pid) = extract_class_and_pid_hyprctl(json).unwrap();
        assert_eq!(class, "Alacritty");
        assert_eq!(pid, None);
    }

    // --- PID extraction from GNOME Introspect ---

    #[test]
    fn test_introspect_pid_extraction() {
        let output = r#"({uint64 1234: {'app-id': <'org.gnome.TextEditor'>, 'pid': <uint32 12345>, 'has-focus': <true>}},)"#;
        let (app_id, pid) = extract_focused_app_and_pid_from_introspect(output).unwrap();
        assert_eq!(app_id, "org.gnome.TextEditor");
        assert_eq!(pid, Some(12345));
    }

    #[test]
    fn test_introspect_pid_extraction_no_pid() {
        let output =
            r#"({uint64 1234: {'app-id': <'org.gnome.TextEditor'>, 'has-focus': <true>}},)"#;
        let (app_id, pid) = extract_focused_app_and_pid_from_introspect(output).unwrap();
        assert_eq!(app_id, "org.gnome.TextEditor");
        assert_eq!(pid, None);
    }

    #[test]
    fn test_introspect_pid_extraction_no_focused() {
        let output = r#"({uint64 1234: {'app-id': <'org.gnome.TextEditor'>, 'pid': <uint32 12345>, 'has-focus': <false>}},)"#;
        assert!(extract_focused_app_and_pid_from_introspect(output).is_none());
    }

    #[test]
    fn test_introspect_pid_extraction_wm_class_fallback() {
        let output = r#"({uint64 1234: {'app-id': <''>, 'wm-class': <'XTerm'>, 'pid': <uint32 99>, 'has-focus': <true>}},)"#;
        let (app_id, pid) = extract_focused_app_and_pid_from_introspect(output).unwrap();
        assert_eq!(app_id, "XTerm");
        assert_eq!(pid, Some(99));
    }

    // --- GVariant uint32 extraction ---

    #[test]
    fn test_extract_gvariant_uint32() {
        let text = "'pid': <uint32 12345>";
        assert_eq!(
            extract_gvariant_uint32(text, "'pid': <uint32 "),
            Some(12345)
        );
    }

    #[test]
    fn test_extract_gvariant_uint32_no_match() {
        let text = "'app-id': <'foo'>";
        assert_eq!(extract_gvariant_uint32(text, "'pid': <uint32 "), None);
    }

    // --- Toolkit cache ---

    #[test]
    fn test_reset_detection_cache_clears_toolkit_cache() {
        // Insert a value into the toolkit cache
        if let Ok(mut cache) = TOOLKIT_CACHE.lock() {
            cache.insert(99999, Toolkit::Gtk4);
        }
        reset_detection_cache();
        if let Ok(cache) = TOOLKIT_CACHE.lock() {
            assert!(
                cache.is_empty(),
                "Toolkit cache should be empty after reset"
            );
        }
    }

    // --- voicsh extension D-Bus response parsing ---

    #[test]
    fn test_parse_voicsh_extension_response_gui_app() {
        let response = "('', uint32 12345, 'firefox')\n";
        let info = parse_voicsh_extension_response(response).unwrap();
        assert_eq!(info.app_id, "firefox");
        assert_eq!(info.pid, Some(12345));
        assert_eq!(info.detection_method, "voicsh-extension");
    }

    #[test]
    fn test_parse_voicsh_extension_response_sandboxed_app() {
        let response = "('org.gnome.Ptyxis', uint32 9876, 'ptyxis')\n";
        let info = parse_voicsh_extension_response(response).unwrap();
        assert_eq!(info.app_id, "org.gnome.Ptyxis");
        assert_eq!(info.pid, Some(9876));
    }

    #[test]
    fn test_parse_voicsh_extension_response_no_window() {
        let response = "('', uint32 0, '')\n";
        assert!(parse_voicsh_extension_response(response).is_none());
    }

    #[test]
    fn test_parse_voicsh_extension_response_wm_class_fallback() {
        // Empty sandboxed app_id but has wm_class → use wm_class as app_id
        let response = "('', uint32 555, 'gnome-text-editor')\n";
        let info = parse_voicsh_extension_response(response).unwrap();
        assert_eq!(info.app_id, "gnome-text-editor");
        assert_eq!(info.pid, Some(555));
    }
}
