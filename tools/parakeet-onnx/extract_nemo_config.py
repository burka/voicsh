"""Extract the config and tokenizer members of a .nemo archive, never the pickled checkpoint.

A .nemo file is a tar archive whose checkpoint is the last member, so the first few
megabytes of the file (an HTTP range download) are enough; reading stops at the
checkpoint. Fails if model_config.yaml or a tokenizer file was not found before it.

Usage: extract_nemo_config.py <file.nemo | first-bytes-of-file.nemo> <out-dir>
"""

import sys
import tarfile
from pathlib import Path

WEIGHT_SUFFIXES = (".ckpt", ".pt", ".pth", ".bin")

nemo, out = Path(sys.argv[1]), Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
with tarfile.open(nemo, "r:") as tar:
    for member in tar:
        name = Path(member.name).name
        if name.endswith(WEIGHT_SUFFIXES):
            break
        if member.isfile():
            member.name = name  # flatten; filter="data" also rejects traversal and links
            tar.extract(member, out, filter="data")
            print(f"extracted {out / name}")
for required in ("model_config.yaml", "*tokenizer.model", "*vocab.txt"):
    if not any(out.glob(required)):
        raise SystemExit(f"{nemo}: no {required} before the checkpoint member; download more of the file")
