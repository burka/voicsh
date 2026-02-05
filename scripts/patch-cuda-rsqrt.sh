#!/usr/bin/env bash
# Patch CUDA 13.x headers for glibc 2.41+ (Ubuntu 25.10) compatibility.
#
# CUDA's math_functions.h declares rsqrt/rsqrtf without noexcept,
# but glibc 2.41 declares them with noexcept(true). This causes
# cudafe++ to fail with "exception specification is incompatible".
#
# This script adds noexcept to the CUDA declarations to match glibc.
# Patches all installed CUDA versions under /usr/local/cuda-*.
# Requires sudo. Safe to run multiple times (idempotent).

set -euo pipefail

patched=0

for cuda_dir in /usr/local/cuda-*/; do
    header="${cuda_dir}targets/x86_64-linux/include/crt/math_functions.h"

    if [[ ! -f "$header" ]]; then
        continue
    fi

    if grep -q 'rsqrt(double x) noexcept;' "$header"; then
        echo "Already patched: $header"
        continue
    fi

    echo "Patching $header ..."
    sudo sed -i.bak \
        -e 's/\(extern __DEVICE_FUNCTIONS_DECL__ __device_builtin__ double *rsqrt(double x)\);/\1 noexcept;/' \
        -e 's/\(extern __DEVICE_FUNCTIONS_DECL__ __device_builtin__ float *rsqrtf(float x)\);/\1 noexcept;/' \
        "$header"
    echo "  Done. Backup: ${header}.bak"
    patched=$((patched + 1))
done

if [[ $patched -eq 0 ]]; then
    echo "Nothing to patch."
else
    echo "Patched $patched CUDA installation(s)."
fi
