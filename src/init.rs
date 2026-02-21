//! Auto-tuning initialization for voicsh.

use std::collections::HashSet;
use std::path::Path;

use crate::benchmark::{ResourceMonitor, SystemInfo, benchmark_model, compute_default_threads};
use crate::config::Config;
use crate::inject::environment::{detect_environment, print_environment_summary};
use crate::models::catalog;
use crate::models::download::{download_model, is_model_installed, models_dir};
use crate::models::remote::fetch_remote_models;

// ── Tuning constants ────────────────────────────────────────────────────

const PROBE_MODEL: &str = "tiny";
const PROBE_MODEL_EN: &str = "tiny.en";

const REFERENCE_SAMPLE_RATE: usize = 16_000;
const REFERENCE_DURATION_SECS: u64 = 5;
const REFERENCE_NUM_SAMPLES: usize = REFERENCE_SAMPLE_RATE * REFERENCE_DURATION_SECS as usize;
const REFERENCE_DURATION_MS: u64 = REFERENCE_DURATION_SECS * 1000;

/// Models must be faster than this RTF to be considered real-time capable.
const RTF_REALTIME_THRESHOLD: f64 = 0.9;
/// Fallback candidates need extra headroom beyond the realtime threshold.
const RTF_FALLBACK_THRESHOLD: f64 = 0.7;

const RAM_HEADROOM_FACTOR: f64 = 1.2;
const DISK_HEADROOM_MB: u64 = 100;

/// English-only models get a small quality bonus when language is "en".
const ENGLISH_QUALITY_BONUS: f64 = 1.05;
/// Below this percentage the estimate is considered "spot on".
const ESTIMATE_ACCURACY_THRESHOLD: f64 = 5.0;

// ── Public types ────────────────────────────────────────────────────────

/// A candidate model for auto-tuning selection.
#[derive(Debug, Clone)]
pub struct ModelCandidate {
    /// Model name (e.g., "small", "base.en", "large-v3-q5_0")
    pub name: String,
    /// Model file size in megabytes
    pub size_mb: u32,
    /// Estimated real-time factor based on probe benchmarks
    pub estimated_rtf: f64,
    /// Quality score in 0.0..1.0
    pub quality: f64,
    /// Whether this model supports English only
    pub english_only: bool,
}

// ── Pure scoring / estimation functions ──────────────────────────────────

/// Extract the model tier (tiny, base, small, medium, large) from a model name.
///
/// The tier is the first component before any `.` or `-` separator.
pub fn model_tier(name: &str) -> &str {
    let base = name.split('.').next().unwrap_or(name);
    base.split('-').next().unwrap_or(base)
}

/// Quality score for a model tier.
///
/// Higher tiers have more parameters and produce better transcriptions.
pub fn tier_quality(tier: &str) -> f64 {
    match tier {
        "tiny" => 0.25,
        "base" => 0.45,
        "small" => 0.65,
        "medium" => 0.80,
        "large" => 0.95,
        _ => 0.0,
    }
}

/// Parse quantization suffix from a model name.
///
/// Returns the quantization identifier (e.g., `"q5_1"`, `"q4_0"`) or `""`
/// for full-precision models.
pub fn parse_quantization(name: &str) -> &str {
    for segment in name.rsplit('-') {
        if let Some(rest) = segment.strip_prefix('q')
            && rest.starts_with(|c: char| c.is_ascii_digit())
        {
            return segment;
        }
    }
    ""
}

/// Quality factor for a quantization level.
///
/// Full precision is 1.0; lower-bit quantization reduces quality slightly.
pub fn quant_quality(quant: &str) -> f64 {
    match quant {
        "" => 1.0,
        "q8_0" => 0.99,
        "q5_1" => 0.97,
        "q5_0" => 0.96,
        "q4_0" => 0.93,
        _ if quant.starts_with("q8") => 0.99,
        _ if quant.starts_with("q5") => 0.96,
        _ if quant.starts_with("q4") => 0.93,
        _ if quant.starts_with("q3") => 0.88,
        _ if quant.starts_with("q2") => 0.80,
        _ => 0.90,
    }
}

/// Compute a combined quality score for a model name.
///
/// The score is `tier_quality * quant_quality`, ranging from 0.0 to 1.0.
pub fn quality_score(name: &str) -> f64 {
    tier_quality(model_tier(name)) * quant_quality(parse_quantization(name))
}

/// Estimate the real-time factor (RTF) for a target model based on probe results.
///
/// Uses a linear size-ratio formula:
/// `estimated_rtf = probe_rtf * (target_size_mb / probe_size_mb)`
///
/// This works because file size is proportional to parameter count, and
/// compute is proportional to parameters.
pub fn estimate_rtf(probe_rtf: f64, probe_size_mb: u32, target_size_mb: u32) -> f64 {
    probe_rtf * (target_size_mb as f64 / probe_size_mb as f64)
}

/// Generate deterministic synthetic audio at 16 kHz for benchmarking.
///
/// Produces [`REFERENCE_NUM_SAMPLES`] i16 samples of low-amplitude pseudo-random
/// noise using a linear congruential generator (no `rand` dependency). The output
/// is deterministic so benchmarks are reproducible.
pub fn generate_reference_audio() -> Vec<i16> {
    // POSIX LCG constants — well-known, deterministic sequence
    const LCG_MULTIPLIER: u32 = 1103515245;
    const LCG_INCREMENT: u32 = 12345;
    const LCG_SEED: u32 = 42;
    const AMPLITUDE: i16 = 500;

    let mut samples = Vec::with_capacity(REFERENCE_NUM_SAMPLES);
    let mut state = LCG_SEED;
    for _ in 0..REFERENCE_NUM_SAMPLES {
        state = state
            .wrapping_mul(LCG_MULTIPLIER)
            .wrapping_add(LCG_INCREMENT);
        // Use bits 16..30 for better randomness from the LCG
        let raw = ((state >> 16) & 0x7FFF) as i16;
        samples.push(raw % (AMPLITUDE * 2 + 1) - AMPLITUDE);
    }
    samples
}

// ── System detection ────────────────────────────────────────────────────

/// Query available disk space (in MB) for the filesystem containing `path`.
///
/// Walks up to the nearest existing ancestor directory and queries via `statvfs`.
/// Returns `u64::MAX` on error (with a warning printed to stderr) so that the
/// disk-space filter does not block all models when detection fails.
pub fn available_disk_mb(path: &Path) -> u64 {
    let check_path = find_existing_ancestor(path);
    let path_str = match check_path.to_str() {
        Some(s) => s,
        None => {
            eprintln!(
                "voicsh: could not convert path to UTF-8 for disk check, assuming enough space"
            );
            return u64::MAX;
        }
    };
    let c_path = match std::ffi::CString::new(path_str) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("voicsh: invalid path for disk space check: {e}");
            return u64::MAX;
        }
    };

    crate::sys::available_disk_mb(&c_path).unwrap_or_else(|| {
        let err = std::io::Error::last_os_error();
        eprintln!("voicsh: could not determine disk space: {err}");
        u64::MAX
    })
}

/// Walk up the directory tree to find an existing ancestor.
fn find_existing_ancestor(path: &Path) -> &Path {
    let mut current = path;
    while !current.exists() {
        match current.parent() {
            Some(parent) => current = parent,
            None => return Path::new("/"),
        }
    }
    current
}

// ── Candidate filtering and ranking ─────────────────────────────────────

/// Filter model candidates by hardware constraints and language compatibility.
///
/// A candidate passes if:
/// - `model_size_mb * RAM_HEADROOM_FACTOR < available_ram_mb`
/// - `model_size_mb + DISK_HEADROOM_MB < available_disk_mb`
/// - `estimated_rtf < RTF_REALTIME_THRESHOLD`
/// - Language compatible (`.en` models excluded when `language != "en"`)
pub fn filter_candidates<'a>(
    candidates: &'a [ModelCandidate],
    available_ram_mb: u64,
    avail_disk_mb: u64,
    language: &str,
) -> Vec<&'a ModelCandidate> {
    candidates
        .iter()
        .filter(|c| {
            let ram_ok = (c.size_mb as f64 * RAM_HEADROOM_FACTOR) < available_ram_mb as f64;
            let disk_ok = (c.size_mb as u64 + DISK_HEADROOM_MB) < avail_disk_mb;
            let rtf_ok = c.estimated_rtf < RTF_REALTIME_THRESHOLD;
            let lang_ok = if language != "en" {
                !c.english_only
            } else {
                true
            };
            ram_ok && disk_ok && rtf_ok && lang_ok
        })
        .collect()
}

/// Collect models from catalog and HuggingFace, deduplicate, and estimate RTF.
async fn collect_all_candidates(
    probe_rtf: f64,
    probe_size: u32,
    allow_quantized: bool,
) -> Vec<ModelCandidate> {
    println!("Fetching available models from HuggingFace...");
    let remote_models = match fetch_remote_models().await {
        Ok(models) => models,
        Err(e) => {
            eprintln!("  Warning: could not fetch remote models: {e}");
            eprintln!("  Falling back to catalog models only.");
            Vec::new()
        }
    };

    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    // Catalog models first (have verified SHA-1 checksums)
    for m in catalog::list_models() {
        if seen.insert(m.name.to_string()) {
            candidates.push(ModelCandidate {
                name: m.name.to_string(),
                size_mb: m.size_mb,
                estimated_rtf: estimate_rtf(probe_rtf, probe_size, m.size_mb),
                quality: quality_score(m.name),
                english_only: m.english_only,
            });
        }
    }
    for m in &remote_models {
        if !allow_quantized && !parse_quantization(&m.name).is_empty() {
            continue;
        }
        if seen.insert(m.name.clone()) {
            candidates.push(ModelCandidate {
                name: m.name.clone(),
                size_mb: m.size_mb,
                estimated_rtf: estimate_rtf(probe_rtf, probe_size, m.size_mb),
                quality: quality_score(&m.name),
                english_only: m.english_only,
            });
        }
    }

    println!("  Found {} models", candidates.len());
    println!();

    candidates
}

/// Sort viable candidates by quality, highest first.
///
/// When `language` is `"en"`, `.en` models get a small quality bonus since
/// they are optimised for English transcription at the same model size.
fn rank_by_quality<'a>(
    mut viable: Vec<&'a ModelCandidate>,
    language: &str,
) -> Vec<&'a ModelCandidate> {
    viable.sort_by(|a, b| {
        let adjusted_quality = |c: &ModelCandidate| -> f64 {
            if language == "en" && c.english_only {
                c.quality * ENGLISH_QUALITY_BONUS
            } else {
                c.quality
            }
        };
        adjusted_quality(b)
            .partial_cmp(&adjusted_quality(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    viable
}

/// Context passed to [`verify_or_fallback`] to avoid a long parameter list.
struct VerifyContext<'a> {
    language: &'a str,
    audio: &'a [i16],
    monitor: &'a ResourceMonitor,
    verbose: u8,
    threads: usize,
    probe_name: &'a str,
}

/// Download, benchmark, and verify the recommended model.
///
/// If the recommended model turns out slower than real-time, iterates through
/// `alternatives` looking for one that works. Returns the final chosen model name.
async fn verify_or_fallback(
    recommended: &ModelCandidate,
    alternatives: &[&ModelCandidate],
    ctx: &VerifyContext<'_>,
) -> anyhow::Result<String> {
    // Download recommended model
    if is_model_installed(&recommended.name) {
        println!("Model {} already installed", recommended.name);
    } else {
        println!(
            "Downloading {} ({} MB)...",
            recommended.name, recommended.size_mb
        );
    }
    download_model(&recommended.name, true).await?;
    println!("\u{2713} Model {} installed", recommended.name);
    println!();

    // Benchmark to verify estimate
    println!("Verifying performance estimate...");
    let result = benchmark_model(
        &recommended.name,
        ctx.language,
        ctx.audio,
        REFERENCE_DURATION_MS,
        ctx.monitor,
        1,
        ctx.verbose,
        ctx.threads,
    )?;
    let actual_rtf = result.realtime_factor;

    println!(
        "  {}: {}ms for {:.1}s audio \u{2192} {:.2}x real-time ({:.1}x speed)",
        recommended.name,
        result.elapsed_ms,
        REFERENCE_DURATION_SECS as f64,
        actual_rtf,
        result.speed_multiplier,
    );

    let accuracy_pct = if recommended.estimated_rtf > 0.0 {
        (actual_rtf - recommended.estimated_rtf).abs() / recommended.estimated_rtf * 100.0
    } else {
        0.0
    };
    if accuracy_pct < ESTIMATE_ACCURACY_THRESHOLD {
        println!(
            "  Estimate accuracy: estimated {:.2}, actual {:.2} \u{2014} spot on!",
            recommended.estimated_rtf, actual_rtf,
        );
    } else {
        println!(
            "  Estimate accuracy: estimated {:.2}, actual {:.2} ({:.0}% off)",
            recommended.estimated_rtf, actual_rtf, accuracy_pct,
        );
    }
    println!();

    // If verified fast enough, we're done
    if actual_rtf <= 1.0 {
        return Ok(recommended.name.clone());
    }

    // Otherwise try alternatives
    eprintln!(
        "Warning: {} is slower than real-time ({:.2}x RTF).",
        recommended.name, actual_rtf,
    );
    for alt in alternatives {
        if alt.estimated_rtf >= RTF_FALLBACK_THRESHOLD {
            continue;
        }
        println!("Trying alternative: {} ...", alt.name);
        if !is_model_installed(&alt.name) {
            download_model(&alt.name, true).await?;
        }
        let alt_result = benchmark_model(
            &alt.name,
            ctx.language,
            ctx.audio,
            REFERENCE_DURATION_MS,
            ctx.monitor,
            1,
            ctx.verbose,
            ctx.threads,
        )?;
        if alt_result.realtime_factor < 1.0 {
            println!(
                "  {}: {:.2}x real-time \u{2014} OK!",
                alt.name, alt_result.speed_multiplier,
            );
            return Ok(alt.name.clone());
        }
    }

    eprintln!(
        "No alternative runs faster than real-time. Using probe model '{}'.",
        ctx.probe_name,
    );
    Ok(ctx.probe_name.to_string())
}

// ── Main entry point ────────────────────────────────────────────────────

/// Entry point for `voicsh init`.
///
/// Detects the desktop environment, recommends the best injection backend,
/// saves the recommendation to config, then runs benchmark auto-tuning.
pub async fn run_full_init(
    language: &str,
    verbose: u8,
    allow_quantized: bool,
) -> anyhow::Result<()> {
    // Step 1: Detect environment
    let env = detect_environment();
    print_environment_summary(&env);
    println!();

    // Step 2: Save recommended backend to config
    let config_path = Config::default_path();
    let config_exists = config_path.exists();

    // Load existing config or start with defaults
    let mut config = Config::load_or_default(&config_path);

    // Init always updates the backend — that's what "init" means.
    // force=true will also overwrite model settings in the benchmark step.
    config.injection.backend = env.recommended_backend;
    config
        .save(&config_path)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    if config_exists {
        println!("Updated injection.backend in {}", config_path.display());
    } else {
        println!("Created config at {}", config_path.display());
    }
    println!("  injection.backend = {:?}", config.injection.backend);
    println!();

    // Step 3: Run benchmark auto-tuning
    run_init(language, verbose, allow_quantized).await
}

/// Run the `voicsh auto-tune` flow (benchmark only).
///
/// Benchmarks a small probe model, estimates performance for all available
/// models, picks the highest-quality model that runs faster than real-time,
/// downloads it, verifies the estimate, and saves the configuration.
pub async fn run_init(language: &str, verbose: u8, allow_quantized: bool) -> anyhow::Result<()> {
    println!("voicsh init: Auto-tuning for your hardware...");
    println!();

    // ── Step 1: Detect system ───────────────────────────────────────────
    let sys_info = SystemInfo::detect();
    let cache_dir = models_dir();
    let disk_mb = available_disk_mb(&cache_dir);

    let gpu_str = sys_info
        .gpu_info
        .as_ref()
        .map(|g| g.display())
        .unwrap_or_else(|| "None".to_string());
    println!(
        "System: {} ({}c/{}t) | GPU: {} | RAM: {} GB",
        sys_info.cpu_model,
        sys_info.cpu_cores,
        sys_info.cpu_threads,
        gpu_str,
        sys_info.total_memory_mb / 1024,
    );
    let threads = compute_default_threads(sys_info.cpu_threads);
    println!(
        "Backend: {} | Threads: {}/{}",
        sys_info.whisper_backend, threads, sys_info.cpu_threads,
    );
    println!(
        "Disk: {} GB available on {}",
        disk_mb / 1024,
        find_existing_ancestor(&cache_dir).display(),
    );
    println!();

    // ── Step 2: Download probe model ────────────────────────────────────
    let probe_name = if language == "en" {
        PROBE_MODEL_EN
    } else {
        PROBE_MODEL
    };
    let probe_info = catalog::get_model(probe_name).ok_or_else(|| {
        anyhow::anyhow!("Probe model '{probe_name}' not found in catalog. This is a bug \u{2014} please report it.")
    })?;
    let probe_size = probe_info.size_mb;

    if is_model_installed(probe_name) {
        println!(
            "Probe model ({}, {} MB) already installed",
            probe_name, probe_size
        );
    } else {
        println!(
            "Downloading probe model ({}, {} MB)...",
            probe_name, probe_size
        );
    }
    download_model(probe_name, true).await?;
    println!("\u{2713} Model {} installed", probe_name);
    println!();

    // ── Step 3: Benchmark probe model ───────────────────────────────────
    let audio = generate_reference_audio();
    let monitor = ResourceMonitor::new();

    println!("Benchmarking probe model...");
    let probe_result = benchmark_model(
        probe_name,
        language,
        &audio,
        REFERENCE_DURATION_MS,
        &monitor,
        1,
        verbose,
        threads,
    )?;
    println!(
        "  {}: {}ms for {:.1}s audio \u{2192} {:.2}x real-time ({:.1}x speed)",
        probe_name,
        probe_result.elapsed_ms,
        REFERENCE_DURATION_SECS as f64,
        probe_result.realtime_factor,
        probe_result.speed_multiplier,
    );
    println!();

    // ── Step 4: Discover models and estimate performance ────────────────
    let candidates =
        collect_all_candidates(probe_result.realtime_factor, probe_size, allow_quantized).await;

    println!("Estimating performance for {} models...", candidates.len());
    let viable = filter_candidates(&candidates, sys_info.available_memory_mb, disk_mb, language);
    let too_slow = candidates.len() - viable.len();
    println!(
        "  \u{2713} {} models can run faster than real-time",
        viable.len()
    );
    if too_slow > 0 {
        println!(
            "  \u{2717} {} models too slow or incompatible for this hardware",
            too_slow
        );
    }
    println!();

    // ── Step 5: Handle empty viable set (fallback to probe) ─────────────
    if viable.is_empty() {
        println!("No models can run at real-time speed on this hardware.");
        println!("Using probe model '{}' as fallback.", probe_name);
        println!();
        save_and_summarize(probe_name, probe_size, probe_result.speed_multiplier)?;
        return Ok(());
    }

    // ── Step 6: Rank by quality and pick best ───────────────────────────
    let ranked = rank_by_quality(viable, language);
    let recommended = ranked[0];

    println!(
        "Recommended: {} ({} MB, estimated {:.2}x RTF, quality: {:.0}%)",
        recommended.name,
        recommended.size_mb,
        recommended.estimated_rtf,
        recommended.quality * 100.0,
    );
    if ranked.len() > 1 {
        println!("  Alternatives:");
        for alt in ranked.iter().skip(1).take(3) {
            println!(
                "    {:<14} ({:>4} MB, est. {:.2}x RTF, quality: {:.0}%)",
                alt.name,
                alt.size_mb,
                alt.estimated_rtf,
                alt.quality * 100.0,
            );
        }
    }
    println!();

    // ── Step 7: Download, verify, and potentially fallback ──────────────
    let ctx = VerifyContext {
        language,
        audio: &audio,
        monitor: &monitor,
        verbose,
        threads,
        probe_name,
    };
    let final_model = verify_or_fallback(recommended, &ranked[1..], &ctx).await?;

    // ── Step 8: Save config and print summary ───────────────────────────
    let final_size = candidates
        .iter()
        .find(|c| c.name == final_model)
        .map(|c| c.size_mb)
        .unwrap_or(0);
    let final_speed = candidates
        .iter()
        .find(|c| c.name == final_model)
        .map(|c| {
            if c.estimated_rtf > 0.0 {
                1.0 / c.estimated_rtf
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);

    save_and_summarize(&final_model, final_size, final_speed)?;
    Ok(())
}

/// Save the chosen model to config and print a summary.
fn save_and_summarize(model: &str, size_mb: u32, speed_multiplier: f64) -> anyhow::Result<()> {
    let config_path = Config::default_path();
    println!("Saving configuration to {}...", config_path.display());
    Config::update_model(&config_path, model)?;
    println!("  stt.model = \"{}\"", model);
    println!();
    println!("voicsh is ready! Run 'voicsh' to start voice typing.");
    println!("  Model: {} ({} MB)", model, size_mb);
    println!("  Speed: {:.1}x real-time", speed_multiplier);
    println!("  Tip: Run 'voicsh init' again anytime to re-tune.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quantization_full_precision() {
        assert_eq!(parse_quantization("tiny"), "");
        assert_eq!(parse_quantization("base.en"), "");
        assert_eq!(parse_quantization("large-v3"), "");
    }

    #[test]
    fn parse_quantization_quantized() {
        assert_eq!(parse_quantization("tiny-q4_0"), "q4_0");
        assert_eq!(parse_quantization("base.en-q5_1"), "q5_1");
        assert_eq!(parse_quantization("large-v3-q5_0"), "q5_0");
        assert_eq!(parse_quantization("medium-q8_0"), "q8_0");
        assert_eq!(parse_quantization("large-v3-turbo-q5_0"), "q5_0");
    }

    #[test]
    fn model_tier_extraction() {
        assert_eq!(model_tier("tiny"), "tiny");
        assert_eq!(model_tier("tiny.en"), "tiny");
        assert_eq!(model_tier("tiny-q4_0"), "tiny");
        assert_eq!(model_tier("base.en-q5_1"), "base");
        assert_eq!(model_tier("small"), "small");
        assert_eq!(model_tier("medium.en"), "medium");
        assert_eq!(model_tier("large-v3"), "large");
        assert_eq!(model_tier("large-v3-turbo"), "large");
        assert_eq!(model_tier("large-v3-turbo-q5_0"), "large");
        assert_eq!(model_tier("large-v1"), "large");
    }

    #[test]
    fn quality_score_ordering() {
        let tiny_q = quality_score("tiny");
        let base_q = quality_score("base");
        let small_q = quality_score("small");
        let medium_q = quality_score("medium");
        let large_q = quality_score("large-v3");

        assert!(
            tiny_q < base_q,
            "tiny ({tiny_q}) should be less than base ({base_q})"
        );
        assert!(
            base_q < small_q,
            "base ({base_q}) should be less than small ({small_q})"
        );
        assert!(
            small_q < medium_q,
            "small ({small_q}) should be less than medium ({medium_q})"
        );
        assert!(
            medium_q < large_q,
            "medium ({medium_q}) should be less than large ({large_q})"
        );
    }

    #[test]
    fn quality_score_quantized_lower() {
        let full = quality_score("small");
        let q5 = quality_score("small-q5_1");
        let q4 = quality_score("small-q4_0");

        assert!(q4 < q5, "q4 ({q4}) should be less than q5 ({q5})");
        assert!(q5 < full, "q5 ({q5}) should be less than full ({full})");
    }

    #[test]
    fn estimate_rtf_proportional_to_size() {
        let probe_rtf = 0.1;
        let probe_size = 75;

        let base_rtf = estimate_rtf(probe_rtf, probe_size, 142);
        assert!(
            (base_rtf - 0.189).abs() < 0.01,
            "base RTF should be ~0.189, got {base_rtf}"
        );

        let small_rtf = estimate_rtf(probe_rtf, probe_size, 466);
        assert!(
            (small_rtf - 0.621).abs() < 0.01,
            "small RTF should be ~0.621, got {small_rtf}"
        );
    }

    #[test]
    fn generate_reference_audio_correct_length() {
        let samples = generate_reference_audio();
        assert_eq!(samples.len(), REFERENCE_NUM_SAMPLES);
    }

    #[test]
    fn generate_reference_audio_not_silence() {
        let samples = generate_reference_audio();
        let has_nonzero = samples.iter().any(|&s| s != 0);
        assert!(has_nonzero, "Reference audio should not be pure silence");
    }

    #[test]
    fn generate_reference_audio_deterministic() {
        let a = generate_reference_audio();
        let b = generate_reference_audio();
        assert_eq!(a, b, "Reference audio should be deterministic");
    }

    #[test]
    fn filter_candidates_by_constraints() {
        let candidates = vec![
            ModelCandidate {
                name: "tiny".to_string(),
                size_mb: 75,
                estimated_rtf: 0.07,
                quality: 0.25,
                english_only: false,
            },
            ModelCandidate {
                name: "small".to_string(),
                size_mb: 466,
                estimated_rtf: 0.42,
                quality: 0.65,
                english_only: false,
            },
            ModelCandidate {
                name: "large-v3".to_string(),
                size_mb: 3095,
                estimated_rtf: 2.5,
                quality: 0.95,
                english_only: false,
            },
            ModelCandidate {
                name: "base.en".to_string(),
                size_mb: 142,
                estimated_rtf: 0.13,
                quality: 0.45,
                english_only: true,
            },
        ];

        // language "auto": .en models excluded, large too slow
        let filtered = filter_candidates(&candidates, 8192, 10000, "auto");
        let names: Vec<&str> = filtered.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["tiny", "small"]);

        // language "en": .en models included, large still too slow
        let filtered = filter_candidates(&candidates, 8192, 10000, "en");
        let names: Vec<&str> = filtered.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["tiny", "small", "base.en"]);

        // Limited RAM: small doesn't fit (466 * 1.2 = 559.2 > 400)
        let filtered = filter_candidates(&candidates, 400, 10000, "en");
        let names: Vec<&str> = filtered.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["tiny", "base.en"]);

        // Limited disk: base.en doesn't fit (142 + 100 = 242 > 200)
        let filtered = filter_candidates(&candidates, 8192, 200, "en");
        let names: Vec<&str> = filtered.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["tiny"]);
    }

    #[test]
    fn tier_quality_known_values() {
        assert_eq!(tier_quality("tiny"), 0.25);
        assert_eq!(tier_quality("base"), 0.45);
        assert_eq!(tier_quality("small"), 0.65);
        assert_eq!(tier_quality("medium"), 0.80);
        assert_eq!(tier_quality("large"), 0.95);
        assert_eq!(tier_quality("unknown"), 0.0);
    }

    #[test]
    fn quant_quality_known_values() {
        assert_eq!(quant_quality(""), 1.0);
        assert_eq!(quant_quality("q8_0"), 0.99);
        assert_eq!(quant_quality("q5_1"), 0.97);
        assert_eq!(quant_quality("q5_0"), 0.96);
        assert_eq!(quant_quality("q4_0"), 0.93);
    }

    #[test]
    fn quant_quality_prefix_fallbacks() {
        assert_eq!(quant_quality("q8_1"), 0.99);
        assert_eq!(quant_quality("q5_K"), 0.96);
        assert_eq!(quant_quality("q4_K"), 0.93);
        assert_eq!(quant_quality("q3_K"), 0.88);
        assert_eq!(quant_quality("q2_K"), 0.80);
    }

    #[test]
    fn quality_score_concrete_values() {
        assert!((quality_score("tiny") - 0.25).abs() < f64::EPSILON);
        assert!((quality_score("small-q5_1") - 0.6305).abs() < 0.001);
        assert!((quality_score("large-v3") - 0.95).abs() < f64::EPSILON);
        assert!((quality_score("large-v3-q4_0") - 0.8835).abs() < 0.001);
    }

    #[test]
    fn estimate_rtf_same_size_returns_probe_rtf() {
        let rtf = estimate_rtf(0.1, 75, 75);
        assert!((rtf - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_rtf_zero_probe_returns_zero() {
        let rtf = estimate_rtf(0.0, 75, 466);
        assert!(rtf.abs() < f64::EPSILON);
    }

    #[test]
    fn filter_candidates_empty_input() {
        let candidates: Vec<ModelCandidate> = vec![];
        let filtered = filter_candidates(&candidates, 8192, 10000, "en");
        assert!(
            filtered.is_empty(),
            "Filtering empty list should return empty"
        );
    }

    #[test]
    fn filter_candidates_all_too_slow() {
        let candidates = vec![ModelCandidate {
            name: "large-v3".to_string(),
            size_mb: 3095,
            estimated_rtf: 2.5,
            quality: 0.95,
            english_only: false,
        }];
        let filtered = filter_candidates(&candidates, 8192, 10000, "en");
        assert!(
            filtered.is_empty(),
            "All too-slow models should be filtered out"
        );
    }

    #[test]
    fn available_disk_mb_root() {
        let mb = available_disk_mb(Path::new("/"));
        assert!(mb > 0, "Root filesystem should report available space > 0");
    }

    #[test]
    fn rank_by_quality_prefers_higher_tiers() {
        let candidates = vec![
            ModelCandidate {
                name: "tiny".to_string(),
                size_mb: 75,
                estimated_rtf: 0.07,
                quality: quality_score("tiny"),
                english_only: false,
            },
            ModelCandidate {
                name: "small".to_string(),
                size_mb: 466,
                estimated_rtf: 0.42,
                quality: quality_score("small"),
                english_only: false,
            },
        ];
        let refs: Vec<&ModelCandidate> = candidates.iter().collect();
        let ranked = rank_by_quality(refs, "auto");
        assert_eq!(ranked[0].name, "small");
        assert_eq!(ranked[1].name, "tiny");
    }

    #[test]
    fn rank_by_quality_english_bonus() {
        let candidates = vec![
            ModelCandidate {
                name: "base".to_string(),
                size_mb: 142,
                estimated_rtf: 0.13,
                quality: quality_score("base"),
                english_only: false,
            },
            ModelCandidate {
                name: "base.en".to_string(),
                size_mb: 142,
                estimated_rtf: 0.13,
                quality: quality_score("base.en"),
                english_only: true,
            },
        ];
        // With language "en", the .en model should rank higher
        let refs: Vec<&ModelCandidate> = candidates.iter().collect();
        let ranked = rank_by_quality(refs, "en");
        assert_eq!(
            ranked[0].name, "base.en",
            ".en model should rank first for English"
        );

        // With language "auto", both have same quality, order is stable
        let refs: Vec<&ModelCandidate> = candidates.iter().collect();
        let ranked = rank_by_quality(refs, "auto");
        assert_eq!(
            ranked[0].name, "base",
            "multilingual should rank first for auto"
        );
    }

    #[test]
    fn constants_are_consistent() {
        assert_eq!(REFERENCE_NUM_SAMPLES, 80_000);
        assert_eq!(REFERENCE_DURATION_MS, 5000);
        assert!(RTF_FALLBACK_THRESHOLD < RTF_REALTIME_THRESHOLD);
    }
}
