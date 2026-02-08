use serde::{Deserialize, Serialize};
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::process;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use sysinfo::{Pid, System};
use voicsh::audio::wav::WavAudioSource;
use voicsh::models::catalog::MODELS;
use voicsh::models::download::model_path;
use voicsh::stt::transcriber::Transcriber;
use voicsh::stt::whisper::{WhisperConfig, WhisperTranscriber};

/// System information for benchmark reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SystemInfo {
    cpu_model: String,
    cpu_cores: usize,
    cpu_threads: usize,
    cpu_frequency: Option<f32>,
    gpu_info: Option<String>,
    whisper_backend: String,
    whisper_threads: usize,
}

impl SystemInfo {
    fn detect() -> Self {
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
        let gpu_info = Self::detect_gpu();

        // Whisper backend - determined by compile-time features
        let whisper_backend = Self::detect_whisper_backend();

        // Thread count for whisper - None means auto-detect (use all cores)
        let whisper_threads = cpu_threads;

        Self {
            cpu_model,
            cpu_cores,
            cpu_threads,
            cpu_frequency,
            gpu_info,
            whisper_backend,
            whisper_threads,
        }
    }

    fn detect_gpu() -> Option<String> {
        // Try to detect NVIDIA GPU
        if let Ok(output) = std::process::Command::new("nvidia-smi")
            .arg("--query-gpu=name")
            .arg("--format=csv,noheader")
            .output()
        {
            if output.status.success() {
                if let Ok(gpu_name) = String::from_utf8(output.stdout) {
                    let gpu_name = gpu_name.trim();
                    if !gpu_name.is_empty() {
                        return Some(format!("NVIDIA {}", gpu_name));
                    }
                }
            }
        }

        // Try to detect AMD GPU via lspci
        if let Ok(output) = std::process::Command::new("lspci").output() {
            if output.status.success() {
                if let Ok(lspci_output) = String::from_utf8(output.stdout) {
                    for line in lspci_output.lines() {
                        if line.contains("VGA") || line.contains("3D") {
                            if line.contains("AMD") || line.contains("Radeon") {
                                // Extract GPU name
                                if let Some(gpu_part) = line.split(':').nth(2) {
                                    return Some(format!("AMD {}", gpu_part.trim()));
                                }
                            }
                        }
                    }
                }
            }
        }

        None
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

    fn print_report(&self) {
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

        // Recommendations
        if self.gpu_info.is_some() && self.whisper_backend.contains("CPU") {
            if self.gpu_info.as_ref().unwrap().contains("NVIDIA") {
                println!("\n⚡ GPU detected but not in use!");
                println!("   Compile with --features cuda for 10-50x speedup:");
                println!(
                    "   cargo build --release --features whisper,cuda,benchmark,model-download,cli"
                );
            } else if self.gpu_info.as_ref().unwrap().contains("AMD") {
                println!("\n⚡ AMD GPU detected but not in use!");
                println!("   Compile with --features hipblas for GPU acceleration:");
                println!(
                    "   cargo build --release --features whisper,hipblas,benchmark,model-download,cli"
                );
            }
        }

        println!("{}", "=".repeat(120));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchmarkResult {
    model_name: String,
    transcription: String,
    confidence: f32,
    elapsed_ms: u128,
    audio_duration_ms: u64,
    realtime_factor: f64,
    cpu_usage_total: f32,
    cpu_usage_per_core: f32,
    memory_usage_mb: f64,
    model_size_mb: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct BenchmarkReport {
    system_info: SystemInfo,
    audio_file: String,
    audio_samples: usize,
    audio_duration_ms: u64,
    iterations: usize,
    results: Vec<BenchmarkResult>,
}

struct ResourceMonitor {
    system: Arc<Mutex<System>>,
    pid: Pid,
    num_cpus: usize,
}

impl ResourceMonitor {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        let pid = sysinfo::get_current_pid().expect("Failed to get current PID");
        let num_cpus = system.cpus().len();

        Self {
            system: Arc::new(Mutex::new(system)),
            pid,
            num_cpus,
        }
    }

    fn get_current_stats(&self) -> (f32, f32, f64) {
        self.system.lock().unwrap().refresh_all();

        let system = self.system.lock().unwrap();
        let process = system
            .process(self.pid)
            .expect("Failed to get process info");

        let cpu_usage_total = process.cpu_usage();
        let cpu_usage_per_core = cpu_usage_total / self.num_cpus as f32;
        let memory_usage = process.memory() as f64 / (1024.0 * 1024.0);

        (cpu_usage_total, cpu_usage_per_core, memory_usage)
    }
}

fn load_wav_file(path: &str) -> Result<(Vec<i16>, u64), Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let source = WavAudioSource::from_reader(Box::new(reader))?;

    let samples = source.into_samples();
    let duration_ms = (samples.len() as u64 * 1000) / 16000;

    Ok((samples, duration_ms))
}

fn benchmark_model(
    model_name: &str,
    audio: &[i16],
    audio_duration_ms: u64,
    monitor: &ResourceMonitor,
    iterations: usize,
) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
    println!("Loading model: {}", model_name);

    let config = WhisperConfig {
        model_path: model_path(model_name).expect("Model path not found"),
        language: "auto".to_string(),
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

    let model_info = MODELS
        .iter()
        .find(|m| m.name == model_name)
        .expect("Model not found in catalog");

    let result = last_result.expect("No transcription result");

    Ok(BenchmarkResult {
        model_name: model_name.to_string(),
        transcription: result.text,
        confidence: result.confidence,
        elapsed_ms: avg_elapsed_ms,
        audio_duration_ms,
        realtime_factor,
        cpu_usage_total: avg_cpu_total,
        cpu_usage_per_core: avg_cpu_per_core,
        memory_usage_mb: avg_memory,
        model_size_mb: model_info.size_mb as u64,
    })
}

fn print_results(results: &[BenchmarkResult]) {
    println!("\n{}", "=".repeat(120));
    println!("BENCHMARK RESULTS");
    println!("{}", "=".repeat(120));

    println!(
        "\n{:<12} {:<35} {:>10} {:>8} {:>10} {:>12} {:>10}",
        "Model", "Transcription", "Time (ms)", "RTF", "Speed", "CPU/Core %", "Mem (MB)"
    );
    println!("{}", "-".repeat(120));

    for result in results {
        let speed = if result.realtime_factor > 0.0 {
            format!("{:.2}x", 1.0 / result.realtime_factor)
        } else {
            "N/A".to_string()
        };

        let transcription = if result.transcription.len() > 32 {
            format!("{}...", &result.transcription[..32])
        } else {
            result.transcription.clone()
        };

        println!(
            "{:<12} {:<35} {:>10} {:>8.2} {:>10} {:>12.1} {:>10.1}",
            result.model_name,
            transcription,
            result.elapsed_ms,
            result.realtime_factor,
            speed,
            result.cpu_usage_per_core,
            result.memory_usage_mb
        );
    }

    println!("\n{}", "=".repeat(120));
    println!("SUMMARY");
    println!("{}", "=".repeat(120));

    if let Some(fastest) = results.iter().min_by_key(|r| r.elapsed_ms) {
        println!(
            "Fastest:        {} ({}ms, {:.2}x realtime, {:.2}x speed)",
            fastest.model_name,
            fastest.elapsed_ms,
            fastest.realtime_factor,
            1.0 / fastest.realtime_factor
        );
    }

    if let Some(most_efficient) = results
        .iter()
        .min_by(|a, b| a.realtime_factor.partial_cmp(&b.realtime_factor).unwrap())
    {
        println!(
            "Most Efficient: {} ({:.2}x realtime factor)",
            most_efficient.model_name, most_efficient.realtime_factor
        );
    }

    if let Some(lowest_cpu) = results.iter().min_by(|a, b| {
        a.cpu_usage_per_core
            .partial_cmp(&b.cpu_usage_per_core)
            .unwrap()
    }) {
        println!(
            "Lowest CPU:     {} ({:.1}% per core, {:.1}% total)",
            lowest_cpu.model_name, lowest_cpu.cpu_usage_per_core, lowest_cpu.cpu_usage_total
        );
    }

    if let Some(lowest_mem) = results
        .iter()
        .min_by(|a, b| a.memory_usage_mb.partial_cmp(&b.memory_usage_mb).unwrap())
    {
        println!(
            "Lowest Memory:  {} ({:.1}MB)",
            lowest_mem.model_name, lowest_mem.memory_usage_mb
        );
    }

    println!("\nModel Sizes:");
    for result in results {
        println!("  {:<12} {}MB", result.model_name, result.model_size_mb);
    }
}

fn print_json_report(report: &BenchmarkReport) {
    match serde_json::to_string_pretty(report) {
        Ok(json) => println!("{}", json),
        Err(e) => eprintln!("Error serializing to JSON: {}", e),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse command line arguments
    let (wav_file, models, iterations, output_format) = if args.len() < 2 {
        eprintln!(
            "Usage: {} <wav-file> [model1,model2,...] [iterations] [--output json]",
            args[0]
        );
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} tests/fixtures/quick_brown_fox.wav", args[0]);
        eprintln!("  {} audio.wav tiny.en,base.en,small.en", args[0]);
        eprintln!("  {} audio.wav all 3", args[0]);
        eprintln!("  {} audio.wav all 1 --output json", args[0]);
        eprintln!();
        eprintln!("Available models:");
        for model in MODELS.iter() {
            eprintln!("  {} ({}MB)", model.name, model.size_mb);
        }
        process::exit(1);
    } else {
        let wav_file = &args[1];
        let models: Vec<String> =
            if args.len() > 2 && args[2] != "all" && !args[2].starts_with("--") {
                args[2].split(',').map(|s| s.to_string()).collect()
            } else if args.get(2).map(|s| s.starts_with("--")).unwrap_or(false) {
                MODELS.iter().map(|m| m.name.to_string()).collect()
            } else {
                MODELS.iter().map(|m| m.name.to_string()).collect()
            };

        let iterations = if args.len() > 3 && !args[3].starts_with("--") {
            args[3].parse().unwrap_or(1)
        } else {
            1
        };

        // Check for --output json flag
        let output_format = if args.iter().any(|arg| arg == "--output") {
            if let Some(pos) = args.iter().position(|arg| arg == "--output") {
                args.get(pos + 1).map(|s| s.as_str()).unwrap_or("table")
            } else {
                "table"
            }
        } else {
            "table"
        };

        (
            wav_file.to_string(),
            models,
            iterations,
            output_format.to_string(),
        )
    };

    // Print system information
    let system_info = SystemInfo::detect();
    system_info.print_report();

    println!("\nWAV Transcription Benchmark");
    println!("{}", "=".repeat(120));

    let (audio, audio_duration_ms) = match load_wav_file(&wav_file) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to load WAV file: {}", e);
            process::exit(1);
        }
    };

    println!("Audio file:     {}", wav_file);
    println!("Samples:        {}", audio.len());
    println!(
        "Duration:       {}ms ({:.2}s)",
        audio_duration_ms,
        audio_duration_ms as f64 / 1000.0
    );
    println!("Sample rate:    16000 Hz");
    println!("Iterations:     {}", iterations);
    println!("{}", "=".repeat(120));
    println!();

    let monitor = ResourceMonitor::new();
    let mut results = Vec::new();

    for model_name in &models {
        if !model_path(model_name).map_or(false, |p| p.exists()) {
            eprintln!("Skipping {}: model not installed", model_name);
            eprintln!(
                "  Install with: cargo run --features model-download --release -- download {}",
                model_name
            );
            println!();
            continue;
        }

        match benchmark_model(model_name, &audio, audio_duration_ms, &monitor, iterations) {
            Ok(result) => {
                println!("Result: \"{}\"", result.transcription.trim());
                println!("Confidence: {:.2}", result.confidence);
                println!();
                results.push(result);
            }
            Err(e) => {
                eprintln!("Failed to benchmark {}: {}", model_name, e);
                println!();
            }
        }
    }

    if results.is_empty() {
        eprintln!("No models were benchmarked successfully.");
        eprintln!();
        eprintln!("Install models with:");
        eprintln!("  cargo run --features model-download --release -- download <model-name>");
        process::exit(1);
    }

    // Output results in requested format
    if output_format == "json" {
        let report = BenchmarkReport {
            system_info,
            audio_file: wav_file,
            audio_samples: audio.len(),
            audio_duration_ms,
            iterations,
            results,
        };
        print_json_report(&report);
    } else {
        print_results(&results);
    }
}
