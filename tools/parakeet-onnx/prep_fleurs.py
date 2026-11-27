"""Extract FLEURS audio (tar, data filter), convert to 16-bit PCM, write manifest + refs.

FLEURS ships 16 kHz mono float32 WAVs; voicsh transcribers take i16 PCM (what the
microphone path delivers), so every clip is converted once with soundfile (clipped
to [-1, 1], PCM_16). FLEURS has several recordings per sentence id; every recording
is one clip.

Usage: prep_fleurs.py <split.tsv> <split.tar.gz> <raw-audio-dir> <pcm-audio-dir> <work-dir>
Writes <work-dir>/manifest.tsv (clip\twav) and <work-dir>/refs.tsv (clip\traw\tnormalized).
"""

import csv
import sys
import tarfile
from pathlib import Path

import numpy as np
import soundfile as sf

tsv, tar_path, raw_dir, pcm_dir, work = (Path(a) for a in sys.argv[1:6])
for directory in (raw_dir, pcm_dir, work):
    directory.mkdir(parents=True, exist_ok=True)

with tarfile.open(tar_path, "r:gz") as tar:
    for member in tar:
        if member.isfile() and member.name.endswith(".wav"):
            member.name = Path(member.name).name
            tar.extract(member, raw_dir, filter="data")

rows = list(csv.reader(tsv.open(encoding="utf-8"), delimiter="\t", quoting=csv.QUOTE_NONE))
with (work / "manifest.tsv").open("w") as manifest, (work / "refs.tsv").open("w") as refs:
    for row in rows:
        _sentence_id, file_name, raw, normalized = row[0], row[1], row[2], row[3]
        audio, rate = sf.read(raw_dir / file_name, dtype="float32", always_2d=True)
        assert (rate, audio.shape[1]) == (16000, 1), file_name
        pcm = pcm_dir / file_name
        sf.write(pcm, np.clip(audio[:, 0], -1.0, 1.0), 16000, subtype="PCM_16")
        clip = Path(file_name).stem
        manifest.write(f"{clip}\t{pcm}\n")
        refs.write(f"{clip}\t{raw}\t{normalized}\n")
print(f"{len(rows)} clips")
