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
            ║  Install: sudo apt install libvulkan-dev                  ║\n\
            ║           mesa-vulkan-drivers vulkan-tools glslc          ║\n\
            ║  Or build without Vulkan: cargo build --release           ║\n\
            ╚══════════════════════════════════════════════════════════╝\n",
        );
    }

    // glslc compiles GLSL shaders to SPIR-V — required by ggml's Vulkan backend at build time.
    if Command::new("glslc").arg("--version").output().is_err() {
        panic!(
            "\n\n\
            ╔══════════════════════════════════════════════════════════╗\n\
            ║  `glslc` not found — SPIR-V shader compiler missing.    ║\n\
            ║                                                          ║\n\
            ║  Install: sudo apt install glslc                         ║\n\
            ║  Or build without Vulkan: cargo build --release          ║\n\
            ╚══════════════════════════════════════════════════════════╝\n",
        );
    }

    // whisper-rs-sys uses bindgen to generate Vulkan FFI bindings from ggml-vulkan.h.
    // bindgen needs clang's built-in headers (stdbool.h, stddef.h, etc.) which come
    // from libclang-dev, not the runtime libclang1-XX package. Without them, bindgen
    // silently falls back to pre-built bindings that lack Vulkan symbols, causing
    // unresolved import errors in whisper-rs.
    check_libclang_headers();

    println!("cargo::warning=Vulkan SDK detected (vulkaninfo + glslc)");
}

/// Verify that bindgen can find clang's built-in headers at runtime.
///
/// GPU feature builds (Vulkan, CUDA, etc.) rely on bindgen to generate FFI
/// bindings that include backend-specific symbols. The pre-built fallback
/// bindings in whisper-rs-sys only cover the base API — missing GPU symbols
/// cause cryptic "unresolved import" errors at compile time.
///
/// bindgen uses libclang, which locates its resource directory (containing
/// stdbool.h, stddef.h, etc.) via the `clang` binary. If `clang` is not in
/// PATH, libclang can't find the resource dir and bindgen silently falls back
/// to incomplete pre-built bindings — even if the headers exist on disk.
fn check_libclang_headers() {
    // If the user already configured bindgen's clang lookup, trust them.
    if let Ok(args) = std::env::var("BINDGEN_EXTRA_CLANG_ARGS")
        && args.contains("-I")
    {
        return;
    }
    // CLANG_PATH tells bindgen/clang-sys which clang binary to use.
    if std::env::var("CLANG_PATH").is_ok() {
        return;
    }

    // Try `clang -print-resource-dir` — this is exactly what libclang uses
    // to find built-in headers. Try versioned names first (e.g. clang-20),
    // since some distros only install versioned binaries.
    let resource_dir = find_clang_resource_dir();

    match resource_dir {
        Some(dir) => {
            let stdbool = std::path::PathBuf::from(&dir).join("include/stdbool.h");
            if !stdbool.exists() {
                panic!(
                    "\n\n\
                    ╔══════════════════════════════════════════════════════════╗\n\
                    ║  clang resource dir found but stdbool.h is missing.      ║\n\
                    ║  Resource dir: {dir:<43} ║\n\
                    ║                                                          ║\n\
                    ║  Install: sudo apt install libclang-dev                   ║\n\
                    ║  Or build without GPU: cargo build --release              ║\n\
                    ╚══════════════════════════════════════════════════════════╝\n",
                );
            }
        }
        None => {
            // clang binary not found — check if a versioned one exists first.
            if has_versioned_clang() {
                panic!(
                    "\n\n\
                    ╔══════════════════════════════════════════════════════════╗\n\
                    ║  `clang` not in PATH (but a versioned binary exists).    ║\n\
                    ║                                                          ║\n\
                    ║  bindgen needs the unversioned `clang` to locate its     ║\n\
                    ║  built-in headers (stdbool.h). Without it, GPU bindings  ║\n\
                    ║  will be incomplete and compilation will fail.            ║\n\
                    ║                                                          ║\n\
                    ║  Fix (pick one):                                         ║\n\
                    ║    sudo apt install clang                                 ║\n\
                    ║    export CLANG_PATH=$(which clang-20)                    ║\n\
                    ║  Or build without GPU: cargo build --release              ║\n\
                    ╚══════════════════════════════════════════════════════════╝\n",
                );
            }

            // No versioned clang either — check if headers exist on disk so we
            // can give a more specific error message.
            let headers_on_disk = std::path::Path::new("/usr/lib/clang")
                .read_dir()
                .ok()
                .and_then(|mut entries| {
                    entries
                        .any(|e| {
                            e.ok()
                                .is_some_and(|e| e.path().join("include/stdbool.h").exists())
                        })
                        .then_some(())
                })
                .is_some();

            if headers_on_disk {
                panic!(
                    "\n\n\
                    ╔══════════════════════════════════════════════════════════╗\n\
                    ║  `clang` not found in PATH.                              ║\n\
                    ║                                                          ║\n\
                    ║  libclang-dev headers exist on disk, but bindgen needs    ║\n\
                    ║  the `clang` binary to locate them at runtime.            ║\n\
                    ║                                                          ║\n\
                    ║  Fix (pick one):                                         ║\n\
                    ║    sudo apt install clang                                 ║\n\
                    ║    export CLANG_PATH=/usr/bin/clang-XX                    ║\n\
                    ║  Or build without GPU: cargo build --release              ║\n\
                    ╚══════════════════════════════════════════════════════════╝\n",
                );
            } else {
                panic!(
                    "\n\n\
                    ╔══════════════════════════════════════════════════════════╗\n\
                    ║  libclang-dev not found.                                  ║\n\
                    ║                                                          ║\n\
                    ║  bindgen needs clang and its built-in headers to generate ║\n\
                    ║  GPU FFI bindings. Without them, compilation will fail.   ║\n\
                    ║                                                          ║\n\
                    ║  Install: sudo apt install clang libclang-dev             ║\n\
                    ║  Or build without GPU: cargo build --release              ║\n\
                    ╚══════════════════════════════════════════════════════════╝\n",
                );
            }
        }
    }
}

/// Check that bindgen can find clang's resource directory at runtime.
///
/// Returns the resource dir path. Returns `None` if the unversioned
/// `clang` binary is missing.
fn find_clang_resource_dir() -> Option<String> {
    clang_resource_dir("clang")
}

/// Check if a versioned clang binary (e.g., clang-15, clang-20) is available.
fn has_versioned_clang() -> bool {
    (10..=30)
        .rev()
        .any(|v| clang_resource_dir(&format!("clang-{v}")).is_some())
}

fn clang_resource_dir(name: &str) -> Option<String> {
    let output = Command::new(name)
        .arg("-print-resource-dir")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!dir.is_empty()).then_some(dir)
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
