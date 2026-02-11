//! Benchmarking utilities for transcription performance testing.
//!
//! Provides system information collection, benchmark execution, and reporting.

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use sysinfo::{Pid, System};

use crate::audio::wav::WavAudioSource;
use crate::defaults;
use crate::models::download::{list_installed_models, model_path};
use crate::stt::transcriber::Transcriber;
use crate::stt::whisper::{WhisperConfig, WhisperTranscriber};

/// Compute default thread count for whisper inference.
///
/// Uses most available threads while leaving headroom for the OS:
/// `min(cpu_threads, max(4, floor((cpu_threads - 1) * 0.9)))`
///
/// This avoids whisper.cpp's conservative default of `min(4, n_processors)`.
pub fn compute_default_threads(cpu_threads: usize) -> usize {
    let headroom = ((cpu_threads.saturating_sub(1) as f64) * 0.9).floor() as usize;
    cpu_threads.min(headroom.max(4))
}

/// GPU hardware information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vram_mb: Option<u64>,
}

impl GpuInfo {
    /// Format as display string, e.g. "NVIDIA GeForce RTX 5060 Ti (16 GB)".
    pub fn display(&self) -> String {
        match self.vram_mb {
            Some(mb) => format!("{} ({} GB)", self.name, mb / 1024),
            None => self.name.clone(),
        }
    }
}

/// Detect available GPU hardware.
///
/// Returns GPU name and VRAM if detected (NVIDIA or AMD), or None if no GPU found.
pub fn detect_gpu() -> Option<GpuInfo> {
    // Try to detect NVIDIA GPU with VRAM
    let nvidia_result = std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name,memory.total")
        .arg("--format=csv,noheader")
        .output();

    if let Ok(output) = nvidia_result
        && output.status.success()
        && let Ok(gpu_line) = String::from_utf8(output.stdout)
    {
        let gpu_line = gpu_line.trim();
        if !gpu_line.is_empty() {
            return Some(parse_nvidia_gpu(gpu_line));
        }
    }

    // Try to detect AMD GPU via lspci
    let lspci_result = std::process::Command::new("lspci").output();
    if let Ok(output) = lspci_result
        && output.status.success()
        && let Ok(lspci_output) = String::from_utf8(output.stdout)
    {
        for line in lspci_output.lines() {
            let is_gpu = line.contains("VGA") || line.contains("3D");
            let is_amd = line.contains("AMD") || line.contains("Radeon");

            if is_gpu
                && is_amd
                && let Some(gpu_part) = line.split(':').nth(2)
            {
                return Some(GpuInfo {
                    name: format!("AMD {}", gpu_part.trim()),
                    vram_mb: None,
                });
            }
        }
    }

    None
}

/// Parse nvidia-smi CSV output like "GeForce RTX 5060 Ti, 16384 MiB".
fn parse_nvidia_gpu(line: &str) -> GpuInfo {
    let parts: Vec<&str> = line.splitn(2, ',').collect();
    let name = format!("NVIDIA {}", parts[0].trim());

    let vram_mb = parts.get(1).and_then(|mem_str| {
        let mem_str = mem_str.trim();
        // nvidia-smi reports "16384 MiB" — parse the number
        mem_str
            .split_whitespace()
            .next()
            .and_then(|n| n.parse::<u64>().ok())
    });

    GpuInfo { name, vram_mb }
}

/// Backend availability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendInfo {
    pub name: String,
    pub available: bool,
    pub active: bool,
    pub compile_flags: String,
}

/// System information for benchmark reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub cpu_model: String,
    pub cpu_cores: usize,
    pub cpu_threads: usize,
    pub cpu_frequency: Option<f32>,
    pub total_memory_mb: u64,
    pub available_memory_mb: u64,
    pub gpu_info: Option<GpuInfo>,
    pub whisper_backend: String,
    pub whisper_threads: usize,
    pub available_backends: Vec<BackendInfo>,
}

impl SystemInfo {
    pub fn detect() -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        // CPU information
        let cpu_model = system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().trim().to_string())
            .unwrap_or_else(|| "Unknown CPU".to_string());

        let cpu_cores = System::physical_core_count().unwrap_or(1);
        let cpu_threads = system.cpus().len();

        let cpu_frequency = system
            .cpus()
            .first()
            .map(|cpu| cpu.frequency() as f32 / 1000.0); // Convert MHz to GHz

        let total_memory_mb = system.total_memory() / (1024 * 1024);
        let available_memory_mb = system.available_memory() / (1024 * 1024);

        // GPU detection - check for NVIDIA/AMD/Intel
        let gpu_info = detect_gpu();

        // Whisper backend - determined by compile-time features
        let whisper_backend = Self::detect_whisper_backend();

        // Thread count for whisper - None means auto-detect (use all cores)
        let whisper_threads = cpu_threads;

        // Detect available backends
        let available_backends = Self::detect_available_backends();

        Self {
            cpu_model,
            cpu_cores,
            cpu_threads,
            cpu_frequency,
            total_memory_mb,
            available_memory_mb,
            gpu_info,
            whisper_backend,
            whisper_threads,
            available_backends,
        }
    }

    fn detect_whisper_backend() -> String {
        if cfg!(feature = "cuda") {
            "whisper.cpp (CUDA)"
        } else if cfg!(feature = "vulkan") {
            "whisper.cpp (Vulkan)"
        } else if cfg!(feature = "hipblas") {
            "whisper.cpp (HIP/ROCm)"
        } else if cfg!(feature = "openblas") {
            "whisper.cpp (OpenBLAS)"
        } else {
            "whisper.cpp (CPU)"
        }
        .to_string()
    }

    fn detect_available_backends() -> Vec<BackendInfo> {
        vec![
            // CPU backend (always available)
            BackendInfo {
                name: "CPU".to_string(),
                available: true,
                active: cfg!(not(any(
                    feature = "cuda",
                    feature = "vulkan",
                    feature = "hipblas",
                    feature = "openblas"
                ))),
                compile_flags:
                    "--no-default-features --features whisper,benchmark,model-download,cli"
                        .to_string(),
            },
            // CUDA backend
            BackendInfo {
                name: "CUDA".to_string(),
                available: cfg!(feature = "cuda"),
                active: cfg!(feature = "cuda"),
                compile_flags: "--features cuda,benchmark,model-download,cli".to_string(),
            },
            // Vulkan backend
            BackendInfo {
                name: "Vulkan".to_string(),
                available: cfg!(feature = "vulkan"),
                active: cfg!(feature = "vulkan"),
                compile_flags: "--features vulkan,benchmark,model-download,cli".to_string(),
            },
            // HipBLAS backend
            BackendInfo {
                name: "HipBLAS".to_string(),
                available: cfg!(feature = "hipblas"),
                active: cfg!(feature = "hipblas"),
                compile_flags: "--features hipblas,benchmark,model-download,cli".to_string(),
            },
            // OpenBLAS backend
            BackendInfo {
                name: "OpenBLAS".to_string(),
                available: cfg!(feature = "openblas"),
                active: cfg!(feature = "openblas"),
                compile_flags: "--features openblas,benchmark,model-download,cli".to_string(),
            },
        ]
    }

    pub fn print_report(&self, _verbose: u8) {
        // Compact CPU info
        let cpu_info = format!(
            "{} ({}c/{}t)",
            self.cpu_model, self.cpu_cores, self.cpu_threads
        );

        // RAM info (approximate from total memory, but we don't have it directly)
        // For now, we'll skip RAM in the system line, or we could add it to SystemInfo

        // GPU info
        let gpu_info = if let Some(ref gpu) = self.gpu_info {
            if self.whisper_backend.contains("CPU") {
                format!("{} (not in use)", gpu.display())
            } else {
                gpu.display()
            }
        } else {
            "None".to_string()
        };

        // Backend info — show "17/20 threads" when not using all available
        let thread_info = if self.whisper_threads < self.cpu_threads {
            format!("{}/{} threads", self.whisper_threads, self.cpu_threads)
        } else {
            format!("{} threads", self.whisper_threads)
        };
        let backend_info = format!("{} ({})", self.whisper_backend, thread_info);

        // Print compact system info on one line
        println!(
            "System: {} | GPU: {} | Backend: {}",
            cpu_info, gpu_info, backend_info
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub model_name: String,
    pub language: String,
    pub backend: String,
    pub transcription: String,
    pub detected_language: String,
    pub confidence: f32,
    pub elapsed_ms: u128,
    pub audio_duration_ms: u64,
    pub realtime_factor: f64,
    pub speed_multiplier: f64,
    pub cpu_usage_total: f32,
    pub cpu_usage_per_core: f32,
    pub memory_usage_mb: f64,
    pub model_size_mb: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub system_info: SystemInfo,
    pub audio_file: String,
    pub audio_samples: usize,
    pub audio_duration_ms: u64,
    pub iterations: usize,
    pub results: Vec<BenchmarkResult>,
}

pub struct ResourceMonitor {
    system: Arc<Mutex<System>>,
    pid: Pid,
    num_cpus: usize,
}

impl ResourceMonitor {
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        // Safe: This is a benchmark tool; if we can't get PID, we can't benchmark
        let pid = sysinfo::get_current_pid().unwrap_or_else(|e| {
            eprintln!("Failed to get current process ID: {}", e);
            std::process::exit(1);
        });
        let num_cpus = system.cpus().len();

        Self {
            system: Arc::new(Mutex::new(system)),
            pid,
            num_cpus,
        }
    }

    pub fn get_current_stats(&self) -> (f32, f32, f64) {
        // Safe: Mutex poisoning only happens if thread panics while holding lock
        // which doesn't happen in normal operation
        if let Ok(mut system) = self.system.lock() {
            system.refresh_all();
        }

        let system = self.system.lock().unwrap_or_else(|e| e.into_inner());

        // Safe: Process must exist since we got its PID in new()
        let process = system.process(self.pid).unwrap_or_else(|| {
            eprintln!("Failed to get process info for PID {:?}", self.pid);
            std::process::exit(1);
        });

        let cpu_usage_total = process.cpu_usage();
        let cpu_usage_per_core = cpu_usage_total / self.num_cpus as f32;
        let memory_usage = process.memory() as f64 / (1024.0 * 1024.0);

        (cpu_usage_total, cpu_usage_per_core, memory_usage)
    }
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

pub fn load_wav_file(path: &str) -> anyhow::Result<(Vec<i16>, u64)> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let source = WavAudioSource::from_reader(Box::new(reader))?;

    let samples = source.into_samples();
    let duration_ms = (samples.len() as u64 * 1000) / 16000;

    Ok((samples, duration_ms))
}

#[allow(clippy::too_many_arguments)]
pub fn benchmark_model(
    model_name: &str,
    language: &str,
    audio: &[i16],
    audio_duration_ms: u64,
    monitor: &ResourceMonitor,
    iterations: usize,
    verbose: u8,
    threads: usize,
) -> anyhow::Result<BenchmarkResult> {
    let path = model_path(model_name);

    let config = WhisperConfig {
        model_path: path,
        language: language.to_string(),
        threads: Some(threads),
        use_gpu: true,
    };

    let transcriber = WhisperTranscriber::new(config)?;

    let mut total_elapsed_ms = 0u128;
    let mut total_cpu_total = 0.0f32;
    let mut total_cpu_per_core = 0.0f32;
    let mut total_memory = 0.0f64;
    let mut last_result = None;

    for i in 0..iterations {
        if verbose >= 1 && iterations > 1 {
            print!("  Iteration {}/{}... ", i + 1, iterations);
        }

        let start = Instant::now();
        let result = transcriber.transcribe(audio)?;
        let elapsed = start.elapsed();

        let (cpu_total, cpu_per_core, memory) = monitor.get_current_stats();

        total_elapsed_ms += elapsed.as_millis();
        total_cpu_total += cpu_total;
        total_cpu_per_core += cpu_per_core;
        total_memory += memory;
        last_result = Some(result);

        if verbose >= 1 && iterations > 1 {
            println!("{}ms", elapsed.as_millis());
        }
    }

    let avg_elapsed_ms = total_elapsed_ms / iterations as u128;
    let avg_cpu_total = total_cpu_total / iterations as f32;
    let avg_cpu_per_core = total_cpu_per_core / iterations as f32;
    let avg_memory = total_memory / iterations as f64;

    let realtime_factor = if audio_duration_ms > 0 {
        avg_elapsed_ms as f64 / audio_duration_ms as f64
    } else {
        0.0
    };

    let speed_multiplier = if realtime_factor > 0.0 {
        1.0 / realtime_factor
    } else {
        0.0
    };

    // Get model size: try catalog first, fall back to file size on disk
    let model_size_mb = crate::models::catalog::get_model(model_name)
        .map(|m| m.size_mb as u64)
        .unwrap_or_else(|| {
            let path = model_path(model_name);
            path.metadata()
                .map(|m| m.len() / (1024 * 1024))
                .unwrap_or(0)
        });

    let result =
        last_result.ok_or_else(|| anyhow::anyhow!("No transcription result after iterations"))?;
    let backend = defaults::gpu_backend().to_string();

    Ok(BenchmarkResult {
        model_name: model_name.to_string(),
        language: language.to_string(),
        backend,
        transcription: result.text,
        detected_language: result.language,
        confidence: result.confidence,
        elapsed_ms: avg_elapsed_ms,
        audio_duration_ms,
        realtime_factor,
        speed_multiplier,
        cpu_usage_total: avg_cpu_total,
        cpu_usage_per_core: avg_cpu_per_core,
        memory_usage_mb: avg_memory,
        model_size_mb,
    })
}

pub fn print_results(results: &[BenchmarkResult], verbose: u8) {
    println!();

    // Standard output - simplified table
    println!(
        "{:<12} {:>11} {:>8} {:>10} {:>11} {:>10}",
        "Model", "Time (ms)", "RTF", "Speed", "CPU/Core %", "Mem (MB)"
    );
    println!("{}", "-".repeat(64));

    for result in results {
        println!(
            "{:<12} {:>11} {:>8.2} {:>9.2}x {:>11.1} {:>10.1}",
            result.model_name,
            result.elapsed_ms,
            result.realtime_factor,
            result.speed_multiplier,
            result.cpu_usage_per_core,
            result.memory_usage_mb
        );
    }

    // Show transcriptions only with verbose >= 1
    if verbose >= 1 {
        println!("\nTranscriptions:");
        for result in results {
            println!(
                "  {}: \"{}\" [detected: {}, confidence: {:.2}]",
                result.model_name,
                result.transcription.trim(),
                result.detected_language,
                result.confidence
            );
        }
    }
}

/// Print guidance based on benchmark results and system information.
///
/// Shows the fastest model, the best quality model that still runs at real-time,
/// and GPU acceleration suggestions if a GPU is detected but not in use.
pub fn print_guidance(results: &[BenchmarkResult], system_info: &SystemInfo) {
    if results.is_empty() {
        return;
    }

    println!("\nGuidance:");

    // Fastest model
    if let Some(fastest) = results.iter().min_by_key(|r| r.elapsed_ms) {
        println!(
            "  Fastest: {} ({:.1}x real-time)",
            fastest.model_name, fastest.speed_multiplier
        );
    }

    // Best quality model that still runs faster than real-time (RTF < 1.0)
    // Quality correlates with model size — larger models are more accurate.
    let realtime_capable: Vec<_> = results.iter().filter(|r| r.realtime_factor < 1.0).collect();

    if let Some(best_quality) = realtime_capable.iter().max_by_key(|r| r.model_size_mb) {
        let fastest = results.iter().min_by_key(|r| r.elapsed_ms);
        if fastest.is_none_or(|f| f.model_name != best_quality.model_name) {
            println!(
                "  Best quality at real-time: {} ({:.1}x real-time)",
                best_quality.model_name, best_quality.speed_multiplier
            );
        }
    } else {
        println!("  No model runs faster than real-time on this hardware.");
        println!("  Try --buffer 5m to tolerate slow transcription, or enable GPU acceleration.");
    }

    // Thread recommendation if not using all available threads
    let default_threads = compute_default_threads(system_info.cpu_threads);
    if system_info.whisper_threads < default_threads {
        println!(
            "  Tip: using {} of {} threads. Try --threads {} for better throughput.",
            system_info.whisper_threads, system_info.cpu_threads, default_threads,
        );
    }

    // GPU recommendation if detected but not in use
    if let Some(ref gpu) = system_info.gpu_info
        && system_info.whisper_backend.contains("CPU")
    {
        let display = gpu.display();
        if gpu.name.contains("NVIDIA") {
            println!("\n  GPU detected ({}) but not in use.", display);
            println!("  Compile with CUDA for 10-50x speedup:");
            println!("    cargo build --release --features cuda");
        } else if gpu.name.contains("AMD") {
            println!("\n  GPU detected ({}) but not in use.", display);
            println!("  Compile with HipBLAS for GPU acceleration:");
            println!("    cargo build --release --features hipblas");
        }
    }

    // Suggest more models when few are installed
    if list_installed_models().len() <= 2 {
        println!("\n  Run 'voicsh models list' to discover more models to compare.");
    }
}

pub fn print_json_report(report: &BenchmarkReport) {
    match serde_json::to_string_pretty(report) {
        Ok(json) => println!("{}", json),
        Err(e) => eprintln!("Error serializing to JSON: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_default_threads_single_core() {
        // min(1, max(4, 0*0.9)) = min(1, 4) = 1
        assert_eq!(compute_default_threads(1), 1);
    }

    #[test]
    fn compute_default_threads_two_cores() {
        // min(2, max(4, 1*0.9)) = min(2, 4) = 2
        assert_eq!(compute_default_threads(2), 2);
    }

    #[test]
    fn compute_default_threads_four_cores() {
        // min(4, max(4, 3*0.9=2)) = min(4, 4) = 4
        assert_eq!(compute_default_threads(4), 4);
    }

    #[test]
    fn compute_default_threads_eight_cores() {
        // min(8, max(4, 7*0.9=6)) = min(8, 6) = 6
        assert_eq!(compute_default_threads(8), 6);
    }

    #[test]
    fn compute_default_threads_twenty_cores() {
        // min(20, max(4, 19*0.9=17)) = min(20, 17) = 17
        assert_eq!(compute_default_threads(20), 17);
    }

    #[test]
    fn compute_default_threads_zero() {
        // Edge case: 0 threads (shouldn't happen, but be safe)
        // min(0, max(4, 0*0.9=0)) = min(0, 4) = 0
        assert_eq!(compute_default_threads(0), 0);
    }

    #[test]
    fn parse_nvidia_gpu_with_vram() {
        let info = parse_nvidia_gpu("GeForce RTX 5060 Ti, 16384 MiB");
        assert_eq!(info.name, "NVIDIA GeForce RTX 5060 Ti");
        assert_eq!(info.vram_mb, Some(16384));
    }

    #[test]
    fn parse_nvidia_gpu_name_only() {
        let info = parse_nvidia_gpu("GeForce RTX 4090");
        assert_eq!(info.name, "NVIDIA GeForce RTX 4090");
        assert_eq!(info.vram_mb, None);
    }

    #[test]
    fn gpu_info_display_with_vram() {
        let info = GpuInfo {
            name: "NVIDIA GeForce RTX 5060 Ti".to_string(),
            vram_mb: Some(16384),
        };
        assert_eq!(info.display(), "NVIDIA GeForce RTX 5060 Ti (16 GB)");
    }

    #[test]
    fn gpu_info_display_without_vram() {
        let info = GpuInfo {
            name: "AMD Radeon RX 7900 XTX".to_string(),
            vram_mb: None,
        };
        assert_eq!(info.display(), "AMD Radeon RX 7900 XTX");
    }
}
