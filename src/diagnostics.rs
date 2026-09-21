//! System diagnostics and dependency checking.
//!
//! Verifies that required system tools are installed and configured correctly.

use crate::defaults;
use std::process::Command;

#[cfg(feature = "benchmark")]
use crate::benchmark::detect_gpu;

/// Result of a dependency check.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckResult {
    /// Tool is installed and working
    Ok,
    /// Tool is not found
    NotFound,
    /// Tool is found but has issues (e.g., daemon not running)
    Warning(String),
}

/// Check if a command exists and is executable.
pub fn check_command(command: &str) -> CheckResult {
    match Command::new(command).arg("--version").output() {
        Ok(output) if output.status.success() => CheckResult::Ok,
        Ok(_) => CheckResult::Warning(format!("'{}' found but --version failed", command)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => CheckResult::NotFound,
        Err(e) => CheckResult::Warning(format!("Error checking '{}': {}", command, e)),
    }
}

/// Check if wtype is available (simpler Wayland typing tool).
pub fn check_wtype() -> CheckResult {
    match Command::new("wtype").arg("--help").output() {
        Ok(output) if output.status.success() => CheckResult::Ok,
        Ok(_) => CheckResult::Ok, // --help might return non-zero but still work
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => CheckResult::NotFound,
        Err(e) => CheckResult::Warning(format!("Error checking wtype: {}", e)),
    }
}

/// Check ydotool backend availability by examining its output.
pub fn check_ydotool_backend() -> CheckResult {
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
pub fn check_portal() -> CheckResult {
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

/// Check whether at least one Whisper model is installed.
#[cfg(feature = "model-download")]
pub fn check_model_installed() -> CheckResult {
    let installed = crate::models::download::list_installed_models();
    if installed.is_empty() {
        CheckResult::NotFound
    } else {
        CheckResult::Ok
    }
}

/// Run all dependency checks and print results.
pub fn check_dependencies() {
    println!("Checking system dependencies...\n");

    // Check Whisper model
    #[cfg(feature = "model-download")]
    {
        let installed = crate::models::download::list_installed_models();
        print!("Whisper model:                    ");
        if installed.is_empty() {
            println!("✗ NOT FOUND");
            println!("  No model installed. Run: voicsh init");
            println!("  Or: voicsh models download tiny.en");
        } else {
            println!("✓ OK ({})", installed.join(", "));
        }
        println!();
    }

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
    print!("  NVIDIA (CUDA):   ");
    print_gpu_check(check_gpu_nvidia(compiled));
    print!("  Vulkan:          ");
    print_gpu_check(check_gpu_vulkan(compiled));
    print!("  AMD (ROCm):      ");
    print_gpu_check(check_gpu_rocm(compiled));

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

/// Outcome of probing one GPU backend against the compiled binary.
#[derive(Debug, Clone, PartialEq)]
enum GpuCheck {
    /// Hardware detected and the binary was built with the matching backend.
    Active(String),
    /// Hardware detected but the binary wasn't built with this backend.
    Found {
        name: String,
        rebuild_features: &'static str,
    },
    /// No hardware/tooling for this backend found.
    NotFound(&'static str),
}

fn print_gpu_check(check: GpuCheck) {
    match check {
        GpuCheck::Active(name) => println!("✓ Active ({})", name),
        GpuCheck::Found {
            name,
            rebuild_features,
        } => println!(
            "✓ {} found → rebuild with: cargo build --release --features {}",
            name, rebuild_features
        ),
        GpuCheck::NotFound(reason) => println!("- {}", reason),
    }
}

/// Check for NVIDIA GPU via GPU detection.
fn check_gpu_nvidia(compiled: &str) -> GpuCheck {
    #[cfg(feature = "benchmark")]
    {
        // Use shared GPU detection when benchmark feature is available
        if let Some(gpu) = detect_gpu()
            && gpu.name.starts_with("NVIDIA")
        {
            let name = gpu.name.strip_prefix("NVIDIA ").unwrap_or(&gpu.name);
            return if compiled == "CUDA" {
                GpuCheck::Active(name.to_string())
            } else {
                GpuCheck::Found {
                    name: name.to_string(),
                    rebuild_features: "cuda",
                }
            };
        }
        GpuCheck::NotFound("not detected")
    }

    #[cfg(not(feature = "benchmark"))]
    {
        // Fallback to direct nvidia-smi check when benchmark feature not available
        match Command::new("nvidia-smi")
            .arg("--query-gpu=gpu_name")
            .arg("--format=csv,noheader")
            .output()
        {
            Ok(output) if output.status.success() => {
                let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if compiled == "CUDA" {
                    GpuCheck::Active(name)
                } else {
                    GpuCheck::Found {
                        name,
                        rebuild_features: "cuda",
                    }
                }
            }
            _ => GpuCheck::NotFound("nvidia-smi not found"),
        }
    }
}

/// Check for Vulkan support via `vulkaninfo`.
fn check_gpu_vulkan(compiled: &str) -> GpuCheck {
    match Command::new("vulkaninfo").arg("--summary").output() {
        Ok(output) if output.status.success() => {
            if compiled == "Vulkan" {
                GpuCheck::Active("vulkaninfo".to_string())
            } else {
                GpuCheck::Found {
                    name: "vulkaninfo".to_string(),
                    rebuild_features: "vulkan",
                }
            }
        }
        _ => GpuCheck::NotFound("vulkaninfo not found"),
    }
}

/// Check for AMD GPU via `rocminfo`.
fn check_gpu_rocm(compiled: &str) -> GpuCheck {
    match Command::new("rocminfo").output() {
        Ok(output) if output.status.success() => {
            if compiled == "HipBLAS (AMD)" {
                GpuCheck::Active("rocminfo".to_string())
            } else {
                GpuCheck::Found {
                    name: "rocminfo".to_string(),
                    rebuild_features: "hipblas",
                }
            }
        }
        _ => GpuCheck::NotFound("rocminfo not found"),
    }
}

/// One-line rebuild suggestion for `voicsh init`, if compiled hardware
/// support doesn't match what's actually detected on this machine.
/// Returns `None` when the compiled backend already matches, or no
/// GPU backend was detected at all — never changes build defaults itself,
/// per INSTALL.md's "GPU feature gates are untested and unverified" stance.
pub fn gpu_build_suggestion() -> Option<String> {
    let compiled = defaults::gpu_backend();
    for check in [
        check_gpu_nvidia(compiled),
        check_gpu_vulkan(compiled),
        check_gpu_rocm(compiled),
    ] {
        if let GpuCheck::Found {
            name,
            rebuild_features,
        } = check
        {
            return Some(format!(
                "{} detected but not compiled in (running on CPU). For GPU acceleration: \
                 cargo build --release --features {} (untested/unverified — see INSTALL.md)",
                name, rebuild_features
            ));
        }
    }
    None
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

    #[cfg(feature = "model-download")]
    #[test]
    fn test_check_model_installed_returns_valid_result() {
        // Justified exception: outcome is machine-dependent (models may or may not
        // be installed). We validate the return type is not Warning (which would
        // indicate a bug in the detection logic), but cannot assert a specific
        // Ok vs NotFound value without controlling the filesystem.
        let result = check_model_installed();
        match result {
            CheckResult::Ok => { /* model is installed — valid */ }
            CheckResult::NotFound => { /* no model — also valid on a fresh machine */ }
            CheckResult::Warning(msg) => {
                panic!("check_model_installed should not return Warning, got: {msg}")
            }
        }
    }

    #[test]
    fn test_check_dependencies_runs_without_panic() {
        // Justified exception: check_dependencies() prints directly to stdout and
        // returns (). There is no return value to assert on, and capturing stdout
        // would add complexity for minimal value. The test validates no panic occurs.
        check_dependencies();
    }

    #[test]
    fn gpu_nvidia_returns_valid_variant() {
        // Justified exception: outcome is machine-dependent (GPU may or may not be
        // present). We only assert the function returns a well-formed variant.
        match check_gpu_nvidia("CPU") {
            GpuCheck::Active(_) | GpuCheck::Found { .. } | GpuCheck::NotFound(_) => {}
        }
    }

    #[test]
    fn gpu_vulkan_returns_valid_variant() {
        match check_gpu_vulkan("CPU") {
            GpuCheck::Active(_) | GpuCheck::Found { .. } | GpuCheck::NotFound(_) => {}
        }
    }

    #[test]
    fn gpu_rocm_returns_valid_variant() {
        match check_gpu_rocm("CPU") {
            GpuCheck::Active(_) | GpuCheck::Found { .. } | GpuCheck::NotFound(_) => {}
        }
    }

    #[test]
    fn gpu_build_suggestion_returns_valid_result() {
        // Justified exception: machine-dependent (GPU hardware may or may not be
        // present in CI/test environments). Validate the type is well-formed:
        // when Some, it must name a --features flag consumers can act on.
        match gpu_build_suggestion() {
            None => {}
            Some(msg) => assert!(
                msg.contains("--features"),
                "suggestion should mention a --features flag, got: {msg}"
            ),
        }
    }
}
