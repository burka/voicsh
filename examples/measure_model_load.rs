//! Measure Whisper model load and drop times.
//!
//! Run:
//!   cargo run --release --features cuda --example measure_model_load
//!
//! Loads each model 3 times so cold CUDA init can be separated from steady-state
//! reload — the latter is the number that matters for an idle-unload policy.

use std::path::PathBuf;
use std::time::Instant;
use voicsh::stt::whisper::{WhisperConfig, WhisperTranscriber};

const ITERATIONS: usize = 3;

fn main() {
    let models_dir = dirs::cache_dir()
        .expect("no cache dir")
        .join("voicsh")
        .join("models");

    let mut models: Vec<PathBuf> = std::fs::read_dir(&models_dir)
        .expect("read models dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("bin"))
        .collect();
    models.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0));

    let use_gpu = std::env::var("VOICSH_NO_GPU").is_err();

    println!("# Whisper model load benchmark");
    println!("# use_gpu = {}", use_gpu);
    println!("# iterations per model = {}", ITERATIONS);
    println!(
        "# {:<30} {:>8}  {:>10}  {:>10}  {:>10}  {:>10}",
        "model", "size_MB", "load_1_ms", "load_2_ms", "load_3_ms", "drop_ms"
    );

    for model_path in &models {
        let size_mb = std::fs::metadata(model_path)
            .map(|m| m.len() as f64 / 1_048_576.0)
            .unwrap_or(0.0);
        let name = model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?");

        let mut loads_ms = [0.0_f64; ITERATIONS];
        let mut last_drop_ms = 0.0_f64;

        for i in 0..ITERATIONS {
            let config = WhisperConfig {
                model_path: model_path.clone(),
                language: "auto".to_string(),
                threads: None,
                use_gpu,
                idle_unload_after: None,
            };

            let t_load = Instant::now();
            let transcriber = match WhisperTranscriber::new(config) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("# load failed for {}: {}", name, e);
                    break;
                }
            };
            loads_ms[i] = t_load.elapsed().as_secs_f64() * 1000.0;

            let t_drop = Instant::now();
            drop(transcriber);
            last_drop_ms = t_drop.elapsed().as_secs_f64() * 1000.0;
        }

        println!(
            "  {:<30} {:>8.1}  {:>10.1}  {:>10.1}  {:>10.1}  {:>10.1}",
            name, size_mb, loads_ms[0], loads_ms[1], loads_ms[2], last_drop_ms
        );
    }
}
