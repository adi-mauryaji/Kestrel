#!/usr/bin/env bash
# Live demo: type a request and watch the cascade route it (kestrel ask, Intents-v0 dev config).
# Run from anywhere: scripts/demo.sh   Builds the binary first if it is missing.
set -euo pipefail
cd "$(dirname "$0")/.."

BIN=target/release/kestrel
CONFIG=experiments/configs/router_intents_v0.json
if [[ ! -x $BIN ]]; then
  cargo build --release -p kestrel-cli --features llama
fi

cat <<'TXT'
Kestrel: on-device intent routing. Every request tries the cheapest tier first and goes
up only when that tier is not confident enough:

  T1 rules    keyword rules, microseconds
  T3 encoder  small embedding model + linear head, ~5-20 ms, accepts at confidence >= 0.964
  T4 SLM      Gemma 4 E2B (local, Q4), ~0.7-1 s, always answers

Each line shows the tier's guess, its calibrated confidence vs. that tier's threshold,
whether it accepted or escalated, and its time. Everything runs offline on this laptop.

Intents: search_notes, read_document, create_task, delete_task, summarize_notes,
         set_preference, retrieve_memory, chitchat, unsupported (anything else)

Try:  remind me to pay rent on friday            (rules)
      kal ki meeting notes ka summary do          (encoder, Hinglish)
      what did I say about my landlord last week  (escalates to the SLM)
      add milk and delete the dentist task        (two requests at once: straight to the SLM)
      book me a flight to delhi                   (unsupported)

TXT
exec "$BIN" ask "$CONFIG"
