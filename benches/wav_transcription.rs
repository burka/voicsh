use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, System};
use voicsh::audio::wav::WavAudioSource;
use voicsh::models::catalog::MODELS;
use voicsh::models::download::model_path;
use voicsh::stt::transcriber::Transcriber;
use voicsh::stt::whisper::{WhisperConfig, WhisperTranscriber};

/// System metrics collected during benchmarking
#[derive(Debug, Clone)]
struct SystemMetrics {
    cpu_usage_percent: f32,
    memory_usage_mb: f64,
    elapsed_ms: u128,
    audio_duration_ms: u64,
    realtime_factor: f64,
}

/// Monitor system resources during transcription
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

    fn measure<F>(&self, f: F) -> (SystemMetrics, String)
    where
        F: FnOnce() -> (String, u64),
    {
        // Initial measurement
        self.system.lock().unwrap().refresh_all();

        let start = Instant::now();
        let (result, audio_duration_ms) = f();
        let elapsed = start.elapsed();

        // Final measurement
        self.system.lock().unwrap().refresh_all();

        let system = self.system.lock().unwrap();
        let process = system
            .process(self.pid)
            .expect("Failed to get process info");

        let cpu_usage = process.cpu_usage();
        let memory_usage = process.memory() as f64 / (1024.0 * 1024.0);

        let realtime_factor = if audio_duration_ms > 0 {
            elapsed.as_millis() as f64 / audio_duration_ms as f64
        } else {
            0.0
        };

        let metrics = SystemMetrics {
            cpu_usage_percent: cpu_usage,
            memory_usage_mb: memory_usage,
            elapsed_ms: elapsed.as_millis(),
            audio_duration_ms,
            realtime_factor,
        };

        (metrics, result)
    }
}

/// Load WAV file and return audio samples with duration
fn load_wav_fixture() -> (Vec<i16>, u64) {
    let file =
        File::open("tests/fixtures/quick_brown_fox.wav").expect("Failed to open test fixture");
    let reader = BufReader::new(file);

    let source =
        WavAudioSource::from_reader(Box::new(reader)).expect("Failed to create WAV source");

    let samples = source.into_samples();

    // Calculate duration: samples / sample_rate * 1000 (to get ms)
    let duration_ms = (samples.len() as u64 * 1000) / 16000;

    (samples, duration_ms)
}

/// Test if a model is installed
fn is_model_installed(model_name: &str) -> bool {
    model_path(model_name).map_or(false, |p| p.exists())
}

/// Benchmark a single model
fn benchmark_model(
    model_name: &str,
    audio: &[i16],
    monitor: &ResourceMonitor,
) -> Option<SystemMetrics> {
    if !is_model_installed(model_name) {
        eprintln!("Skipping {}: model not installed", model_name);
        return None;
    }

    println!("Benchmarking model: {}", model_name);

    let config = WhisperConfig {
        model_path: model_path(model_name).expect("Model path not found"),
        language: "auto".to_string(),
        threads: None,
    };

    let transcriber = match WhisperTranscriber::new(config) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to load model {}: {}", model_name, e);
            return None;
        }
    };

    let audio_clone = audio.to_vec();
    let (metrics, text) = monitor.measure(|| {
        let result = transcriber
            .transcribe(&audio_clone)
            .expect("Transcription failed");
        let duration_ms = (audio_clone.len() as u64 * 1000) / 16000;
        (result.text, duration_ms)
    });

    println!("  Result: {} (confidence: {:.2})", text.trim(), 0.0); // Confidence not available in this benchmark
    println!(
        "  Time: {}ms, RTF: {:.2}x, CPU: {:.1}%, Memory: {:.1}MB",
        metrics.elapsed_ms,
        metrics.realtime_factor,
        metrics.cpu_usage_percent,
        metrics.memory_usage_mb
    );

    Some(metrics)
}

/// Criterion benchmark for Whisper models
fn criterion_benchmark(c: &mut Criterion) {
    let (audio, audio_duration_ms) = load_wav_fixture();
    println!(
        "Loaded WAV fixture: {} samples, {}ms duration",
        audio.len(),
        audio_duration_ms
    );

    let monitor = ResourceMonitor::new();
    let mut group = c.benchmark_group("whisper_models");
    group.sample_size(10); // Reduce sample size due to long transcription times
    group.measurement_time(Duration::from_secs(60)); // Allow up to 60s per benchmark

    for model in MODELS.iter() {
        if !is_model_installed(model.name) {
            eprintln!("Skipping {}: not installed", model.name);
            continue;
        }

        group.bench_with_input(
            BenchmarkId::from_parameter(model.name),
            &audio,
            |b, audio| {
                let config = WhisperConfig {
                    model_path: model_path(model.name).expect("Model path not found"),
                    language: "auto".to_string(),
                    threads: None,
                };

                let transcriber =
                    WhisperTranscriber::new(config).expect("Failed to create transcriber");

                b.iter(|| {
                    transcriber
                        .transcribe(black_box(audio))
                        .expect("Transcription failed")
                });
            },
        );
    }

    group.finish();
}

/// Manual comparison benchmark (not using Criterion)
fn run_comparison_benchmark() {
    println!("\n=== WAV Transcription Benchmark ===\n");

    let (audio, audio_duration_ms) = load_wav_fixture();
    println!(
        "Audio: {} samples, {}ms duration\n",
        audio.len(),
        audio_duration_ms
    );

    let monitor = ResourceMonitor::new();
    let mut results = Vec::new();

    for model in MODELS.iter() {
        if let Some(metrics) = benchmark_model(model.name, &audio, &monitor) {
            results.push((model.name, metrics));
        }
        println!();
    }

    // Print comparison table
    if !results.is_empty() {
        println!("\n=== Benchmark Results ===\n");
        println!(
            "{:<12} {:>10} {:>10} {:>8} {:>10} {:>10}",
            "Model", "Time (ms)", "RTF", "Speed", "CPU (%)", "Memory (MB)"
        );
        println!(
            "{:-<12} {:->10} {:->10} {:->8} {:->10} {:->10}",
            "", "", "", "", "", ""
        );

        for (name, metrics) in &results {
            let speed = if metrics.realtime_factor > 0.0 {
                format!("{:.2}x", 1.0 / metrics.realtime_factor)
            } else {
                "N/A".to_string()
            };

            println!(
                "{:<12} {:>10} {:>10.2} {:>8} {:>10.1} {:>10.1}",
                name,
                metrics.elapsed_ms,
                metrics.realtime_factor,
                speed,
                metrics.cpu_usage_percent,
                metrics.memory_usage_mb
            );
        }

        // Find fastest model
        if let Some((fastest_name, fastest_metrics)) = results
            .iter()
            .min_by(|a, b| a.1.elapsed_ms.cmp(&b.1.elapsed_ms))
        {
            println!(
                "\nFastest: {} ({}ms, {:.2}x realtime)",
                fastest_name, fastest_metrics.elapsed_ms, fastest_metrics.realtime_factor
            );
        }

        // Find most efficient (lowest RTF)
        if let Some((efficient_name, efficient_metrics)) = results.iter().min_by(|a, b| {
            a.1.realtime_factor
                .partial_cmp(&b.1.realtime_factor)
                .unwrap()
        }) {
            println!(
                "Most efficient: {} ({:.2}x realtime factor)",
                efficient_name, efficient_metrics.realtime_factor
            );
        }
    } else {
        println!("No models installed. Install models first:");
        println!("  cargo run --release -- download <model-name>");
    }
}

// Criterion main entry point
criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

// Add a test that runs the comparison benchmark
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Run with: cargo test --benches -- --ignored
    fn test_comparison_benchmark() {
        run_comparison_benchmark();
    }
}
