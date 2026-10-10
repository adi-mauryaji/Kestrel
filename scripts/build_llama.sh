#!/usr/bin/env bash
# Build the llama.cpp CLI tools (llama-bench, llama-cli, llama-embedding, ...) used for
# Week-2 hardware benchmarks (E0). Pinned to the commit vendored by the Rust crate, so CLI
# numbers and the in-app runtime use the same kernels (blueprint Ch. 26).
#
#   llama-cpp-2 =0.1.157  ->  llama-cpp-sys-2/llama.cpp submodule @ LLAMA_COMMIT
#
# Output: third_party/llama.cpp/build/bin/  (gitignored). Re-running is idempotent.
set -euo pipefail

LLAMA_COMMIT=26394b4e6749a41c3633db040e0987500a5f7013
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/third_party/llama.cpp"

if [ ! -d "$SRC/.git" ]; then
  git clone --filter=blob:none https://github.com/ggml-org/llama.cpp.git "$SRC"
fi
git -C "$SRC" fetch --quiet origin "$LLAMA_COMMIT"
git -C "$SRC" checkout --quiet --detach "$LLAMA_COMMIT"

# CPU-only. GGML_NATIVE=ON tunes kernels to *this* CPU: fine for experiments, must be
# recorded in the environment record, wrong for redistributed binaries (Ch. 45.2).
cmake -S "$SRC" -B "$SRC/build" \
  -DCMAKE_BUILD_TYPE=Release \
  -DGGML_NATIVE=ON \
  -DLLAMA_CURL=OFF \
  -DBUILD_SHARED_LIBS=OFF
cmake --build "$SRC/build" --config Release -j "$(nproc)" \
  --target llama-bench llama-cli llama-embedding llama-quantize

echo "llama.cpp $(git -C "$SRC" rev-parse --short HEAD) built: $SRC/build/bin"
