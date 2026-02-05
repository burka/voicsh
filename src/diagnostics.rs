//! System diagnostics and dependency checking.
//!
//! Verifies that required system tools are installed and configured correctly.

use crate::defaults;
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

/// Check if wtype is available (simpler Wayland typing tool).
fn check_wtype() -> CheckResult {
    match Command::new("wtype").arg("--help").output() {
        Ok(output) if output.status.success() => CheckResult::Ok,
        Ok(_) => CheckResult::Ok, // --help might return non-zero but still work
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => CheckResult::NotFound,
        Err(e) => CheckResult::Warning(format!("Error checking wtype: {}", e)),
    }
}

/// Check ydotool backend availability by examining its output.
fn check_ydotool_backend() -> CheckResult {
    // Run ydotool with a simple command that triggers backend check
    match Command::new("ydotool").args(["type", "--help"]).output() {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("backend unavailable") {
                CheckResult::Warning(
                    "ydotool shows 'backend unavailable'. The ydotoold daemon is needed.\n\
                     For ydotool 0.1.x: install ydotoold separately or upgrade to ydotool 1.0+\n\
                     Alternative: install wtype (simpler, no daemon needed):\n\
                       sudo apt install wtype  (Debian/Ubuntu)\n\
                       sudo pacman -S wtype    (Arch)"
                        .to_string(),
                )
            } else {
                CheckResult::Ok
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => CheckResult::NotFound,
        Err(e) => CheckResult::Warning(format!("Error checking ydotool: {}", e)),
    }
}

/// Check if xdg-desktop-portal RemoteDesktop is available.
fn check_portal() -> CheckResult {
    match Command::new("gdbus")
        .args([
            "introspect",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
        ])
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("RemoteDesktop") {
                CheckResult::Ok
            } else {
                CheckResult::Warning(
                    "Portal available but RemoteDesktop interface missing".to_string(),
                )
            }
        }
        Ok(_) => CheckResult::NotFound,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            CheckResult::Warning("gdbus not found (needed for portal check)".to_string())
        }
        Err(e) => CheckResult::Warning(format!("Error checking portal: {}", e)),
    }
}

/// Run all dependency checks and print results.
pub fn check_dependencies() {
    println!("Checking system dependencies...\n");

    // Check xdg-desktop-portal RemoteDesktop (GNOME/KDE key injection)
    print!("xdg-desktop-portal RemoteDesktop: ");
    let portal_available = match check_portal() {
        CheckResult::Ok => {
            println!("✓ OK (portal key injection available)");
            true
        }
        CheckResult::NotFound => {
            println!("- not available");
            false
        }
        CheckResult::Warning(msg) => {
            println!("⚠ WARNING: {}", msg);
            false
        }
    };

    // Check wl-copy (clipboard)
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

    // Check wtype (preferred input method - simpler, no daemon)
    print!("wtype (input injection): ");
    let wtype_available = match check_wtype() {
        CheckResult::Ok => {
            println!("✓ OK (preferred - no daemon needed)");
            true
        }
        CheckResult::NotFound => {
            println!("- not installed");
            false
        }
        CheckResult::Warning(msg) => {
            println!("⚠ WARNING: {}", msg);
            false
        }
    };

    // Check ydotool (fallback input method)
    print!("ydotool (input injection): ");
    match check_command("ydotool") {
        CheckResult::Ok | CheckResult::Warning(_) => {
            // ydotool binary exists, check backend
            match check_ydotool_backend() {
                CheckResult::Ok => {
                    println!("✓ OK");
                }
                CheckResult::Warning(msg) => {
                    println!("⚠ WARNING");
                    for line in msg.lines() {
                        println!("  {}", line);
                    }
                }
                CheckResult::NotFound => println!("✗ NOT FOUND"),
            }
        }
        CheckResult::NotFound => {
            println!("- not installed");
            if !wtype_available {
                println!("  Install wtype (recommended): sudo apt install wtype");
                println!("  Or ydotool: sudo apt install ydotool");
            }
        }
    }

    // GPU acceleration
    println!();
    println!("GPU acceleration:");
    let compiled = defaults::gpu_backend();
    println!("  Compiled backend: {}", compiled);
    check_gpu_nvidia(compiled);
    check_gpu_vulkan(compiled);
    check_gpu_rocm(compiled);

    println!();
    if portal_available {
        println!("✓ Portal key injection available (best for GNOME).");
    }
    if wtype_available {
        println!("✓ Ready to inject text using wtype + wl-copy.");
    }
    if !portal_available && !wtype_available {
        println!("⚠ Text injection may not work. Install wtype for best results:");
        println!("  sudo apt install wtype    (Debian/Ubuntu)");
        println!("  sudo pacman -S wtype      (Arch)");
    }
}

/// Check for NVIDIA GPU via `nvidia-smi`.
fn check_gpu_nvidia(compiled: &str) {
    print!("  NVIDIA (CUDA):   ");
    match Command::new("nvidia-smi")
        .arg("--query-gpu=gpu_name")
        .arg("--format=csv,noheader")
        .output()
    {
        Ok(output) if output.status.success() => {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if compiled == "CUDA" {
                println!("✓ Active ({})", name);
            } else {
                println!(
                    "✓ {} found → rebuild with: cargo build --release --features cuda",
                    name
                );
            }
        }
        _ => println!("- nvidia-smi not found"),
    }
}

/// Check for Vulkan support via `vulkaninfo`.
fn check_gpu_vulkan(compiled: &str) {
    print!("  Vulkan:          ");
    match Command::new("vulkaninfo").arg("--summary").output() {
        Ok(output) if output.status.success() => {
            if compiled == "Vulkan" {
                println!("✓ Active");
            } else {
                println!(
                    "✓ vulkaninfo found → rebuild with: cargo build --release --features vulkan"
                );
            }
        }
        _ => println!("- vulkaninfo not found"),
    }
}

/// Check for AMD GPU via `rocminfo`.
fn check_gpu_rocm(compiled: &str) {
    print!("  AMD (ROCm):      ");
    match Command::new("rocminfo").output() {
        Ok(output) if output.status.success() => {
            if compiled == "HipBLAS (AMD)" {
                println!("✓ Active");
            } else {
                println!(
                    "✓ rocminfo found → rebuild with: cargo build --release --features hipblas"
                );
            }
        }
        _ => println!("- rocminfo not found"),
    }
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

    #[test]
    fn gpu_nvidia_runs_without_panic() {
        // Just verify it doesn't panic regardless of whether nvidia-smi exists
        check_gpu_nvidia("CPU");
    }

    #[test]
    fn gpu_vulkan_runs_without_panic() {
        check_gpu_vulkan("CPU");
    }

    #[test]
    fn gpu_rocm_runs_without_panic() {
        check_gpu_rocm("CPU");
    }
}
