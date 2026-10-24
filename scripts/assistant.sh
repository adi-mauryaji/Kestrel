#!/usr/bin/env bash
# Kestrel as an assistant over your notes: `kestrel ask --notes DIR --brief` with the Intents-v0 router.
# Everything runs offline on this machine; tasks, preferences and memories go into the memory file
# ($KESTREL_MEMORY, else ~/.local/share/kestrel/memory.db).
#   scripts/assistant.sh ~/notes          (or set KESTREL_NOTES)
set -euo pipefail
cd "$(dirname "$0")/.."

NOTES="${1:-${KESTREL_NOTES:-}}"
if [[ -z $NOTES ]]; then
  echo "usage: scripts/assistant.sh NOTES_DIR   (or set KESTREL_NOTES)" >&2
  exit 2
fi
BIN=target/release/kestrel
if [[ ! -x $BIN ]]; then
  cargo build --release -p kestrel-cli --features llama
fi
exec "$BIN" ask --notes "$NOTES" --brief experiments/configs/router_intents_v0.json
