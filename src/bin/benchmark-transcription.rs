use std::env;
use std::process;
use voicsh::benchmark::*;
use voicsh::models::catalog::MODELS;
use voicsh::models::download::model_path;

fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse command line arguments
    let (wav_file, models, languages, iterations, output_format, compare_backends) = if args.len()
        < 2
    {
        eprintln!("Usage: {} <wav-file> [OPTIONS]", args[0]);
        eprintln!();
        eprintln!("OPTIONS:");
        eprintln!("  [model1,model2,...]     Models to benchmark (default: all)");
        eprintln!("  [iterations]            Number of iterations (default: 1)");
        eprintln!("  --output FORMAT         Output format: table (default) or json");
        eprintln!("  --languages LANGS       Languages to test (comma-separated, e.g., en,de,es)");
        eprintln!("  --compare-backends      Show available backends and compilation flags");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} tests/fixtures/quick_brown_fox.wav", args[0]);
        eprintln!("  {} audio.wav tiny.en,base.en,small.en", args[0]);
        eprintln!("  {} audio.wav all 3", args[0]);
        eprintln!("  {} audio.wav all 1 --output json", args[0]);
        eprintln!("  {} audio.wav tiny --languages en,de,es", args[0]);
        eprintln!("  {} audio.wav all --compare-backends", args[0]);
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

        // Check for --languages flag
        let languages: Vec<String> = if args.iter().any(|arg| arg == "--languages") {
            if let Some(pos) = args.iter().position(|arg| arg == "--languages") {
                args.get(pos + 1)
                    .map(|s| s.split(',').map(|l| l.trim().to_string()).collect())
                    .unwrap_or_else(|| vec!["auto".to_string()])
            } else {
                vec!["auto".to_string()]
            }
        } else {
            vec!["auto".to_string()]
        };

        // Check for --compare-backends flag
        let compare_backends = args.iter().any(|arg| arg == "--compare-backends");

        (
            wav_file.to_string(),
            models,
            languages,
            iterations,
            output_format.to_string(),
            compare_backends,
        )
    };

    // Print system information
    let system_info = SystemInfo::detect();
    system_info.print_report(compare_backends);

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
    println!("Languages:      {}", languages.join(", "));
    println!("{}", "=".repeat(120));
    println!();

    let monitor = ResourceMonitor::new();
    let mut results = Vec::new();

    for model_name in &models {
        if !model_path(model_name).is_some_and(|p| p.exists()) {
            eprintln!("Skipping {}: model not installed", model_name);
            eprintln!(
                "  Install with: cargo run --features model-download --release -- download {}",
                model_name
            );
            println!();
            continue;
        }

        for language in &languages {
            match benchmark_model(
                model_name,
                language,
                &audio,
                audio_duration_ms,
                &monitor,
                iterations,
            ) {
                Ok(result) => {
                    println!("Result: \"{}\"", result.transcription.trim());
                    println!(
                        "Detected language: {}, Confidence: {:.2}",
                        result.detected_language, result.confidence
                    );
                    println!();
                    results.push(result);
                }
                Err(e) => {
                    eprintln!("Failed to benchmark {} ({}): {}", model_name, language, e);
                    println!();
                }
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
        print_results(&results, languages.len() > 1);
    }
}
