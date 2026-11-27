"""Score hypothesis files against FLEURS references with one shared normalizer.

Usage: score.py <refs.tsv> <name=hyp.tsv> [<name=hyp.tsv> ...] [--pair=a,b ...] [--clips=manifest.tsv]
  refs.tsv: clip\traw\tnormalized ; hyp.tsv: clip\taudio_s\tinfer_s\ttext
  --clips restricts scoring to the clips listed in a manifest (clip\twav).

Primary normalizer ("keep"): lowercase, NFKC, every Unicode mark/symbol/punctuation
-> space, collapse whitespace. It is Whisper's BasicTextNormalizer minus its
bracket removal: FLEURS speakers read parentheticals aloud, so dropping "(...)"
from the reference turns correctly transcribed words into insertions.
The "whisper" column applies BasicTextNormalizer as-is (drops [...] and (...)).
The "fleurs-norm" column scores against FLEURS' own normalized transcription.

Hypotheses have the SentencePiece "<unk>" piece stripped first (parakeet-ultra emits
it where its training text had parentheses). Neither normalizer rewrites numbers
("30" vs "dreißig") or merges compounds ("Sahel Zone" vs "Sahelzone"); both cost
every system WER, and leaderboard normalizers handle them differently, so absolute
numbers are only comparable within one scorer. CER (spaces removed) is
insensitive to compound splits.
"""

import random
import re
import sys
import unicodedata
from collections import defaultdict

import jiwer


def keep_normalize(text: str) -> str:
    text = unicodedata.normalize("NFKC", text.lower())
    text = "".join(" " if unicodedata.category(c)[0] in "MSP" else c for c in text)
    return re.sub(r"\s+", " ", text).strip()


def whisper_basic_normalize(text: str) -> str:
    text = re.sub(r"[<\[][^>\]]*[>\]]", "", text.lower())
    text = re.sub(r"\(([^)]+?)\)", "", text)
    return keep_normalize(text)


def load_tsv(path, columns):
    rows = {}
    with open(path, encoding="utf-8") as lines:
        for line in lines:
            parts = line.rstrip("\n").split("\t")
            rows[parts[0]] = tuple(parts[i] if len(parts) > i else "" for i in columns)
    return rows


def clip_errors(ref: str, hyp: str, normalize):
    """(word edits, ref words, char edits, ref chars) after normalization."""
    ref_n, hyp_n = normalize(ref), normalize(hyp)
    words = jiwer.process_words(ref_n, hyp_n) if hyp_n else None
    word_edits = (words.substitutions + words.deletions + words.insertions) if words else len(ref_n.split())
    ref_c, hyp_c = ref_n.replace(" ", ""), hyp_n.replace(" ", "")
    chars = jiwer.process_characters(ref_c, hyp_c) if hyp_c else None
    char_edits = (chars.substitutions + chars.deletions + chars.insertions) if chars else len(ref_c)
    return word_edits, len(ref_n.split()), char_edits, len(ref_c)


def rate(stats, clips, edit_idx, total_idx):
    edits = sum(stats[c][edit_idx] for c in clips)
    total = sum(stats[c][total_idx] for c in clips)
    return 100.0 * edits / total


def paired_bootstrap(stats_a, stats_b, clusters, rounds=4000, seed=7):
    rng = random.Random(seed)
    keys = list(clusters)
    diffs = []
    for _ in range(rounds):
        sample = [c for k in (rng.choice(keys) for _ in keys) for c in clusters[k]]
        diffs.append(rate(stats_a, sample, 0, 1) - rate(stats_b, sample, 0, 1))
    diffs.sort()
    better = sum(d < 0 for d in diffs) / rounds
    return diffs[int(0.025 * rounds)], diffs[int(0.975 * rounds)], better


def main():
    args = sys.argv[1:]
    options = dict(a[2:].split("=", 1) for a in args if a.startswith("--") and not a.startswith("--pair="))
    pairs = [a.split("=", 1)[1].split(",") for a in args if a.startswith("--pair=")]
    systems = [a.split("=", 1) for a in args[1:] if not a.startswith("--")]
    refs = load_tsv(args[0], (1, 2))
    raw_hyps = {name: {c: v[0] for c, v in load_tsv(path, (3,)).items()} for name, path in systems}
    # The SentencePiece <unk> piece is decoder output, not speech: strip it the way
    # transcribe.cpp does (parakeet-rs passes it through as literal text).
    hyps = {name: {c: t.replace("<unk>", " ") for c, t in h.items()} for name, h in raw_hyps.items()}
    common = set(refs).intersection(*(h.keys() for h in hyps.values()))
    if "clips" in options:
        common &= set(load_tsv(options["clips"], (1,)))
    common = sorted(common)
    clusters = defaultdict(list)
    for clip in common:
        clusters[refs[clip][0]].append(clip)
    print(f"clips scored: {len(common)} (distinct sentences: {len(clusters)})")
    stats = {}
    print(
        f"{'system':24} {'WER% keep':>10} {'WER% whisper':>13} {'WER% fleurs-norm':>17} {'CER%':>6} "
        f"{'empty':>6} {'<unk>':>6}"
    )
    for name, hyp in hyps.items():
        stats[name] = {c: clip_errors(refs[c][0], hyp[c], keep_normalize) for c in common}
        whisper = {c: clip_errors(refs[c][0], hyp[c], whisper_basic_normalize) for c in common}
        fleurs_norm = {c: clip_errors(refs[c][1], hyp[c], keep_normalize) for c in common}
        empty = sum(1 for c in common if not keep_normalize(hyp[c]))
        print(
            f"{name:24} {rate(stats[name], common, 0, 1):10.2f} {rate(whisper, common, 0, 1):13.2f} "
            f"{rate(fleurs_norm, common, 0, 1):17.2f} {rate(stats[name], common, 2, 3):6.2f} {empty:6d} "
            f"{sum(raw_hyps[name][c].count('<unk>') for c in common):6d}"
        )
    for a, b in pairs:
        lo, hi, p_better = paired_bootstrap(stats[a], stats[b], clusters)
        print(
            f"WER({a}) - WER({b}): {rate(stats[a], common, 0, 1) - rate(stats[b], common, 0, 1):+.2f} pp, "
            f"95% CI [{lo:+.2f}, {hi:+.2f}], P({a} better) = {p_better:.3f} (sentence-clustered bootstrap)"
        )


if __name__ == "__main__":
    main()
