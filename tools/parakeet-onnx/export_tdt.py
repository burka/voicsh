"""Export a Parakeet TDT checkpoint in HF-transformers safetensors layout to the
istupakov/onnx-asr ONNX layout that parakeet-rs loads (encoder-model.onnx +
encoder-model.onnx.data, decoder_joint-model.onnx, vocab.txt).

The NeMo model is built from the NeMo model_config.yaml of nvidia/parakeet-tdt-0.6b-v3
(see extract_nemo_config.py; no pickled checkpoint is ever loaded). Its weights come
from the safetensors file, renamed with the inverse of transformers'
convert_nemo_to_hf.py mapping; tensors without a NeMo counterpart (parakeet-ultra's
vad_head) are dropped and listed. Any other unmapped, missing or mis-shaped tensor
aborts the export.

Usage: export_tdt.py <nemo-config-dir> <model.safetensors> <out-dir>
"""

import re
import shutil
import sys
from pathlib import Path

import onnx
import torch
from nemo.collections.asr.models import EncDecRNNTBPEModel
from omegaconf import OmegaConf, open_dict
from safetensors.torch import load_file

# Inverse of transformers' NEMO_TO_HF_WEIGHT_MAPPING + NEMO_TDT_WEIGHT_MAPPING.
HF_TO_NEMO = [
    (r"^encoder\.subsampling\.layers\.", "encoder.pre_encode.conv."),
    (r"^encoder\.subsampling\.linear\.", "encoder.pre_encode.out."),
    (r"^encoder\.layers\.(\d+)\.conv\.norm\.", r"encoder.layers.\1.conv.batch_norm."),
    (r"\.q_proj\.", ".linear_q."),
    (r"\.k_proj\.", ".linear_k."),
    (r"\.v_proj\.", ".linear_v."),
    (r"\.o_proj\.", ".linear_out."),
    (r"\.relative_k_proj\.", ".linear_pos."),
    (r"\.bias_u$", ".pos_bias_u"),
    (r"\.bias_v$", ".pos_bias_v"),
    (r"^decoder\.embedding\.", "decoder.prediction.embed."),
    (r"^decoder\.lstm\.", "decoder.prediction.dec_rnn.lstm."),
    (r"^encoder_projector\.", "joint.enc."),
    (r"^decoder\.decoder_projector\.", "joint.pred."),
    (r"^joint\.head\.", "joint.joint_net.2."),
]
NOT_IN_NEMO = re.compile(r"^vad_head\.")


def to_nemo_key(key: str) -> str:
    for pattern, replacement in HF_TO_NEMO:
        key = re.sub(pattern, replacement, key)
    return key


def build_model(config_dir: Path) -> EncDecRNNTBPEModel:
    cfg = OmegaConf.load(config_dir / "model_config.yaml")
    tokenizer_model = next(config_dir.glob("*tokenizer.model"))
    tokenizer_vocab = next(config_dir.glob("*vocab.txt"))
    with open_dict(cfg):
        cfg.tokenizer.dir = str(config_dir)
        cfg.tokenizer.model_path = str(tokenizer_model)
        cfg.tokenizer.vocab_path = str(tokenizer_vocab)
        cfg.tokenizer.pop("spe_tokenizer_vocab", None)
        for ds in ("train_ds", "validation_ds", "test_ds"):
            cfg[ds] = None
    model = EncDecRNNTBPEModel(cfg=cfg)
    return model.eval()


def load_weights(model: torch.nn.Module, weights: Path) -> None:
    hf = load_file(str(weights))
    dropped = sorted(k for k in hf if NOT_IN_NEMO.match(k))
    mapped = {to_nemo_key(k): v.float() for k, v in hf.items() if not NOT_IN_NEMO.match(k)}
    target = model.state_dict()
    # The mel filterbank/window are derived from the config, not stored in HF weights.
    expected = {k for k in target if not k.startswith("preprocessor.")}
    missing = sorted(expected - mapped.keys())
    unexpected = sorted(mapped.keys() - expected)
    shape_mismatch = [k for k in expected & mapped.keys() if target[k].shape != mapped[k].shape]
    print(f"dropped (no NeMo counterpart): {dropped}")
    if missing or unexpected or shape_mismatch:
        raise SystemExit(f"mapping failed: missing={missing} unexpected={unexpected} shape={shape_mismatch}")
    result = model.load_state_dict(mapped, strict=False)
    if result.unexpected_keys or set(result.missing_keys) - {k for k in target if k.startswith("preprocessor.")}:
        raise SystemExit(f"load_state_dict mismatch: {result}")
    print(f"loaded {len(mapped)} tensors from {weights}")


def consolidate(src: Path, dest: Path) -> None:
    """Re-save an exported graph; weights near protobuf's 2 GB cap go to one <name>.data file (istupakov layout)."""
    model = onnx.load(str(src), load_external_data=True)
    size = sum(t.ByteSize() for t in model.graph.initializer)
    if size > 1_800_000_000:
        onnx.save_model(
            model,
            str(dest),
            save_as_external_data=True,
            all_tensors_to_one_file=True,
            location=dest.name + ".data",
            size_threshold=1024,
        )
    else:
        onnx.save_model(model, str(dest))


def write_vocab(model: EncDecRNNTBPEModel, out: Path) -> None:
    with (out / "vocab.txt").open("wt") as f:
        for i, token in enumerate([*model.tokenizer.vocab, "<blk>"]):
            f.write(f"{token} {i}\n")


def main() -> None:
    config_dir, weights, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
    out.mkdir(parents=True, exist_ok=True)
    torch.set_grad_enabled(False)
    model = build_model(config_dir)
    load_weights(model, weights)
    staging = out / "staging"
    staging.mkdir()
    model.export(str(staging / "model.onnx"))
    write_vocab(model, out)
    for graph in ("encoder-model.onnx", "decoder_joint-model.onnx"):
        consolidate(staging / graph, out / graph)
    shutil.rmtree(staging)
    print("exported:", sorted(p.name for p in out.iterdir()))


if __name__ == "__main__":
    main()
