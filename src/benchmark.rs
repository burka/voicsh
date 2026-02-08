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
use crate::models::catalog::MODELS;
use crate::models::download::model_path;
use crate::stt::transcriber::Transcriber;
use crate::stt::whisper::{WhisperConfig, WhisperTranscriber};

/// Detect available GPU hardware.
///
/// Returns GPU name if detected (NVIDIA or AMD), or None if no GPU found.
pub fn detect_gpu() -> Option<String> {
    // Try to detect NVIDIA GPU
    let nvidia_result = std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader")
        .output();

    if let Ok(output) = nvidia_result
        && output.status.success()
        && let Ok(gpu_name) = String::from_utf8(output.stdout)
    {
        let gpu_name = gpu_name.trim();
        if !gpu_name.is_empty() {
            return Some(format!("NVIDIA {}", gpu_name));
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
                return Some(format!("AMD {}", gpu_part.trim()));
            }
        }
    }

    None
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
    pub gpu_info: Option<String>,
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

        let cpu_cores = system.physical_core_count().unwrap_or(1);
        let cpu_threads = system.cpus().len();

        let cpu_frequency = system
            .cpus()
            .first()
            .map(|cpu| cpu.frequency() as f32 / 1000.0); // Convert MHz to GHz

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
            gpu_info,
            whisper_backend,
            whisper_threads,
            available_backends,
        }
    }

    fn detect_whisper_backend() -> String {
        #[cfg(feature = "cuda")]
        {
            return "whisper.cpp (CUDA)".to_string();
        }

        #[cfg(feature = "vulkan")]
        {
            return "whisper.cpp (Vulkan)".to_string();
        }

        #[cfg(feature = "hipblas")]
        {
            return "whisper.cpp (HIP/ROCm)".to_string();
        }

        #[cfg(feature = "openblas")]
        {
            return "whisper.cpp (OpenBLAS)".to_string();
        }

        "whisper.cpp (CPU)".to_string()
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

    pub fn print_report(&self, show_backends: bool) {
        println!("\n{}", "=".repeat(120));
        println!("SYSTEM INFORMATION");
        println!("{}", "=".repeat(120));

        // CPU info
        print!(
            "CPU:       {} ({} cores, {} threads)",
            self.cpu_model, self.cpu_cores, self.cpu_threads
        );
        if let Some(freq) = self.cpu_frequency {
            println!(" @ {:.1} GHz", freq);
        } else {
            println!();
        }

        // GPU info
        match &self.gpu_info {
            Some(gpu) => println!("GPU:       {}", gpu),
            None => {
                println!("GPU:       Not detected");
            }
        }

        // Backend info
        println!(
            "Backend:   {} ({} threads)",
            self.whisper_backend, self.whisper_threads
        );

        // Show backend comparison if requested
        if show_backends {
            println!("\n{}", "=".repeat(120));
            println!("BACKEND COMPARISON");
            println!("{}", "=".repeat(120));
            println!(
                "\n{:<15} {:<12} {:<10} Compile Flags",
                "Backend", "Status", "Active"
            );
            println!("{}", "-".repeat(120));

            for backend in &self.available_backends {
                let status = if backend.available {
                    "Available"
                } else {
                    "Not compiled"
                };
                let active = if backend.active { "Yes" } else { "No" };
                println!(
                    "{:<15} {:<12} {:<10} {}",
                    backend.name, status, active, backend.compile_flags
                );
            }

            println!("\nTo benchmark with a different backend:");
            println!("1. Compile with the desired backend:");
            for backend in &self.available_backends {
                if !backend.available && backend.name != "CPU" {
                    println!("   cargo build --release {}", backend.compile_flags);
                }
            }
            println!("2. Run benchmark with the new binary");
            println!("3. Compare JSON outputs across backends");
        }

        // Recommendations
        if let Some(ref gpu) = self.gpu_info
            && self.whisper_backend.contains("CPU")
        {
            println!("\n{}", "=".repeat(120));
            println!("RECOMMENDATION");
            println!("{}", "=".repeat(120));
            if gpu.contains("NVIDIA") {
                println!("GPU detected but not in use!");
                println!("Compile with --features cuda for 10-50x speedup:");
                println!("  cargo build --release --features cuda,benchmark,model-download,cli");
            } else if gpu.contains("AMD") {
                println!("AMD GPU detected but not in use!");
                println!("Compile with --features hipblas for GPU acceleration:");
                println!("  cargo build --release --features hipblas,benchmark,model-download,cli");
            }
        }

        println!("{}", "=".repeat(120));
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

pub fn load_wav_file(path: &str) -> Result<(Vec<i16>, u64), Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let source = WavAudioSource::from_reader(Box::new(reader))?;

    let samples = source.into_samples();
    let duration_ms = (samples.len() as u64 * 1000) / 16000;

    Ok((samples, duration_ms))
}

pub fn benchmark_model(
    model_name: &str,
    language: &str,
    audio: &[i16],
    audio_duration_ms: u64,
    monitor: &ResourceMonitor,
    iterations: usize,
) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
    println!("Loading model: {} (language: {})", model_name, language);

    let path = model_path(model_name)
        .ok_or_else(|| format!("Model path not found for: {}", model_name))?;

    let config = WhisperConfig {
        model_path: path,
        language: language.to_string(),
        threads: None,
    };

    let transcriber = WhisperTranscriber::new(config)?;

    println!("Running {} iteration(s)...", iterations);

    let mut total_elapsed_ms = 0u128;
    let mut total_cpu_total = 0.0f32;
    let mut total_cpu_per_core = 0.0f32;
    let mut total_memory = 0.0f64;
    let mut last_result = None;

    for i in 0..iterations {
        print!("  Iteration {}/{}... ", i + 1, iterations);

        let start = Instant::now();
        let result = transcriber.transcribe(audio)?;
        let elapsed = start.elapsed();

        let (cpu_total, cpu_per_core, memory) = monitor.get_current_stats();

        total_elapsed_ms += elapsed.as_millis();
        total_cpu_total += cpu_total;
        total_cpu_per_core += cpu_per_core;
        total_memory += memory;
        last_result = Some(result);

        println!("{}ms", elapsed.as_millis());
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

    let model_info = MODELS
        .iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| format!("Model not found in catalog: {}", model_name))?;

    let result = last_result.ok_or("No transcription result after iterations")?;
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
        model_size_mb: model_info.size_mb as u64,
    })
}

pub fn print_results(results: &[BenchmarkResult], compare_languages: bool) {
    println!("\n{}", "=".repeat(120));
    println!("BENCHMARK RESULTS");
    println!("{}", "=".repeat(120));

    if compare_languages {
        // Group by model and show language comparisons
        let mut models: Vec<String> = results
            .iter()
            .map(|r| r.model_name.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        models.sort();

        for model in &models {
            let model_results: Vec<&BenchmarkResult> =
                results.iter().filter(|r| r.model_name == *model).collect();

            if model_results.is_empty() {
                continue;
            }

            println!("\nModel: {}", model);
            println!(
                "{:<12} {:<10} {:>10} {:>8} {:>10} {:>10} {:>10}",
                "Backend", "Language", "Time (ms)", "RTF", "Speed", "CPU%", "Memory"
            );
            println!("{}", "-".repeat(120));

            for result in model_results {
                println!(
                    "{:<12} {:<10} {:>10} {:>8.3} {:>10.2}x {:>10.1} {:>10.1}",
                    result.backend,
                    result.language,
                    result.elapsed_ms,
                    result.realtime_factor,
                    result.speed_multiplier,
                    result.cpu_usage_per_core,
                    result.memory_usage_mb
                );
            }
        }
    } else {
        // Standard output
        println!(
            "\n{:<12} {:<10} {:<10} {:>10} {:>8} {:>10} {:>10} {:>10}",
            "Model", "Backend", "Language", "Time (ms)", "RTF", "Speed", "CPU%", "Mem (MB)"
        );
        println!("{}", "-".repeat(120));

        for result in results {
            println!(
                "{:<12} {:<10} {:<10} {:>10} {:>8.3} {:>10.2}x {:>10.1} {:>10.1}",
                result.model_name,
                result.backend,
                result.language,
                result.elapsed_ms,
                result.realtime_factor,
                result.speed_multiplier,
                result.cpu_usage_per_core,
                result.memory_usage_mb
            );
        }
    }

    println!("\n{}", "=".repeat(120));
    println!("SUMMARY");
    println!("{}", "=".repeat(120));

    if let Some(fastest) = results.iter().min_by_key(|r| r.elapsed_ms) {
        println!(
            "Fastest:        {} ({} / {}) - {}ms, {:.3}x RTF, {:.2}x speed",
            fastest.model_name,
            fastest.backend,
            fastest.language,
            fastest.elapsed_ms,
            fastest.realtime_factor,
            fastest.speed_multiplier
        );
    }

    if let Some(most_efficient) = results.iter().min_by(|a, b| {
        a.realtime_factor
            .partial_cmp(&b.realtime_factor)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) {
        println!(
            "Most Efficient: {} ({} / {}) - {:.3}x RTF, {:.2}x speed",
            most_efficient.model_name,
            most_efficient.backend,
            most_efficient.language,
            most_efficient.realtime_factor,
            most_efficient.speed_multiplier
        );
    }

    if let Some(lowest_cpu) = results.iter().min_by(|a, b| {
        a.cpu_usage_per_core
            .partial_cmp(&b.cpu_usage_per_core)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) {
        println!(
            "Lowest CPU:     {} ({:.1}% per core, {:.1}% total)",
            lowest_cpu.model_name, lowest_cpu.cpu_usage_per_core, lowest_cpu.cpu_usage_total
        );
    }

    if let Some(lowest_mem) = results.iter().min_by(|a, b| {
        a.memory_usage_mb
            .partial_cmp(&b.memory_usage_mb)
            .unwrap_or(std::cmp::Ordering::Equal)
    }) {
        println!(
            "Lowest Memory:  {} ({:.1}MB)",
            lowest_mem.model_name, lowest_mem.memory_usage_mb
        );
    }

    println!("\nTranscriptions:");
    for result in results {
        println!(
            "  {} ({} / {}): \"{}\" [detected: {}, confidence: {:.2}]",
            result.model_name,
            result.backend,
            result.language,
            result.transcription.trim(),
            result.detected_language,
            result.confidence
        );
    }
}

pub fn print_json_report(report: &BenchmarkReport) {
    match serde_json::to_string_pretty(report) {
        Ok(json) => println!("{}", json),
        Err(e) => eprintln!("Error serializing to JSON: {}", e),
    }
}
