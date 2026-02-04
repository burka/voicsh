//! System diagnostics and dependency checking.
//!
//! Verifies that required system tools are installed and configured correctly.

use std::process::Command;

/// Result of a dependency check.
#[derive(Debug, PartialEq)]
pub enum CheckResult {
    /// Tool is installed and working
    Ok,
    /// Tool is not found
    NotFound,
    /// Tool is found but has issues (e.g., daemon not running)
    Warning(String),
}

/// Check if a command exists and is executable.
fn check_command(command: &str) -> CheckResult {
    match Command::new(command).arg("--version").output() {
        Ok(output) if output.status.success() => CheckResult::Ok,
        Ok(_) => CheckResult::Warning(format!("'{}' found but --version failed", command)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => CheckResult::NotFound,
        Err(e) => CheckResult::Warning(format!("Error checking '{}': {}", command, e)),
    }
}

/// Check if ydotoold daemon is running.
fn check_ydotool_daemon() -> CheckResult {
    // Try to check if ydotoold is running by executing a harmless ydotool command
    match Command::new("systemctl")
        .args(["is-active", "ydotool"])
        .output()
    {
        Ok(output) if output.status.success() => CheckResult::Ok,
        Ok(_) => CheckResult::Warning(
            "ydotoold daemon is not running. Start it with: sudo systemctl enable --now ydotool"
                .to_string(),
        ),
        Err(_) => {
            // systemctl not available, try alternative check
            CheckResult::Warning(
                "Cannot verify ydotoold status. Ensure the daemon is running.".to_string(),
            )
        }
    }
}

/// Run all dependency checks and print results.
pub fn check_dependencies() {
    println!("Checking system dependencies...\n");

    // Check wl-copy
    print!("wl-copy (clipboard): ");
    match check_command("wl-copy") {
        CheckResult::Ok => println!("✓ OK"),
        CheckResult::NotFound => {
            println!("✗ NOT FOUND");
            println!("  Install: sudo apt install wl-clipboard  (Debian/Ubuntu)");
            println!("           sudo pacman -S wl-clipboard    (Arch)");
        }
        CheckResult::Warning(msg) => println!("⚠ WARNING: {}", msg),
    }

    // Check ydotool
    print!("ydotool (input injection): ");
    match check_command("ydotool") {
        CheckResult::Ok => {
            println!("✓ OK");
            // Check if daemon is running
            print!("ydotoold (daemon): ");
            match check_ydotool_daemon() {
                CheckResult::Ok => println!("✓ RUNNING"),
                CheckResult::Warning(msg) => println!("⚠ {}", msg),
                CheckResult::NotFound => {
                    println!("✗ NOT RUNNING");
                    println!("  Start: sudo systemctl enable --now ydotool");
                }
            }
        }
        CheckResult::NotFound => {
            println!("✗ NOT FOUND");
            println!("  Install: sudo apt install ydotool  (Debian/Ubuntu)");
            println!("           sudo pacman -S ydotool    (Arch)");
            println!("  After install, start daemon: sudo systemctl enable --now ydotool");
        }
        CheckResult::Warning(msg) => println!("⚠ WARNING: {}", msg),
    }

    println!("\nNote: Both wl-copy and ydotool are required for text injection to work.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_result_equality() {
        assert_eq!(CheckResult::Ok, CheckResult::Ok);
        assert_eq!(CheckResult::NotFound, CheckResult::NotFound);
        assert_eq!(
            CheckResult::Warning("test".to_string()),
            CheckResult::Warning("test".to_string())
        );
    }

    #[test]
    fn test_check_result_inequality() {
        assert_ne!(CheckResult::Ok, CheckResult::NotFound);
        assert_ne!(
            CheckResult::Warning("a".to_string()),
            CheckResult::Warning("b".to_string())
        );
    }

    #[test]
    fn test_check_command_echo_exists() {
        // echo should exist on all Unix systems and support --version
        let result = check_command("echo");
        // echo might not support --version on all systems, so we accept both Ok and Warning
        match result {
            CheckResult::Ok | CheckResult::Warning(_) => {}
            CheckResult::NotFound => panic!("echo command should be found on Unix systems"),
        }
    }

    #[test]
    fn test_check_command_nonexistent() {
        let result = check_command("nonexistent-command-xyz-12345");
        assert_eq!(result, CheckResult::NotFound);
    }

    #[test]
    fn test_check_dependencies_runs_without_panic() {
        // Just verify it doesn't panic
        check_dependencies();
    }
}
