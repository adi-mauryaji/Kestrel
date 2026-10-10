#!/usr/bin/env bash
# Convert multilingual-e5-small (no official GGUF) from the original weights to GGUF using
# the pinned llama.cpp converter. Inputs: models/src/multilingual-e5-small (fetched and
# hash-verified by scripts/fetch_models.py). Outputs: models/multilingual-e5-small-{f16,q8_0}.gguf
#
# Needs the converter venv (separate from the research .venv because the converter pins
# torch/transformers):
#   python3 -m venv third_party/convert-venv
#   third_party/convert-venv/bin/pip install --index-url https://download.pytorch.org/whl/cpu torch==2.11.0
#   third_party/convert-venv/bin/pip install transformers==4.57.6 'sentencepiece<0.3' 'protobuf>=6,<7' numpy ./third_party/llama.cpp/gguf-py
# protobuf deviates from llama.cpp's legacy pin (<5): protobuf 4.x's C extension crashes on
# Python 3.14 ("Metaclasses with custom tp_new are not supported"), wheel and sdist alike.
# Checked 2026-10-10: protobuf 6.33.6 parses sentencepiece.bpe.model correctly (UNIGRAM,
# 250000 pieces, normalizer charsmap present), which is the converter's only protobuf use.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PY="$ROOT/third_party/convert-venv/bin/python"
# Wrapper around llama.cpp's convert_hf_to_gguf.py; see its docstring for the one patch.
CONVERT="$ROOT/scripts/convert_bert_spm.py"
SRC="$ROOT/models/src/multilingual-e5-small"

for outtype in f16 q8_0; do
  "$PY" "$CONVERT" "$SRC" --outtype "$outtype" \
    --outfile "$ROOT/models/multilingual-e5-small-$outtype.gguf"
done
sha256sum "$ROOT"/models/multilingual-e5-small-*.gguf
