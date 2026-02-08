# WAV Transcription Benchmarking

This document describes the benchmarking suite for testing WAV transcription performance across different Whisper models, backends, and languages.

## Overview

The benchmark suite provides comprehensive performance testing capabilities:

1. **Standalone Binary** - Quick and easy performance comparison
2. **Multi-Backend Comparison** - Compare CPU vs GPU performance
3. **Multi-Language Testing** - Test the same model across different languages
4. **Criterion Benchmarks** - Statistical analysis for detailed performance testing

## Standalone Benchmark Tool

### Installation

The benchmark tool requires the `whisper` and `benchmark` features:

```bash
cargo build --release --bin benchmark-transcription --features whisper,benchmark
```

### Usage

Basic usage:
```bash
cargo run --release --bin benchmark-transcription --features whisper,benchmark -- <wav-file>
```

Test specific models:
```bash
cargo run --release --bin benchmark-transcription --features whisper,benchmark -- audio.wav tiny.en,base.en,small.en
```

Run multiple iterations for averaging:
```bash
cargo run --release --bin benchmark-transcription --features whisper,benchmark -- audio.wav all 3
```

Compare backends:
```bash
cargo run --release --bin benchmark-transcription --features whisper,benchmark -- audio.wav --compare-backends
```

Test multiple languages:
```bash
cargo run --release --bin benchmark-transcription --features whisper,benchmark -- audio.wav tiny --languages en,de,es,fr
```

Output as JSON for analysis:
```bash
cargo run --release --bin benchmark-transcription --features whisper,benchmark -- audio.wav all --output json > results.json
```

### Example Output

```
WAV Transcription Benchmark
========================================================================================================================
Audio file:     tests/fixtures/quick_brown_fox.wav
Samples:        112000
Duration:       7000ms (7.00s)
Sample rate:    16000 Hz
Iterations:     1
========================================================================================================================

Loading model: tiny.en
Running 1 iteration(s)...
  Iteration 1/1... 423ms
Result: "The quick brown fox jumps over the lazy dog."
Confidence: 0.85

Loading model: base.en
Running 1 iteration(s)...
  Iteration 1/1... 612ms
Result: "The quick brown fox jumps over the lazy dog."
Confidence: 0.92

========================================================================================================================
BENCHMARK RESULTS
========================================================================================================================

Model        Transcription                                      Time (ms)      RTF    Speed   CPU (%)   Mem (MB)
------------------------------------------------------------------------------------------------------------------------
tiny.en      The quick brown fox jumps over the lazy dog.            423     0.06    16.55x      85.2      245.3
base.en      The quick brown fox jumps over the lazy dog.            612     0.09    11.44x      92.1      378.5

========================================================================================================================
SUMMARY
========================================================================================================================
Fastest:        tiny.en (423ms, 0.06x realtime, 16.55x speed)
Most Efficient: tiny.en (0.06x realtime factor)
Lowest CPU:     tiny.en (85.2%)
Lowest Memory:  tiny.en (245.3MB)

Model Sizes:
  tiny.en      75MB
  base.en      142MB
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

Example output:
```
BACKEND COMPARISON
========================================================================================================================
Backend         Status        Active     Compile Flags
------------------------------------------------------------------------------------------------------------------------
CPU             Available     Yes        --no-default-features --features whisper,benchmark,model-download,cli
CUDA            Not compiled  No         --features cuda,benchmark,model-download,cli
Vulkan          Not compiled  No         --features vulkan,benchmark,model-download,cli
HipBLAS         Not compiled  No         --features hipblas,benchmark,model-download,cli
OpenBLAS        Not compiled  No         --features openblas,benchmark,model-download,cli
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

Expected speedups with GPU acceleration:
- CUDA: 10-50x faster than CPU
- Vulkan: 5-20x faster than CPU
- HipBLAS: 10-40x faster than CPU (AMD GPUs)

## Multi-Language Testing

Test the same model with different language codes to compare performance and accuracy:

```bash
cargo run --release --bin benchmark-transcription -- audio.wav tiny --languages auto,en,de,es,fr,it
```

Example output:
```
Model: tiny
Backend      Language    Time (ms)    RTF      Speed      CPU%       Memory
------------------------------------------------------------------------------------------------------------------------
CPU          auto        450          0.064    15.6x      22.1       248.1
CPU          en          445          0.063    15.8x      21.8       248.0
CPU          de          465          0.066    15.1x      22.3       248.2
CPU          es          458          0.065    15.4x      22.2       248.1
CPU          fr          462          0.066    15.2x      22.4       248.3
CPU          it          460          0.065    15.3x      22.3       248.2
```

Notes:
- Use multilingual models (without .en suffix) for language comparison
- `auto` lets Whisper detect the language automatically
- English-only models (.en suffix) ignore the language parameter and always use English
- Performance differences between languages are usually minimal (within 5-10%)

## Available Models

| Model | Size | Language | Typical RTF | Use Case |
|-------|------|----------|-------------|----------|
| tiny.en | 75 MB | English only | ~0.05 | Fast, lower accuracy |
| tiny | 75 MB | Multilingual | ~0.06 | Fast, multilingual |
| base.en | 142 MB | English only | ~0.08 | **Recommended for English** |
| base | 142 MB | Multilingual | ~0.09 | **Default, good balance** |
| small.en | 466 MB | English only | ~0.20 | Better accuracy |
| small | 466 MB | Multilingual | ~0.22 | Better accuracy, multilingual |
| medium.en | 1533 MB | English only | ~0.50 | High accuracy |
| medium | 1533 MB | Multilingual | ~0.55 | High accuracy, multilingual |
| large | 3094 MB | Multilingual | ~1.00 | Highest accuracy |

*Note: RTF values are approximate and depend on CPU speed*

## Installing Models

Models are downloaded automatically on first use, or you can pre-download them:

```bash
# Download a specific model
cargo run --release --features model-download -- download base.en

# Download all English models
for model in tiny.en base.en small.en medium.en; do
  cargo run --release --features model-download -- download $model
done
```

Models are cached in `~/.cache/voicsh/models/`

## Criterion Benchmarks

For detailed statistical analysis using Criterion:

```bash
cargo bench --bench wav_transcription
```

This will:
- Run each model 10 times (configurable)
- Calculate mean, median, and standard deviation
- Generate HTML reports in `target/criterion/`
- Compare against previous runs to detect performance regressions

View results:
```bash
open target/criterion/report/index.html
```

## Interpreting Results

### For Interactive Use (Voice Typing)

You need RTF < 1.0 for comfortable interactive use:
- RTF 0.1-0.3: Excellent, very responsive
- RTF 0.3-0.5: Good, slight delay acceptable
- RTF 0.5-1.0: Usable but noticeable lag
- RTF > 1.0: Not suitable for live transcription

Recommended models for live use: `tiny.en`, `base.en`, `small.en`

### For Batch Processing

RTF is less critical, prioritize accuracy:
- Use `medium.en` or `large` for best accuracy
- Processing time doesn't matter as much
- Higher CPU/memory usage is acceptable

### For Resource-Constrained Systems

Prioritize low CPU and memory usage:
- `tiny.en` or `tiny` for minimal resources
- `base.en` for better accuracy with moderate resources
- Avoid `medium` and `large` models

## Benchmarking Best Practices

1. **Close other applications** - CPU and memory measurements can be affected by background processes

2. **Run multiple iterations** - Use 3-5 iterations to get stable averages:
   ```bash
   cargo run --release --bin benchmark-transcription -- audio.wav all 5
   ```

3. **Use representative audio** - Test with audio similar to your actual use case:
   - Same duration (short clips vs long recordings)
   - Same content (speech style, accents, background noise)
   - Same sample rate and format

4. **Test on target hardware** - Performance varies significantly between CPUs:
   - Desktop CPUs: 2-5x faster than laptop CPUs
   - Apple Silicon (M1/M2): Often faster than Intel/AMD
   - Older CPUs: May struggle with larger models

5. **Consider GPU acceleration** - Build with CUDA/Vulkan for 10-50x speedup:
   ```bash
   cargo build --release --features whisper,cuda
   ```

## Troubleshooting

### Model Not Installed

```
Skipping base.en: model not installed
  Install with: cargo run --features model-download --release -- download base.en
```

Download the model first before benchmarking.

### Out of Memory

If benchmarking fails with OOM errors, try:
- Test smaller models only: `tiny.en,base.en`
- Close other applications
- Reduce iterations: use 1 instead of 3-5

### Slow Performance

If all models are slow (RTF > 1.0):
- Check CPU usage in system monitor
- Ensure running in `--release` mode (not debug)
- Consider GPU acceleration features
- Try a faster CPU or reduce model size

## Contributing Benchmark Results

When reporting benchmark results, include:
- CPU model and clock speed
- RAM amount
- Operating system
- Rust version (`rustc --version`)
- Full command used
- Complete benchmark output

This helps compare performance across different systems.
