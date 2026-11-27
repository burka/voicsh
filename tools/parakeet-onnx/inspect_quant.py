"""Summarize how an ONNX graph is quantized: op histogram and weight-scale granularity.

Usage: inspect_quant.py <model.onnx>
"""

import sys
from collections import Counter

import onnx

REPORTED_OPS = {
    "MatMulInteger",
    "ConvInteger",
    "MatMul",
    "Conv",
    "Gemm",
    "LSTM",
    "DynamicQuantizeLinear",
    "DynamicQuantizeLSTM",
    "QuantizeLinear",
    "DequantizeLinear",
    "MatMulNBits",
}

model = onnx.load(sys.argv[1], load_external_data=False)
graph = model.graph
ops = Counter(n.op_type for n in graph.node)
print("ops:", {op: n for op, n in ops.most_common() if op in REPORTED_OPS})
inits = {i.name: i for i in graph.initializer}
granularity = Counter()
for node in graph.node:
    if node.op_type in ("MatMulInteger", "ConvInteger"):
        weight = node.input[1]
        if weight not in inits:
            granularity[(node.op_type, "weight not constant")] += 1
            continue
        scale = inits.get(weight.removesuffix("_quantized") + "_scale")
        dims = tuple(scale.dims) if scale is not None else None
        kind = "?" if dims is None else ("per-tensor" if dims in ((), (1,)) else "per-channel")
        dtype = onnx.TensorProto.DataType.Name(inits[weight].data_type)
        granularity[(node.op_type, dtype, kind)] += 1
print("quantized weights:", dict(granularity))
print("opset:", [(o.domain, o.version) for o in model.opset_import])
