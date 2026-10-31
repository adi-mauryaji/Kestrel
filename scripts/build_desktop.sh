#!/usr/bin/env bash
# The desktop installers (v4 A3; Linux: .deb and .rpm). The service binary goes in as the app's
# sidecar, and Kestrel's files as its resources: configs, contracts, tier-3 heads, the e5 encoder,
# the model catalog and the dev splits `kestrel calibrate` reads (never a test split, blueprint
# rule 8). Gemma is not inside: `kestrel model import FILE.gguf` brings it in after install.
# Bundle settings live in apps/desktop/tauri.bundle.json, so plain builds and CI need none of this.
# Needs tauri-cli (registry `tauri`): cargo install tauri-cli --version =2.12.3 --locked
set -euo pipefail
cd "$(dirname "$0")/.."

triple=$(rustc -vV | sed -n 's/^host: //p' | tr -d '\r')
exe=""
if [[ $triple == *windows* ]]; then exe=.exe; fi
cargo build --release -p kestrel-cli --features llama
mkdir -p apps/desktop/binaries
cp "target/release/kestrel$exe" "apps/desktop/binaries/kestrel-$triple$exe"
cd apps/desktop
cargo tauri build --config tauri.bundle.json "$@"
ls -l target/release/bundle/*/
