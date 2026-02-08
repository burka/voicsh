use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::fs::File;
use std::io::BufReader;
use std::time::{Duration, Instant};
use voicsh::audio::wav::WavAudioSource;
use voicsh::benchmark::ResourceMonitor;
use voicsh::models::catalog::MODELS;
use voicsh::models::download::model_path;
use voicsh::stt::transcriber::Transcriber;
use voicsh::stt::whisper::{WhisperConfig, WhisperTranscriber};

/// System metrics collected during benchmarking
#[derive(Debug, Clone)]
struct SystemMetrics {
    cpu_usage_total: f32,
    cpu_usage_per_core: f32,
    memory_usage_mb: f64,
    elapsed_ms: u128,
    audio_duration_ms: u64,
    realtime_factor: f64,
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

    let start = Instant::now();
    let result = transcriber.transcribe(audio).expect("Transcription failed");
    let elapsed = start.elapsed();

    let (cpu_total, cpu_per_core, memory) = monitor.get_current_stats();
    let audio_duration_ms = (audio.len() as u64 * 1000) / 16000;

    let realtime_factor = if audio_duration_ms > 0 {
        elapsed.as_millis() as f64 / audio_duration_ms as f64
    } else {
        0.0
    };

    let metrics = SystemMetrics {
        cpu_usage_total: cpu_total,
        cpu_usage_per_core: cpu_per_core,
        memory_usage_mb: memory,
        elapsed_ms: elapsed.as_millis(),
        audio_duration_ms,
        realtime_factor,
    };

    println!(
        "  Result: {} (confidence: {:.2})",
        result.text.trim(),
        result.confidence
    );
    println!(
        "  Time: {}ms, RTF: {:.2}x, CPU: {:.1}% per core, Memory: {:.1}MB",
        metrics.elapsed_ms,
        metrics.realtime_factor,
        metrics.cpu_usage_per_core,
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
                metrics.cpu_usage_per_core,
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
