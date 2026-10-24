# Reproducing Kestrel's results (Week 15, blueprint Ch. 40.3)

Everything a result depends on is pinned and hashed. A test-set run is only valid on the frozen PREREG (tag
`exp-E1-frozen`, `experiments/PREREG.md` §9), and its logs, manifests and analyses are committed.

| What | Pinned by |
|---|---|
| Rust toolchain, crates | `rust-toolchain.toml`, `Cargo.lock` (`llama-cpp-2 =0.1.157`, llama.cpp 26394b4e) |
| Python | 3.14.7, `requirements.lock` |
| Models | `models/MANIFEST.toml` (repo, revision, SHA-256; the e5 GGUF is converted locally by `scripts/convert_encoder.sh`) |
| Public data | `datasets/MANIFEST.toml` (CLINC150 at a pinned commit, SHA-256) |
| Kestrel data | `datasets/kestrel_intents_v0/splits/` (fingerprints in PREREG §2) |
| Configs | `experiments/configs/` (FROZEN; `python experiments/candidates.py --check`) |
| Runs | `experiments/logs/<experiment>/` with a `manifest.json` per run (environment, hashes, checks, `valid`) |

## On this machine: one command

```
python experiments/repro.py --rerun-e5
```

It clones the committed HEAD into a scratch directory, adds the raw datasets after the SHA-256 check, regenerates
every analysis from the committed raw logs, and compares each output with the committed one. With `--rerun-e5` it
also reruns E5 on test (about 3 minutes). Last result: `logs/repro/20261024T035903Z.json`, **reproduced at 74def15**:
all 11 outputs byte-identical (Tables R1–R3, R6, the Holm family, RESULTS.md, the failure analysis), and the E5
rerun's numbers equal (timings excluded).

## On a second machine (Week 15 "Done": a second person or clean VM reproduces one result)

```
git clone <repo> kestrel && cd kestrel && git checkout <commit to reproduce>
python3.14 -m venv .venv && .venv/bin/pip install -r requirements.lock
python scripts/fetch_models.py          # downloads, verifies SHA-256 against models/MANIFEST.toml
scripts/convert_encoder.sh              # multilingual-e5-small -> GGUF (pinned converter, its own venv in
                                        # third_party/convert-venv); hash checked against the manifest
python scripts/fetch_datasets.py        # CLINC150 at the pinned commit, SHA-256 checked
scripts/build_llama.sh                  # llama.cpp CLI at the vendored commit (E0's llama-bench only)
cargo build --release -p kestrel-cli --features llama
cargo build --release -p kestrel-nlu --example rules_items
cargo build --release -p kestrel-inference --features llama --example classify_items --example embed_texts
python experiments/tier3/train.py       # rebuilds the embedding cache and the Tier-3 heads
python experiments/candidates.py --check
python experiments/repro.py --rerun-e5
```

The Tier-3 heads are not in git (`experiments/artifacts/`). The frozen configs pin each head's SHA-256, and the E1
harness refuses a mismatch. Retraining with `train.py` is deterministic on this machine; whether it gives the same
bytes on another platform has not been checked. If it doesn't, copy `experiments/artifacts/tier3/` from this machine,
because a different head is a different system. The analyses reproduce from the committed logs anywhere; rerunning
E5 needs the embedding cache. **Latency, RSS and E0 numbers
belong to this machine** (Ryzen 5 5500U, Fedora, kernel 7.2.6, PREREG §4) and must never be mixed with numbers from
another one. Another machine's timings are a different platform, reported separately if at all.

## Per experiment

| Exp. | Run (test, after the freeze) | Analysis | Artifacts |
|---|---|---|---|
| E0 | `python experiments/measure/bench_e0.py` (protocol v3: AC, boost 0, governor performance) | `python experiments/analysis/e0_summary.py <3 session CSVs>` | `logs/E0/<session>.csv`, `.env.json`, README |
| E1 | `python experiments/e1/run_e1.py --dataset {intents_v0,clinc150} --split test --chunk K` (CLINC150: K = 1..4), then `python experiments/e1/rss.py --dataset D` | `python experiments/analysis/analyze_e1.py --dataset D --split test` | `logs/E1/<dataset>_test/chunk*/`, `logs/E1/rss/`, `logs/E1/analysis_<dataset>_test.json` |
| E2 | `python experiments/router/tier_outputs.py --dataset D --split test` | `python experiments/analysis/analyze_e2.py --dataset D --split test` | `logs/router/tier_outputs_<dataset>_test.*`, `logs/E2/analysis_<dataset>_test.json` |
| E3 | (from E1's rows) | `python experiments/analysis/analyze_e3.py --dataset D --split test` | `logs/E3/analysis_<dataset>_test.json` |
| E5 | `python experiments/continual/e5_test.py --jobs 5` | `python experiments/analysis/analyze_e5.py` | `logs/E5/test.json`, `logs/E5/analysis_test.json` |
| Family | (after the analyses) | `python experiments/analysis/family.py`, `results.py` | `logs/family_v1.json`, `RESULTS.md` |
| Privacy (Ch. 42) | `python experiments/security/privacy_check.py` | (in the run) | `logs/privacy/<UTC>.json` |
| Failure analysis | (from E1's rows; exploratory) | `python experiments/analysis/errors.py` | `ERROR_ANALYSIS.md`, `logs/errors_test.json` |
| E4, E6 | pending: their datasets' review and freeze (ADR-031, ADR-033) | | |

A run that fails a check is never deleted: it stays in its log directory, marked invalid (E1 chunks also go to
`INVALIDATED.json` for rule 3), and is rerun completely.
