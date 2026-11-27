"""Dynamic int8 quantization of an istupakov-layout export: per-channel, MatMul-only.

Only the MatMul weights (attention, feed-forward, joint projections) are quantized,
each with one scale per output channel. Conv (the pointwise/depthwise convolutions),
the LSTM prediction network and everything else stay fp32. Per-tensor int8 that also
quantizes Conv, as in the istupakov int8 export, transcribes measurably worse;
this recipe stays within noise of fp32 at about a third of its size.

Weights are signed int8 against uint8 activations (U8S8). On x86 CPUs without VNNI,
ONNX Runtime warns that U8S8 can saturate; check accuracy on such hardware before
shipping.

Writes encoder-model.int8.onnx, decoder_joint-model.int8.onnx and copies vocab.txt,
the int8 file names parakeet-rs discovers.

Usage: quantize_int8.py <fp32-dir> <int8-dir>
"""

import shutil
import sys
from pathlib import Path

from onnxruntime.quantization import QuantType, quantize_dynamic

src, dest = Path(sys.argv[1]), Path(sys.argv[2])
dest.mkdir(parents=True, exist_ok=True)
for graph in ("encoder-model", "decoder_joint-model"):
    quantize_dynamic(
        model_input=src / f"{graph}.onnx",
        model_output=dest / f"{graph}.int8.onnx",
        op_types_to_quantize=["MatMul"],
        per_channel=True,
        weight_type=QuantType.QInt8,
        extra_options={"MatMulConstBOnly": True},
    )
    print("quantized", graph)
shutil.copy(src / "vocab.txt", dest / "vocab.txt")
