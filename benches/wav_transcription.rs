use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::fs::File;
use std::hint::black_box;
use std::io::BufReader;
use std::time::Duration;
use voicsh::audio::wav::WavAudioSource;
use voicsh::models::catalog::MODELS;
use voicsh::models::download::model_path;
use voicsh::stt::transcriber::Transcriber;
use voicsh::stt::whisper::{WhisperConfig, WhisperTranscriber};

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

/// Criterion benchmark for Whisper models
fn criterion_benchmark(c: &mut Criterion) {
    let (audio, audio_duration_ms) = load_wav_fixture();
    println!(
        "Loaded WAV fixture: {} samples, {}ms duration",
        audio.len(),
        audio_duration_ms
    );
    let mut group = c.benchmark_group("whisper_models");
    group.sample_size(10); // Reduce sample size due to long transcription times
    group.measurement_time(Duration::from_secs(60)); // Allow up to 60s per benchmark

    for model in MODELS.iter() {
        if !model_path(model.name).exists() {
            eprintln!("Skipping {}: not installed", model.name);
            continue;
        }

        group.bench_with_input(
            BenchmarkId::from_parameter(model.name),
            &audio,
            |b, audio| {
                let config = WhisperConfig {
                    model_path: model_path(model.name),
                    language: "auto".to_string(),
                    threads: None,
                    use_gpu: true,
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

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
