#!/usr/bin/env bash
# Kestrel's files the desktop installers need that are not in git (v4 A3, ADR-056): the e5 encoder
# (made by scripts/convert_encoder.sh) and the tier-3 heads (experiments/artifacts/tier3). Packed
# the same way each time (sorted, one time stamp, no owner), so the same files give the same
# SHA-256. The human uploads it once as a GitHub Release asset; .github/workflows/release-desktop.yml
# downloads it and checks that SHA-256 before building the macOS / Windows / Linux installers:
#   scripts/pack_ci_assets.sh
#   gh release create models-v1 target/kestrel-ci-assets-v1.tar.gz --title "CI assets v1" \
#     --notes "Kestrel's files for the installer builds (e5 encoder, tier-3 heads); not an app release."
# New heads or a new encoder: pack again as v2 (new tag and file name) and update the workflow.
set -euo pipefail
cd "$(dirname "$0")/.."

out=target/kestrel-ci-assets-v1.tar.gz
e5=models/multilingual-e5-small-q8_0.gguf
# The encoder must be the one models/SHA256SUMS names (the hash every Kestrel run checks).
(cd models && grep " ${e5#models/}\$" SHA256SUMS | sha256sum -c --quiet)
mkdir -p target
tar --sort=name --mtime=2026-10-31T00:00:00Z --owner=0 --group=0 --numeric-owner --format=gnu \
    --mode='u+rwX,go+rX,go-w' -cf - "$e5" experiments/artifacts/tier3 | gzip -n -9 > "$out"
ls -l "$out"
sha256sum "$out"
