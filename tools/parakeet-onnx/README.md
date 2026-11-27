# Parakeet TDT → ONNX export and German evaluation

Converts Parakeet TDT 0.6B checkpoints published as HF-transformers safetensors
(`nvidia/parakeet-tdt-0.6b-v3`, its fine-tune `moondream/parakeet-ultra`) into the
ONNX layout that `parakeet-rs` loads for `ParakeetArchitecture::Tdt`
(`src/stt/parakeet.rs`), quantizes them to int8 without losing accuracy, and
measures WER on FLEURS through voicsh's own transcribers.

Nothing here is part of the voicsh build or its quality gate.

## Safety

Downloaded checkpoints and datasets are untrusted data. Put each download in its own
empty directory, keep these scripts elsewhere, and run Python with `-I`. No pickled
checkpoint is loaded: the model is built from the NeMo YAML config and filled from
safetensors.

## Setup

```bash
uv venv --python 3.12 venv
uv pip install --python venv/bin/python -r tools/parakeet-onnx/requirements.txt
PY="venv/bin/python -I"; T=tools/parakeet-onnx
```

## Export

```bash
# NeMo config + tokenizer: the .nemo tar holds them before its checkpoint, so the head suffices.
curl -sSfL -r 0-1999999 -o dl/v3-nemo/head.nemo \
  https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3/resolve/541d1f99c6b0c3cd0b11a95167540bb8edefd82b/parakeet-tdt-0.6b-v3.nemo
$PY $T/extract_nemo_config.py dl/v3-nemo/head.nemo work/v3-config

curl -sSfL -o dl/ultra/model.safetensors \
  https://huggingface.co/moondream/parakeet-ultra/resolve/73175eb7aeb0d82f1e2a6b53b3aabc10a90bcd0b/model.safetensors
$PY $T/export_tdt.py work/v3-config dl/ultra/model.safetensors work/ultra-fp32
$PY $T/quantize_int8.py work/ultra-fp32 work/ultra-int8
$PY $T/inspect_quant.py work/ultra-int8/encoder-model.int8.onnx
```

The export runs on the CPU and needs several GB of RAM. Ultra stores its matrices in fp16, so the fp32
export is a lossless upcast. Its `vad_head` (long-form segmentation) is dropped:
voicsh segments audio with its own VAD.

`quantize_int8.py` documents the int8 recipe and why it is not per-tensor.

## Verify the pipeline

Export `nvidia/parakeet-tdt-0.6b-v3`'s `model.safetensors` the same way and compare it
with `istupakov/parakeet-tdt-0.6b-v3-onnx`, the export voicsh's catalog already trusts.
The outputs must be identical and `vocab.txt` byte-identical:

```bash
$PY $T/compare_onnx.py dl/istupakov-v3/encoder-model.onnx work/v3-fp32/encoder-model.onnx 400
$PY $T/compare_onnx.py dl/istupakov-v3/decoder_joint-model.onnx work/v3-fp32/decoder_joint-model.onnx 1
cmp dl/istupakov-v3/vocab.txt work/v3-fp32/vocab.txt
```

## Evaluate (FLEURS German)

```bash
# google/fleurs: data/de_de/test.tsv and data/de_de/audio/test.tar.gz
$PY $T/prep_fleurs.py dl/fleurs/test.tsv dl/fleurs/test.tar.gz dl/fleurs-wav work/fleurs/pcm16 work/fleurs
cargo build --release --manifest-path $T/asr-eval/Cargo.toml
$T/asr-eval/target/release/asr-eval tdt work/ultra-int8 work/fleurs/manifest.tsv work/hyp-ultra-int8.tsv 4 de
$T/asr-eval/target/release/asr-eval whisper ~/.cache/voicsh/models/ggml-large-v3-turbo.bin \
  work/fleurs/manifest.tsv work/hyp-whisper.tsv 4 de
$PY $T/score.py work/fleurs/refs.tsv ultra=work/hyp-ultra-int8.tsv whisper=work/hyp-whisper.tsv --pair=ultra,whisper
```

`asr-eval` prints RTF, load time and peak RSS. `score.py` documents its normalizer
and the pitfalls it avoids. Compare systems only on the same clips, with the same
normalizer: `score.py` scores the intersection of the hypothesis files.

## Licensing of exported weights

Both checkpoints are CC-BY-4.0. A redistributed export must credit NVIDIA
(`nvidia/parakeet-tdt-0.6b-v3`) and, for Ultra, Moondream (`moondream/parakeet-ultra`),
link the license, and state the changes (ONNX conversion, `vad_head` removed, int8
quantization).
