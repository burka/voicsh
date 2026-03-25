//! Systemd user service management.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SERVICE_NAME: &str = "voicsh.service";

/// Install systemd user service and activate it (start or restart).
///
/// Steps:
/// 1. Resolve systemd user directory (XDG_CONFIG_HOME or HOME)
/// 2. Create directory if needed
/// 3. Get current executable path
/// 4. Write unit file
/// 5. Reload systemd daemon
/// 6. Check if already active
/// 7. Restart if active, enable+start if not
pub fn install_and_activate() -> Result<()> {
    let systemd_dir = service_dir()?;
    fs::create_dir_all(&systemd_dir).context("Failed to create systemd user directory")?;

    let exe_path = std::env::current_exe().context("Failed to get current executable path")?;

    let service_content = format!(
        r#"[Unit]
Description=voicsh - Voice typing daemon
After=graphical-session.target

[Service]
Type=simple
ExecStart={} daemon
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
        exe_path.display()
    );

    let service_path = systemd_dir.join(SERVICE_NAME);
    fs::write(&service_path, service_content).context("Failed to write service file")?;

    eprintln!("Service file written to: {}", service_path.display());

    run_systemctl(&["daemon-reload"], "daemon-reload")?;

    // Check if service is already active
    let is_active = Command::new("systemctl")
        .args(["--user", "is-active", SERVICE_NAME])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if is_active {
        eprintln!("Service is already active, restarting...");
        run_systemctl(&["restart", SERVICE_NAME], "restart voicsh")?;
        println!("Service restarted.");
    } else {
        eprintln!("Enabling and starting service...");
        run_systemctl(
            &["enable", "--now", SERVICE_NAME],
            "enable and start voicsh",
        )?;
        println!("Service enabled and started.");
    }

    Ok(())
}

/// Stop, disable, and remove systemd user service.
///
/// Best-effort cleanup: stop and disable failures are logged but don't halt removal.
/// Reload failure is fatal (indicates systemd communication problem).
pub fn stop_and_disable() -> Result<()> {
    // Stop service (cleanup — warn on failure, don't bail)
    warn_systemctl(&["stop", SERVICE_NAME], "stop service");

    // Disable service (cleanup — warn on failure, don't bail)
    warn_systemctl(&["disable", SERVICE_NAME], "disable service");

    // Remove service file
    let systemd_dir = service_dir()?;
    let service_path = systemd_dir.join(SERVICE_NAME);
    if service_path.exists() {
        fs::remove_file(&service_path).context("Failed to remove service file")?;
    }

    // Reload systemd (fatal — indicates communication problem)
    run_systemctl(&["daemon-reload"], "daemon-reload")?;

    Ok(())
}

/// Run `systemctl --user <args>` and fail if the command exits non-zero.
fn run_systemctl(args: &[&str], action: &str) -> Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()
        .with_context(|| format!("Failed to run systemctl {action}"))?;

    anyhow::ensure!(
        status.success(),
        "systemctl {action} failed (exit code: {status}). Check: systemctl --user status"
    );
    Ok(())
}

/// Run `systemctl --user <args>`, logging warnings on failure (best-effort cleanup).
fn warn_systemctl(args: &[&str], action: &str) {
    match Command::new("systemctl").arg("--user").args(args).status() {
        Ok(s) if !s.success() => eprintln!("Warning: systemctl {action} exited with {s}"),
        Err(e) => eprintln!("Warning: Failed to run systemctl {action}: {e}"),
        Ok(_) => {}
    }
}

/// Resolve systemd user service directory.
///
/// Checks XDG_CONFIG_HOME first, falls back to HOME/.config.
fn service_dir() -> Result<PathBuf> {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        Ok(PathBuf::from(config_home).join("systemd/user"))
    } else if let Ok(home) = std::env::var("HOME") {
        Ok(PathBuf::from(home).join(".config/systemd/user"))
    } else {
        anyhow::bail!("Could not determine user config directory (HOME or XDG_CONFIG_HOME)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes all tests that mutate environment variables.
    ///
    /// Environment variables are process-global. Without serialization, concurrent tests
    /// that set/remove XDG_CONFIG_HOME or HOME will race and produce non-deterministic results.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Test that service_dir uses XDG_CONFIG_HOME when set.
    #[test]
    fn service_dir_uses_xdg_config_home() {
        let _guard = ENV_LOCK.lock().expect("ENV_LOCK poisoned");

        let prev_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        crate::sys::set_env("XDG_CONFIG_HOME", "/custom/config");

        let result = service_dir();

        // Restore env state before asserting to ensure cleanup on panic.
        match prev_xdg {
            Some(v) => crate::sys::set_env("XDG_CONFIG_HOME", &v),
            None => crate::sys::remove_env("XDG_CONFIG_HOME"),
        }

        let path = result.expect("service_dir should succeed when XDG_CONFIG_HOME is set");
        assert_eq!(
            path,
            PathBuf::from("/custom/config/systemd/user"),
            "should use XDG_CONFIG_HOME as the config root"
        );
    }

    /// Test that service_dir falls back to HOME/.config when XDG_CONFIG_HOME is absent.
    #[test]
    fn service_dir_falls_back_to_home() {
        let _guard = ENV_LOCK.lock().expect("ENV_LOCK poisoned");

        let prev_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let prev_home = std::env::var("HOME").ok();

        crate::sys::remove_env("XDG_CONFIG_HOME");
        crate::sys::set_env("HOME", "/home/testuser");

        let result = service_dir();

        match prev_xdg {
            Some(v) => crate::sys::set_env("XDG_CONFIG_HOME", &v),
            None => crate::sys::remove_env("XDG_CONFIG_HOME"),
        }
        match prev_home {
            Some(v) => crate::sys::set_env("HOME", &v),
            None => crate::sys::remove_env("HOME"),
        }

        let path = result.expect("service_dir should succeed when HOME is set");
        assert_eq!(
            path,
            PathBuf::from("/home/testuser/.config/systemd/user"),
            "should use HOME/.config as the config root"
        );
    }

    /// Test that service_dir returns an error when neither XDG_CONFIG_HOME nor HOME is set.
    #[test]
    fn service_dir_errors_without_home_or_xdg() {
        let _guard = ENV_LOCK.lock().expect("ENV_LOCK poisoned");

        let prev_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let prev_home = std::env::var("HOME").ok();

        crate::sys::remove_env("XDG_CONFIG_HOME");
        crate::sys::remove_env("HOME");

        let result = service_dir();

        match prev_xdg {
            Some(v) => crate::sys::set_env("XDG_CONFIG_HOME", &v),
            None => crate::sys::remove_env("XDG_CONFIG_HOME"),
        }
        match prev_home {
            Some(v) => crate::sys::set_env("HOME", &v),
            None => crate::sys::remove_env("HOME"),
        }

        let err = result.expect_err("service_dir should fail without HOME or XDG_CONFIG_HOME");
        let msg = err.to_string();
        assert!(
            msg.contains("HOME") || msg.contains("XDG_CONFIG_HOME"),
            "error should mention the missing env vars, got: {}",
            msg
        );
    }

    /// Test that the unit file template contains required systemd section headers and the
    /// ExecStart field. This validates the format string without invoking systemctl.
    #[test]
    fn service_file_content_has_required_sections() {
        // Reconstruct the same template logic used in install_and_activate, using a
        // known executable path to avoid depending on the actual binary location.
        let exe_path = std::path::PathBuf::from("/usr/bin/voicsh");
        let content = format!(
            r#"[Unit]
Description=voicsh - Voice typing daemon
After=graphical-session.target

[Service]
Type=simple
ExecStart={} daemon
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
            exe_path.display()
        );

        assert!(
            content.contains("[Unit]"),
            "unit file must contain [Unit] section"
        );
        assert!(
            content.contains("[Service]"),
            "unit file must contain [Service] section"
        );
        assert!(
            content.contains("[Install]"),
            "unit file must contain [Install] section"
        );
        assert_eq!(
            content
                .lines()
                .find(|l| l.starts_with("ExecStart="))
                .expect("unit file must have ExecStart line"),
            "ExecStart=/usr/bin/voicsh daemon",
            "ExecStart must point to the binary with 'daemon' subcommand"
        );
        assert!(
            content.contains("WantedBy=default.target"),
            "unit file must target default.target for user services"
        );
    }
}
