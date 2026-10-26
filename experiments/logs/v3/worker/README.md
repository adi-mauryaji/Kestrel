# v3: the service's Gemma worker starting with mmap and a saved prompt prefix (ADR-036)

Engineering measurement for the always-on service, not a PREREG experiment.

**Why.** The first request that needs Gemma took ~20 s in daily use, but loading the weights takes only 2.5–3.7 s
(`../mmap/README.md`). `classify_items` on the service's own prompt (Intents-v0 config, 36 few-shot examples) showed
where the rest goes: the prompt prefix is **1132 tokens, primed in 12.6–16.7 s** (two runs, 2026-10-26), at the
start of every worker (`GenerativeTier::new`).

**Change.** The worker (`Models::load_for_service`) maps the weights and keeps the primed prefix's KV state in
`$XDG_CACHE_HOME/kestrel/tier4-<config>-<model size>-<model mtime>-<n_ctx>.kv` (13 MB). The state is used only
when the token list saved with it equals the prefix's; otherwise the prefix is computed and the file replaced
(`LlamaSession::prime_cached`). `scripts/install_service.sh` deletes these files, so a new binary never reads an
old binary's state. Research loads are unchanged (weights copied, prefix computed).

**Tests.** `kestrel-inference` (model-backed, Qwen3-1.7B): a loaded prefix classifies byte for byte like a
computed one (every logit compared); a file saved for another prefix, or a damaged one, is recomputed.
`apps/cli/tests/worker.rs` (Gemma): the real worker maps its weights, saves one file, and after a restart from
that file gives the same answer bytes.

**Startup** (`20261026T142304Z.tsv`): time until the worker exits after one empty request, i.e. load + prime +
one error reply; AC, boost on, model file in the page cache; the three binaries alternate in each round.

| worker | prefix | n | seconds (median, range) | RssAnon / RssFile at ready, MiB |
|---|---|---|---|---|
| v2 (installed, `stage2-frozen`) | computed | 3 | 19.1 (18.0–19.3) | 3356 / 8 |
| v3, cache empty | computed, saved | 3 | 17.4 (17.3–17.9) | — |
| v3, cache saved | loaded | 3 | **2.0** (2.00–2.04) | 1135 / 2702 |

`maxrss_kib` in the TSV is `VmHWM`, which also counts the mapped file (2702 MiB, clean pages the kernel can
reclaim): 3.8 GiB for v3 against 3.3 GiB for v2, although v3's anonymous memory is a third of v2's.

**Reproduce**
```
cargo build --release -p kestrel-cli --features llama
C=$(mktemp -d)   # a cache directory of your own
echo '' | XDG_CACHE_HOME=$C /usr/bin/time -f '%e s' target/release/kestrel tier4-worker \
    experiments/configs/router_intents_v0.json > /dev/null   # 1st: computes and saves; 2nd: loads
cargo test --release -p kestrel-cli --features llama --test worker -- --ignored
```
