"""Run two ONNX graphs with identical IO on the same random inputs and report output diffs.

Dynamic dims get fixed sizes; integer inputs named *length* get the time size, other
integer inputs get small token ids. Usage: compare_onnx.py <a.onnx> <b.onnx> [time=200]
"""

import sys

import numpy as np
import onnxruntime as ort

DTYPES = {"tensor(float)": np.float32, "tensor(int32)": np.int32, "tensor(int64)": np.int64}


def make_inputs(session, time):
    rng = np.random.default_rng(0)
    feeds = {}
    for node in session.get_inputs():
        shape = []
        for i, dim in enumerate(node.shape):
            if isinstance(dim, int):
                shape.append(dim)
            elif i == 0:
                shape.append(1)  # batch
            else:
                shape.append(time if i == len(node.shape) - 1 else 1)
        dtype = DTYPES[node.type]
        if dtype is np.float32:
            feeds[node.name] = rng.standard_normal(shape).astype(dtype)
        elif "length" in node.name:
            feeds[node.name] = np.full(shape, time, dtype=dtype)
        else:
            feeds[node.name] = rng.integers(0, 1000, shape).astype(dtype)
    return feeds


def main():
    a_path, b_path = sys.argv[1:3]
    time = int(sys.argv[3]) if len(sys.argv) > 3 else 200
    options = ort.SessionOptions()
    options.intra_op_num_threads = 4
    a = ort.InferenceSession(a_path, options, providers=["CPUExecutionProvider"])
    b = ort.InferenceSession(b_path, options, providers=["CPUExecutionProvider"])
    io_a = [(n.name, n.type) for n in a.get_inputs()], [n.name for n in a.get_outputs()]
    io_b = [(n.name, n.type) for n in b.get_inputs()], [n.name for n in b.get_outputs()]
    print("inputs/outputs A:", io_a)
    print("inputs/outputs B:", io_b)
    feeds = make_inputs(a, time)
    out_a, out_b = a.run(None, feeds), b.run(None, feeds)
    for name, x, y in zip(io_a[1], out_a, out_b):
        if x.dtype.kind == "f":
            diff = np.abs(x - y)
            print(
                f"{name}: shape {x.shape} max|a-b| {diff.max():.3e} mean|a-b| {diff.mean():.3e} max|a| {np.abs(x).max():.3e}"
            )
        else:
            print(f"{name}: shape {x.shape} equal={np.array_equal(x, y)}")


if __name__ == "__main__":
    main()
