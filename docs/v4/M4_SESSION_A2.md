# M4 session: A2 acceptance (capabilities, speed test, calibrate)

What this checks: Kestrel's A2 on the Mac (M4, 16 GB unified memory), next to the laptop's run of
2026-10-29. A2 is accepted when the states below match on the M4 (blueprint §13, phase A2: "correct
states on the laptop, the M4 and the fake probes"). Nothing here downloads a model or needs the internet
after step 1.

Time: about 30–40 minutes, most of it the first build. Keep the Mac on power and close heavy apps.

Every command runs in the repo folder on the Mac. Outputs go to one log folder:

```sh
L=experiments/logs/v4/m4/$(date +%F); mkdir -p "$L"
```

## 0. Once on this Mac

- Xcode command line tools: `xcode-select --install` (skip if `clang --version` works)
- cmake: `brew install cmake` (skip if `cmake --version` works)
- Rust: rustup from rustup.rs; the repo's `rust-toolchain.toml` picks the version
- git

## 1. The code

After the laptop's `feat/v4` is pushed:

```sh
git fetch && git checkout feat/v4 && git pull
git log --oneline -1 | tee "$L/commit.txt"
```

The commit should be 4339c77 or later.

## 2. The files git does not carry

Copy these from the laptop's repo into the same places on the Mac (USB, AirDrop, or `scp`):

| From the laptop | Size |
|---|---|
| `models/gemma-4-E2B-it-Q4_0.gguf` | 2.8 GB |
| `models/multilingual-e5-small-q8_0.gguf` | 132 MB |
| `experiments/artifacts/tier3/intents_v0-multilingual-e5-small-q8_0/` (folder) | 0.9 MB |
| `experiments/artifacts/tier3/kinds_v2-multilingual-e5-small-q8_0/` (folder) | 0.9 MB |

Check the two model files:

```sh
(cd models && grep -E 'gemma-4-E2B-it-Q4_0.gguf|multilingual-e5-small-q8_0.gguf' SHA256SUMS | shasum -a 256 -c -)
```

Both lines must say `OK`.

## 3. Build and test

```sh
time cargo test --workspace 2>&1 | tail -3 | tee "$L/tests.txt"
time cargo build --release -p kestrel-cli --features llama 2>&1 | tail -2 | tee "$L/build.txt"
```

The second one builds llama.cpp with Metal; no extra flag is needed. On macOS, Metal is on by default.
The first build takes about 5–15 minutes.

Hardware, for the log:

```sh
system_profiler SPHardwareDataType | grep -E 'Chip|Memory' | tee "$L/hardware.txt"
sysctl -n hw.memsize | tee -a "$L/hardware.txt"
```

## 4. Before measuring

```sh
K=./target/release/kestrel
$K capabilities      | tee "$L/1_capabilities.txt"
$K capabilities --hi | tee "$L/1_capabilities_hi.txt"
$K model             | tee "$L/2_model.txt"
```

## 5. The speed test (1–2 minutes)

```sh
$K capabilities --measure | tee "$L/3_measure.txt"
$K model                  | tee "$L/4_model_after_measure.txt"
```

## 6. Calibrate (about 5–10 minutes)

```sh
/usr/bin/time -l $K calibrate 2>&1 | grep -vE '^(llama|load|print_info|gguf|ggml|clip)' | tee "$L/5_calibrate.txt"
$K capabilities | tee "$L/6_capabilities_after.txt"
$K model        | tee "$L/7_model_after.txt"
cp "$HOME/Library/Application Support/kestrel/device_profile.json" "$L/"
```

## 7. Back to the laptop

Copy the folder `$L` to the laptop's repo at the same path. Claude compares it with the table
below and commits it. Only the human pushes.

## What to expect

The laptop's numbers (2026-10-29, CPU only) are there to compare against.

| # | Check | Expected on the M4 | Laptop |
|---|---|---|---|
| 1 | `shasum` | both `OK` | — |
| 2 | `cargo test --workspace` | all pass | 223 passed |
| 3 | header (step 4) | `macos, 16.0 GB RAM, GPU Apple M4 (…)`; writer gemma-4-E2B-it-Q4_0 | linux, 14.9 GB, no GPU |
| 4 | "a model may use" | about 13.0 GB: the larger of RAM − 3 GB (13.0) and the GPU share Metal reports. **Note the GPU number:** the blueprint says Apple's budget is the GPU working set. If Metal reports less than 13.0 GB, the code and the blueprint disagree, and that needs a decision. | 11.9 GB |
| 5 | Tasks, reminders, notes search | ✅ | ✅ |
| 6 | Chat, email, rewrite/translate, notes questions, summaries, before step 5 | 🔧 "run the speed test (1–2 min)" | same |
| 7 | Code check | 🔒 "the code sandbox runs on Linux only until v4 phase A4" | ✅ (bubblewrap) |
| 8 | writer line under the header, before step 6 | "not checked here yet: `kestrel calibrate`" | same |
| 9 | speed (step 5) | writing ≥ 7 tokens/s, reading ≥ 40 tokens/s (the gates) | 12.0 / 90.9 |
| 10 | writer cards after step 5 | ✅ (or "✅ slow" if within 20% of a gate) | ✅ |
| 11 | `kestrel model` after step 5 | E2B "here"; E4B and 12B "could run here (not downloaded)" if their estimated speed passes; 26B A4B and 31B "too big here" | 12B too slow, 26B/31B too big |
| 12 | calibrate: kind fallback | finishes; τ and τ_g on the grid (0.5–0.9); close to 0.8 / 0.7, T near 1.74 (Metal's numbers can differ a little) | T 1.7411, τ 0.8, τ_g 0.7, 6 wrong / 4 asked |
| 13 | calibrate: notes questions | close to 20 of 20 right and 4 of 6 left unanswered | 20/20, 4/6 |
| 14 | calibrate: code | "code: not measured (no sandbox here)" | 17/18 |
| 15 | writer line after step 6 | "checked here: notes questions …" (no code part) | with code |
| 16 | time and memory of step 6 | write down "real" and "maximum resident set size" from `time -l` | 7 min 43 s, 3.9 GB |

Anything that differs from this table: note it with the file it shows in. Don't fix it on the Mac;
the next laptop session does that.

Not this session (each needs a separate yes): downloading E4B or 12B (4.2 / 6.2 GB; their SHA-256 must
go into `models/SHA256SUMS` first), and `kestrel serve` on macOS (needs the A3 shell for a real test).
