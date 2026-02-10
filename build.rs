//! Build script: pre-flight checks for GPU feature flags.
//!
//! Verifies that required toolkits are installed before whisper-rs-sys tries
//! to compile. For version mismatches (which we can't reliably detect ahead
//! of time), we print helpful diagnostic info that will appear in the build
//! output if compilation fails.

use std::process::Command;

fn main() {
    // Embed git short hash for version string
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        && output.status.success()
    {
        let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
        println!("cargo:rustc-env=GIT_HASH={}", hash);
    }
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/");

    if cfg!(feature = "cuda") {
        check_cuda();
    }
    if cfg!(feature = "vulkan") {
        check_vulkan();
    }
    if cfg!(feature = "hipblas") {
        check_rocm();
    }
    if cfg!(feature = "openblas") {
        check_openblas();
    }
}

fn check_cuda() {
    let output = Command::new("nvcc").arg("--version").output();
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let version = parse_cuda_version(&text);

            // Print diagnostic info — this appears BEFORE whisper-rs-sys compiles,
            // so we tell the user to scroll up if they see errors
            println!("cargo::warning=");
            println!(
                "cargo::warning=╔═══════════════════════════════════════════════════════════════╗"
            );
            println!(
                "cargo::warning=║  CUDA BUILD — SCROLL UP HERE IF BUILD FAILS                  ║"
            );
            println!(
                "cargo::warning=╠═══════════════════════════════════════════════════════════════╣"
            );
            if let Some((major, minor)) = version {
                println!(
                    "cargo::warning=║  Toolkit: CUDA {}.{}                                            ║",
                    major, minor
                );
            } else {
                println!(
                    "cargo::warning=║  Toolkit: CUDA (version unknown)                              ║"
                );
            }
            if let Some(driver_cuda) = get_driver_cuda_version() {
                println!(
                    "cargo::warning=║  Driver:  supports up to CUDA {}                            ║",
                    driver_cuda
                );
            }
            println!(
                "cargo::warning=╠═══════════════════════════════════════════════════════════════╣"
            );
            println!(
                "cargo::warning=║  If you see 'Unsupported gpu architecture':                  ║"
            );
            println!(
                "cargo::warning=║  → Your GPU needs a newer CUDA toolkit                       ║"
            );
            println!(
                "cargo::warning=║  → Update: https://developer.nvidia.com/cuda-downloads       ║"
            );
            println!(
                "cargo::warning=╚═══════════════════════════════════════════════════════════════╝"
            );
            println!("cargo::warning=");
        }
        _ => {
            panic!(
                "\n\n\
                ╔══════════════════════════════════════════════════════════╗\n\
                ║  `nvcc` not found — CUDA toolkit is not installed.       ║\n\
                ║                                                          ║\n\
                ║  Install: https://developer.nvidia.com/cuda-downloads    ║\n\
                ║  Or build without CUDA: cargo build --release            ║\n\
                ╚══════════════════════════════════════════════════════════╝\n",
            );
        }
    }
}

/// Parse "release X.Y" from nvcc --version output.
fn parse_cuda_version(text: &str) -> Option<(u32, u32)> {
    // nvcc output: "Cuda compilation tools, release 12.4, V12.4.131"
    let release_pos = text.find("release ")?;
    let after = &text[release_pos + 8..];
    let comma = after.find(',')?;
    let version_str = &after[..comma];
    let mut parts = version_str.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Get the CUDA version supported by the driver from nvidia-smi.
fn get_driver_cuda_version() -> Option<String> {
    let output = Command::new("nvidia-smi")
        .arg("--query-gpu=driver_version")
        .arg("--format=csv,noheader")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // nvidia-smi header shows "CUDA Version: X.Y", parse it from full output
    let full_output = Command::new("nvidia-smi").output().ok()?;
    let text = String::from_utf8_lossy(&full_output.stdout);

    // Look for "CUDA Version: X.Y"
    let cuda_pos = text.find("CUDA Version:")?;
    let after = &text[cuda_pos + 14..];
    let end = after.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    Some(after[..end].trim().to_string())
}

fn check_vulkan() {
    if Command::new("vulkaninfo")
        .arg("--summary")
        .output()
        .is_err()
    {
        panic!(
            "\n\n\
            ╔══════════════════════════════════════════════════════════╗\n\
            ║  `vulkaninfo` not found — Vulkan SDK is not installed.   ║\n\
            ║                                                          ║\n\
            ║  Install: https://vulkan.lunarg.com/                     ║\n\
            ║  Or build without Vulkan: cargo build --release          ║\n\
            ╚══════════════════════════════════════════════════════════╝\n",
        );
    }
    println!("cargo::warning=Vulkan SDK detected");
}

fn check_rocm() {
    if Command::new("rocminfo").output().is_err() {
        panic!(
            "\n\n\
            ╔══════════════════════════════════════════════════════════╗\n\
            ║  `rocminfo` not found — ROCm is not installed.           ║\n\
            ║                                                          ║\n\
            ║  Install: https://rocm.docs.amd.com/                     ║\n\
            ║  Or build without HipBLAS: cargo build --release         ║\n\
            ╚══════════════════════════════════════════════════════════╝\n",
        );
    }
    println!("cargo::warning=ROCm detected");
}

fn check_openblas() {
    // Check for libopenblas via pkg-config or known paths
    let pkg_config_ok = Command::new("pkg-config")
        .args(["--exists", "openblas"])
        .status()
        .is_ok_and(|s| s.success());

    if !pkg_config_ok {
        // Fallback: check if the shared library exists
        let lib_exists = std::path::Path::new("/usr/lib/x86_64-linux-gnu/libopenblas.so").exists()
            || std::path::Path::new("/usr/lib/libopenblas.so").exists()
            || std::path::Path::new("/usr/lib64/libopenblas.so").exists();

        if !lib_exists {
            panic!(
                "\n\n\
                ╔══════════════════════════════════════════════════════════╗\n\
                ║  OpenBLAS not found.                                     ║\n\
                ║                                                          ║\n\
                ║  Install: sudo apt install libopenblas-dev               ║\n\
                ║  Or build without OpenBLAS: cargo build --release        ║\n\
                ╚══════════════════════════════════════════════════════════╝\n",
            );
        }
    }
    println!("cargo::warning=OpenBLAS detected");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cuda_version_standard() {
        let text = "nvcc: NVIDIA (R) Cuda compiler driver\n\
                    Copyright (c) 2005-2024 NVIDIA Corporation\n\
                    Built on Thu_Mar_28_02:18:24_PDT_2024\n\
                    Cuda compilation tools, release 12.4, V12.4.131\n\
                    Build cuda_12.4.r12.4/compiler.34097967_0";
        assert_eq!(parse_cuda_version(text), Some((12, 4)));
    }

    #[test]
    fn parse_cuda_version_13() {
        let text = "Cuda compilation tools, release 13.0, V13.0.76";
        assert_eq!(parse_cuda_version(text), Some((13, 0)));
    }

    #[test]
    fn parse_cuda_version_no_match() {
        assert_eq!(parse_cuda_version("no version here"), None);
    }

    #[test]
    fn parse_cuda_version_partial() {
        assert_eq!(parse_cuda_version("release abc, V1"), None);
    }
}
