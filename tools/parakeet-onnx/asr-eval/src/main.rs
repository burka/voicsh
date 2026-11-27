//! Transcribe a manifest of 16 kHz mono WAV clips through voicsh's own transcribers
//! and record text + timing + peak RSS.
//!
//! Usage: asr-eval <tdt|whisper> <model-path> <manifest.tsv: id\twav> <out.tsv> <threads> [language]

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use voicsh::Transcriber;
use voicsh::stt::parakeet::{ParakeetArchitecture, ParakeetConfig, ParakeetTranscriber};
use voicsh::stt::whisper::{WhisperConfig, WhisperTranscriber};

fn proc_status_kb(field: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("read /proc/self/status");
    status
        .lines()
        .find_map(|l| l.strip_prefix(field))
        .and_then(|v| v.trim().trim_end_matches(" kB").trim().parse().ok())
        .expect("status field")
}

fn read_wav(path: &str) -> Vec<i16> {
    let mut reader = hound::WavReader::open(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let spec = reader.spec();
    assert_eq!(
        (spec.sample_rate, spec.channels),
        (16000, 1),
        "{path}: need 16 kHz mono"
    );
    reader
        .samples::<i16>()
        .map(|s| s.expect("sample"))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, backend, model, manifest, out, threads, rest @ ..] = args.as_slice() else {
        panic!("usage: asr-eval <tdt|whisper> <model> <manifest> <out> <threads> [language]");
    };
    let threads: usize = threads.parse().expect("threads");
    let language = rest.first().cloned().unwrap_or_else(|| "auto".to_string());
    let rss_before = proc_status_kb("VmRSS:");

    let load_start = Instant::now();
    let transcriber: Box<dyn Transcriber> = match backend.as_str() {
        "tdt" => Box::new(
            ParakeetTranscriber::new(ParakeetConfig {
                architecture: ParakeetArchitecture::Tdt,
                model_dir: PathBuf::from(model),
                language: language.clone(),
                threads: Some(threads),
                idle_unload_after: None,
            })
            .expect("load parakeet"),
        ),
        "whisper" => Box::new(
            WhisperTranscriber::new(WhisperConfig {
                model_path: PathBuf::from(model),
                language: language.clone(),
                threads: Some(threads),
                use_gpu: false,
                idle_unload_after: None,
            })
            .expect("load whisper"),
        ),
        other => panic!("unknown backend {other}"),
    };
    let load = load_start.elapsed();
    let rss_loaded = proc_status_kb("VmRSS:");

    let clips: Vec<(String, String)> = std::fs::read_to_string(manifest)
        .expect("manifest")
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (id, wav) = l.split_once('\t').expect("id\\twav");
            (id.to_string(), wav.to_string())
        })
        .collect();

    // Warm-up (ORT graph optimisation / first-call allocations), excluded from timing.
    let warm = read_wav(&clips[0].1);
    let warm_start = Instant::now();
    transcriber.transcribe(&warm).expect("warm-up");
    let warm_time = warm_start.elapsed();

    let mut file = std::fs::File::create(out).expect("out");
    let (mut audio_total, mut infer_total) = (0.0f64, Duration::ZERO);
    for (i, (id, wav)) in clips.iter().enumerate() {
        let samples = read_wav(wav);
        let seconds = samples.len() as f64 / 16000.0;
        let start = Instant::now();
        let result = transcriber.transcribe(&samples).expect("transcribe");
        let took = start.elapsed();
        audio_total += seconds;
        infer_total += took;
        let text = result.text.replace(['\t', '\n', '\r'], " ");
        writeln!(
            file,
            "{id}\t{seconds:.3}\t{:.4}\t{text}",
            took.as_secs_f64()
        )
        .expect("write");
        if (i + 1) % 50 == 0 {
            eprintln!(
                "  {}/{} clips, RTF so far {:.4}",
                i + 1,
                clips.len(),
                infer_total.as_secs_f64() / audio_total
            );
        }
    }
    println!(
        "backend={backend} model={model} threads={threads} lang={language} clips={} audio_s={audio_total:.1} \
         infer_s={:.2} rtf={:.4} load_s={:.2} warmup_s={:.2} rss_base_mb={} rss_loaded_mb={} rss_peak_mb={}",
        clips.len(),
        infer_total.as_secs_f64(),
        infer_total.as_secs_f64() / audio_total,
        load.as_secs_f64(),
        warm_time.as_secs_f64(),
        rss_before / 1024,
        rss_loaded / 1024,
        proc_status_kb("VmHWM:") / 1024,
    );
}
