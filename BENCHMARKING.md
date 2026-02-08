# WAV Transcription Benchmarking

This document describes the benchmarking suite for testing WAV transcription performance across different Whisper models, backends, and languages.

## Overview

The benchmark suite provides:
- Standalone binary for quick performance comparison
- Multi-backend comparison (CPU vs GPU)
- Multi-language testing
- Criterion integration for statistical analysis

## Standalone Benchmark Tool

### Installation

The benchmark tool requires the `whisper` and `benchmark` features:

```bash
cargo build --release --bin benchmark-transcription --features whisper,benchmark
```

### Usage

```bash
# Basic usage
cargo run --release --bin benchmark-transcription --features whisper,benchmark -- <wav-file>

# Specific models with iterations
cargo run --release --bin benchmark-transcription --features whisper,benchmark -- audio.wav tiny.en,base.en,small.en 3

# Compare backends
cargo run --release --bin benchmark-transcription --features whisper,benchmark -- audio.wav --compare-backends

# Test languages
cargo run --release --bin benchmark-transcription --features whisper,benchmark -- audio.wav tiny --languages en,de,es,fr

# JSON output
cargo run --release --bin benchmark-transcription --features whisper,benchmark -- audio.wav all --output json > results.json
```

## Metrics Explained

- **Time (ms)** - Total transcription time in milliseconds
- **RTF (Realtime Factor)** - Ratio of processing time to audio duration
  - RTF < 1.0 means faster than realtime (good)
  - RTF = 0.5 means processing is 2x faster than realtime
  - RTF = 1.0 means processing takes same time as audio duration
  - RTF > 1.0 means slower than realtime (problematic for live use)
- **Speed** - Inverse of RTF (1/RTF), shows how many times faster than realtime
  - 10x speed means processing is 10 times faster than the audio duration
- **CPU (%)** - CPU usage percentage during transcription
- **Mem (MB)** - Memory usage in megabytes during transcription

## Multi-Backend Comparison

The benchmark tool supports comparing performance across different backends (CPU, CUDA, Vulkan, HipBLAS, OpenBLAS).

### Backend Detection

Use `--compare-backends` to see which backends are available:

```bash
cargo run --release --bin benchmark-transcription -- audio.wav --compare-backends
```

### Comparing Backends

To compare different backends, compile and run the benchmark with each backend:

```bash
# CPU benchmark
cargo build --release --bin benchmark-transcription --no-default-features --features whisper,benchmark,model-download,cli
./target/release/benchmark-transcription audio.wav tiny --output json > results-cpu.json

# CUDA benchmark (requires NVIDIA GPU and CUDA toolkit)
cargo build --release --bin benchmark-transcription --features cuda,benchmark,model-download,cli
./target/release/benchmark-transcription audio.wav tiny --output json > results-cuda.json

# Vulkan benchmark (requires Vulkan SDK)
cargo build --release --bin benchmark-transcription --features vulkan,benchmark,model-download,cli
./target/release/benchmark-transcription audio.wav tiny --output json > results-vulkan.json
```

### Backend Requirements

- **CPU**: No additional requirements (always available)
- **CUDA**: NVIDIA GPU, CUDA toolkit, cuDNN
- **Vulkan**: Vulkan SDK
- **HipBLAS**: AMD GPU, ROCm/HIP toolkit
- **OpenBLAS**: OpenBLAS library

## Multi-Language Testing

Test the same model with different language codes to compare performance and accuracy:

```bash
cargo run --release --bin benchmark-transcription -- audio.wav tiny --languages auto,en,de,es,fr,it
```

Notes:
- Use multilingual models (without .en suffix) for language comparison
- `auto` lets Whisper detect the language automatically
- English-only models (.en suffix) ignore the language parameter and always use English
- Performance differences between languages are usually minimal (within 5-10%)

## Available Models

| Model | Size | Language | Use Case |
|-------|------|----------|----------|
| tiny.en | 75 MB | English only | Fast, lower accuracy |
| tiny | 75 MB | Multilingual | Fast, multilingual |
| base.en | 142 MB | English only | **Recommended for English** |
| base | 142 MB | Multilingual | **Default, good balance** |
| small.en | 466 MB | English only | Better accuracy |
| small | 466 MB | Multilingual | Better accuracy, multilingual |
| medium.en | 1533 MB | English only | High accuracy |
| medium | 1533 MB | Multilingual | High accuracy, multilingual |
| large | 3094 MB | Multilingual | Highest accuracy |

## Installing Models

Models auto-download on first use or pre-download manually:

```bash
cargo run --release --features model-download -- download base.en
```

Models are cached in `~/.cache/voicsh/models/`

## Criterion Benchmarks

For detailed statistical analysis:

```bash
cargo bench --bench wav_transcription
open target/criterion/report/index.html
```

Provides mean, median, standard deviation, and regression detection across runs.

## Interpreting Results

**Interactive Use (Voice Typing)**: RTF < 1.0 required for comfortable use. Models: `tiny.en`, `base.en`, `small.en`

**Batch Processing**: Prioritize accuracy over speed. Models: `medium.en`, `large`

**Resource-Constrained Systems**: Use `tiny.en` or `base.en`

## Troubleshooting

**Model Not Installed**: Download with `cargo run --features model-download --release -- download <model>`

**Out of Memory**: Test smaller models (`tiny.en`, `base.en`) or reduce iterations

**Slow Performance**: Ensure `--release` mode, check CPU usage, or try GPU acceleration
