use std::env;
use std::fs::File;
use std::io::BufReader;
use std::process;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use sysinfo::{Pid, ProcessRefreshKind, System};
use voicsh::audio::wav::WavAudioSource;
use voicsh::models::catalog::MODELS;
use voicsh::models::download::model_path;
use voicsh::stt::transcriber::Transcriber;
use voicsh::stt::whisper::{WhisperConfig, WhisperTranscriber};

#[derive(Debug, Clone)]
struct BenchmarkResult {
    model_name: String,
    transcription: String,
    confidence: f32,
    elapsed_ms: u128,
    audio_duration_ms: u64,
    realtime_factor: f64,
    cpu_usage_percent: f32,
    memory_usage_mb: f64,
    model_size_mb: u64,
}

struct ResourceMonitor {
    system: Arc<Mutex<System>>,
    pid: Pid,
}

impl ResourceMonitor {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        let pid = sysinfo::get_current_pid().expect("Failed to get current PID");

        Self {
            system: Arc::new(Mutex::new(system)),
            pid,
        }
    }

    fn get_current_stats(&self) -> (f32, f64) {
        self.system.lock().unwrap().refresh_all();

        let system = self.system.lock().unwrap();
        let process = system
            .process(self.pid)
            .expect("Failed to get process info");

        let cpu_usage = process.cpu_usage();
        let memory_usage = process.memory() as f64 / (1024.0 * 1024.0);

        (cpu_usage, memory_usage)
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
    let mut total_cpu = 0.0f32;
    let mut total_memory = 0.0f64;
    let mut last_result = None;

    for i in 0..iterations {
        print!("  Iteration {}/{}... ", i + 1, iterations);

        let start = Instant::now();
        let result = transcriber.transcribe(audio)?;
        let elapsed = start.elapsed();

        let (cpu, memory) = monitor.get_current_stats();

        total_elapsed_ms += elapsed.as_millis();
        total_cpu += cpu;
        total_memory += memory;
        last_result = Some(result);

        println!("{}ms", elapsed.as_millis());
    }

    let avg_elapsed_ms = total_elapsed_ms / iterations as u128;
    let avg_cpu = total_cpu / iterations as f32;
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
        cpu_usage_percent: avg_cpu,
        memory_usage_mb: avg_memory,
        model_size_mb: model_info.size_mb as u64,
    })
}

fn print_results(results: &[BenchmarkResult]) {
    println!("\n{}", "=".repeat(120));
    println!("BENCHMARK RESULTS");
    println!("{}", "=".repeat(120));

    println!(
        "\n{:<12} {:<50} {:>10} {:>8} {:>10} {:>10} {:>10}",
        "Model", "Transcription", "Time (ms)", "RTF", "Speed", "CPU (%)", "Mem (MB)"
    );
    println!("{}", "-".repeat(120));

    for result in results {
        let speed = if result.realtime_factor > 0.0 {
            format!("{:.2}x", 1.0 / result.realtime_factor)
        } else {
            "N/A".to_string()
        };

        let transcription = if result.transcription.len() > 47 {
            format!("{}...", &result.transcription[..47])
        } else {
            result.transcription.clone()
        };

        println!(
            "{:<12} {:<50} {:>10} {:>8.2} {:>10} {:>10.1} {:>10.1}",
            result.model_name,
            transcription,
            result.elapsed_ms,
            result.realtime_factor,
            speed,
            result.cpu_usage_percent,
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
        a.cpu_usage_percent
            .partial_cmp(&b.cpu_usage_percent)
            .unwrap()
    }) {
        println!(
            "Lowest CPU:     {} ({:.1}%)",
            lowest_cpu.model_name, lowest_cpu.cpu_usage_percent
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

fn main() {
    let args: Vec<String> = env::args().collect();

    let (wav_file, models, iterations) = if args.len() < 2 {
        eprintln!(
            "Usage: {} <wav-file> [model1,model2,...] [iterations]",
            args[0]
        );
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} tests/fixtures/quick_brown_fox.wav", args[0]);
        eprintln!("  {} audio.wav tiny.en,base.en,small.en", args[0]);
        eprintln!("  {} audio.wav all 3", args[0]);
        eprintln!();
        eprintln!("Available models:");
        for model in MODELS.iter() {
            eprintln!("  {} ({}MB)", model.name, model.size_mb);
        }
        process::exit(1);
    } else {
        let wav_file = &args[1];
        let models: Vec<String> = if args.len() > 2 && args[2] != "all" {
            args[2].split(',').map(|s| s.to_string()).collect()
        } else {
            MODELS.iter().map(|m| m.name.to_string()).collect()
        };
        let iterations = if args.len() > 3 {
            args[3].parse().unwrap_or(1)
        } else {
            1
        };
        (wav_file.to_string(), models, iterations)
    };

    println!("WAV Transcription Benchmark");
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

    print_results(&results);
}
