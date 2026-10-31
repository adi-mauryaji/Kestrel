# Kestrel v4: product blueprint and architecture

Status: **PROPOSED** (ADR-052). Product decisions behind it: ADR-049, ADR-050, ADR-051 (accepted). Date 2026-10-28. Branch `feat/v4` (from v3 @ea59303).

How to read: §2 lists the rules every later section obeys, and §10 gives the build order. **(meas.)** marks a number measured on the Linux laptop (Ryzen 5 5500U, 14 GB RAM, no GPU). **(est.)** marks an estimate that the capability system must measure before anything depends on it. **(eval)** marks a choice that is made only after a short evaluation recorded in `experiments/logs/v4/`.

---

## 1. The product

**Who:** people with personal documents, notes and routines on a normal laptop. The first pilot is students and acquaintances, 10–20 people for two weeks (ADR-051).

**Promise:** *"ChatGPT knows the world. Kestrel knows you, and tells no one."*

**Kestrel sits next to ChatGPT, it does not replace it.** Users keep their cloud AI for general knowledge and give Kestrel their life: documents, notes, reminders, mail. In India strong cloud AI is cheap or free: ChatGPT Go costs ₹399/month and Plus ₹1,999, and Jio users got Google AI Pro free for 18 months. Kestrel therefore cannot win on intelligence or price per answer. It wins where cloud AI is the wrong tool:

| Where Kestrel wins | Why cloud AI is weaker there |
|---|---|
| Private documents (ID cards, bank statements, medical reports, client files) | Cloud assistants that read your mail and files do it on the company's servers |
| Proactive help (reminders, expiry dates, morning brief, bill dates) | A chat box waits to be asked |
| No or bad internet (trains, villages, exam halls) | Needs the network |
| ₹0 for good, no limits | Free offers end; plans cost monthly |
| Confidential work (CA, lawyer, doctor, HR) | Sending client data to an AI service is a compliance question under the DPDP Rules (main obligations from about May 2027); this is not legal advice |

**What would make people give up, and what this blueprint does about it:**

| Reason to give up | Answer here |
|---|---|
| Hard to install | One installer per OS, onboarding, model download with progress (§4.10) |
| Doesn't run on Windows | Linux, macOS and Windows from one codebase (§4.1, ADR-051) |
| Makes the laptop slow | Model loaded on demand and unloaded when idle; budgets in §7 |
| Slow answers | Instant routing, streaming, cached prompt prefix (§7) |
| Confidently wrong | Grounding check, sources, saying "not sure" (§4.6, §4.7) |
| "It can't do X" with no reason | The capability list says what is locked and what device would unlock it (§4.2) |
| Data lost | Export and backup (§6) |

**Success = measured, not assumed.** The pilot (§10) tracks days used per week, reminders, tasks and documents added, the things users could not have done without Kestrel, and the "how disappointed would you be without it" question. 40% or more answering "very disappointed" is the bar.

**Not in v4:** a phone app (Tauri 2 can target Android and iOS later; that is a separate decision); devices lending each other models (option B, ADR-049); voice and photo; any cloud LLM; any payment made by Kestrel; IRCTC automation; a free-roaming browser agent.

---

## 2. Rules (non-negotiable)

1. **No data to an AI company.** No note, document, memory or conversation is sent to any AI model or API. Models run on the device. (ADR-049)
2. **The core has no network.** Every internet request goes through one separate process, `kestrel-net`, and every request is logged and shown on the page. The core binary links no network-client crate. Its only socket client is a loopback-only one for local model servers (§4.3).
3. **Nothing leaves without a yes.** Every outbound action (mail, form, upload) shows a preview with the exact text and recipient and waits for y/N. Web text is data and never instructions (§4.8).
4. **Capabilities are unlocked by measurement.** States are ✅ ready, 🔧 possible here once a named setup step is done, and 🔒 not on this device, with the reason and the device it needs. A model that fits in memory but is too slow, or fails the task set, stays locked. (ADR-049)
5. **One codebase for Linux, macOS and Windows.** OS differences live only in the platform layer (§4.1). (ADR-051)
6. **Light on the device.** The big model is loaded on demand and unloaded after idle. The idle footprint and the latencies in §7 are tested.
7. **₹0.** Free and open-source tools only. Anything paid (signing certificates, APIs) is named with its cost and asked first.
8. **The research stays untouched.** Frozen tags, research datasets and PREREG integrity are kept: calibration uses dev splits only, never test splits. Research branches keep the ADR-006/007 pins.

---

## 3. System overview

```
 ┌─────────────────────────── one device ───────────────────────────────────────┐
 │                                                                              │
 │  Kestrel app (Tauri 2 shell)          kestrel CLI (chat / say)               │
 │  window · tray · autostart ·               │                                 │
 │  notifications · onboarding                │                                 │
 │         │  HTTP 127.0.0.1 + token          │                                 │
 │         ▼                                  ▼                                 │
 │  ┌──────────────────── kestrel serve (core, no network) ──────────────────┐  │
 │  │ capability registry · router (rules + e5 + kind head) · skills          │  │
 │  │ memory + conversations (SQLite) · notes & documents index · reminders   │  │
 │  │ outbound log · approval gate · page + API                               │  │
 │  └───────┬──────────────────────┬───────────────────────┬──────────────────┘  │
 │          │ stdio JSON lines     │ in-process            │ stdio JSON lines    │
 │          ▼                      ▼                       ▼                     │
 │  model worker            code sandbox             kestrel-net (only network)  │
 │  llama.cpp (CPU /        wasmtime +               search · fetch · weather ·  │
 │  Metal / Vulkan) or      python.wasm              RSS · SMTP · model download │
 │  loopback local server   (no net, one folder)     allowlist, every call logged │
 └──────────────────────────────────────────────────────────────────────────────┘
                         ✗ nothing to any AI company
```

**Why separate processes:**
- **RAM:** the model worker exits after idle. v3 already does this; v3's worker is ready again in 2.0 s from a saved prefix (meas.).
- **Crash isolation:** a crash in llama.cpp or the PDF parser does not take down reminders.
- **An auditable network boundary:** `cargo tree` of the core stays free of network crates.
- **The research binary keeps its behaviour.**

---

## 4. Components

### 4.1 Platform layer (new crate `kestrel-platform`)
Every OS difference goes here. This is what exists today in v3 and what replaces it:

| Concern | v3 (Linux only), file | v4 Linux | macOS | Windows |
|---|---|---|---|---|
| Data / config / cache dirs | XDG variables (`memory.rs`, `service.rs`) | XDG | `~/Library/Application Support`, `Caches` | `%APPDATA%`, `%LOCALAPPDATA%` |
| Local transport | Unix socket + loopback HTTP (`service.rs`, `web.rs`) | loopback HTTP + token; Unix socket kept for the CLI | same | loopback HTTP + token only |
| Secret files | 0600 under `cfg(unix)` | 0600 | 0600 | per-user profile directory (user-only ACL) |
| Secrets (mail password) | none | OS keyring (Secret Service) | Keychain | Credential Manager |
| Local time | runs `date` (`clock.rs`) | a time-zone crate (eval) | same | same |
| Notifications | `notify-send` (`remind.rs`) | the shell, via `/api/events` | the shell | the shell |
| Autostart / supervision | systemd user unit + timer (`control.rs`, `install_service.sh`) | the shell; the systemd unit stays for headless use | the shell (LaunchAgent) | the shell (login item) |
| Code sandbox | bubblewrap + `/usr/bin/python3` (`sandbox.rs`) | wasmtime + python.wasm (§4.9) | same | same |
| Process memory | `/proc/self/status` (`kestrel-bench/rss.rs`) | `/proc` | OS API | OS API |
| GPU backend | none | Vulkan | Metal | Vulkan |

**Path safety is tested on each OS:** `..`, absolute paths and symlinks everywhere; on Windows also `..\`, drive letters, UNC paths and junctions.

### 4.2 Device probe and capability registry (new crate `kestrel-capability`)

**Probe** (runs at every start; cheap):
- OS, version, architecture
- CPU model, physical cores, SIMD (AVX2, AVX-512, NEON, i8mm)
- total and free RAM, unified memory (Apple)
- GPUs with total and free memory, read from llama.cpp's backend device list, i.e. the same code that will run the model
- free disk in the data dir
- which models and tools are present (python.wasm, keyring)
- whether internet features are switched on

**Capability cards:** one JSON file per capability in `capabilities/` (JSON, not TOML: serde_json is already here, ADR-053). A card lists what the capability needs:
- OS
- other cards that must be ready
- memory
- internet
- setup steps
- measured gates

Example:
```json
{
  "id": "notes.conversation",
  "name": {"en": "Long conversation over notes", "hi": "Notes pe lambi baat"},
  "needs": {"cards": ["notes.search"], "writer": true},
  "gates": [{"measure": "decode_tok_s", "min": 7.0}, {"measure": "prefill_tok_s", "min": 40.0}]
}
```

**Evaluation (pure function; tested first with fake probes):**
- **🔒** when an OS or hardware need fails, or a gate fails even with the best model this device can hold.
- **🔧** when the hardware is fine but a step is missing: a model not downloaded, a password not set, internet off. The missing step is named.
- **✅** otherwise. "✅ slow" is shown when a gate passes by less than 20%.

**Measurements:**
- **First run:** a 1–2 min speed test (load, prefill, decode) using the existing `kestrel-bench`.
- **`kestrel calibrate`:** run per model on dev task sets. It produces the kind-fallback τ_g on Kinds-v2 dev, and pass rates on NotesQA dev, CodeTasks-v0 dev and a new small Tools-v0 set for agent features.

Results go to `device_profile.json` in the data dir, keyed by model SHA-256 and binary version. They are re-measured when either changes. Free RAM is checked again when a model loads: if it is low, the user sees a notice and llama.cpp fits what it can.

**Shown on:** the page's "What Kestrel can do here" section, `GET /api/capabilities`, and `kestrel capabilities`.

### 4.3 Model slot and catalog
`models/catalog.json` lists each model with its file, SHA-256, size, licence, estimated memory and a quality reference. Gemma 4 sizes and multi-step tool-use scores (Tau2, from the model card):

| Model | Q4 file | Tau2 | Expected home |
|---|---|---|---|
| Gemma 4 E2B Q4_0 | 2.84 GB (meas.) | 24.5% | 8–16 GB laptops (default) |
| Gemma 4 E4B | ~5 GB (est.) | 42.2% | 16 GB laptops, 6 GB+ GPUs |
| Gemma 4 12B | ~7 GB (est.) | 69.0% | M4 16 GB (GPU share ~11–12 GB), 24 GB+ laptops |
| Gemma 4 26B A4B (MoE, 3.8B active) | ~15 GB (est.) | 68.2% | 32 GB+ |
| Gemma 4 31B | ~18 GB (est.) | 76.9% | 48 GB+ or a 24 GB GPU |

**Default model:** the largest catalog model that fits the budget and passes the speed gate. The budget is RAM minus an OS reserve and 2 GB of headroom; on Apple machines it is the GPU working set. The user can override the choice in Settings with any GGUF file, or with a **local** OpenAI-compatible server (llama-server, Ollama, LM Studio) on 127.0.0.1. That server is reached through a std-only loopback HTTP client that refuses any non-loopback address, so rule 2 holds.

**Backends shipped:**
- CPU everywhere
- Metal on macOS
- Vulkan on Linux and Windows
- CUDA not shipped in v4 (toolkit size, build cost)

**Routing stays fixed on every device:** rules + e5 + the kind head, which is tiny. Changing the writer model re-runs `calibrate` for the kind fallback. If calibration fails, Kestrel asks the user instead of guessing.

**Downloads:** first-run download goes through `kestrel-net` (logged), is resumable, and is SHA-256-checked. **Import from file** is also offered (USB, slow internet). The e5 encoder (127 MB) ships inside the installer, so the core works offline straight after install.

**Crate version:** v4 needs a newer `llama-cpp-2` than the research pin 0.1.157 (crates.io shows 0.1.160). Newer versions bring Gemma 4 audio/image, auto-fit to free memory, and current Metal/Vulkan. The bump happens only on `feat/v4`, after the v3 worker tests pass byte-identical on the laptop (eval). Research branches keep the pin.

### 4.4 Router and skills
v3 is kept as it is:
- 7 kinds: app, chat, code, email, notes, rewrite, translate
- τ = 0.8, Gemma fallback, ask when unsure
- Hinglish routed through English for meaning search (ADR-040)

v4 adds requests the head has never seen: documents ("mera PAN kahan hai"), conversation, send ("bhej do"), web search, weather and news. Covering them needs **Kinds-v3**: data drafted from real and pilot requests, reviewed, with dev/test splits, and a fresh one-time test, the same process as v3.

### 4.5 Memory and conversations
The SQLite memory file stays one file per user (rusqlite bundled; tasks, aliases, preferences as today). New tables:
- `conversations` (id, topic, folder)
- `turns` (conversation, n, role, text, time)
- `summaries` (conversation, up to turn, text)
- `documents` (path, kind, title, expires_on, sha256)
- `chunks` (document, n, text, embedding)
- `outbound` (time, kind, host, what was sent, approved, result)

Migrations are versioned. `kestrel memory export` (exists) covers the new tables.

### 4.6 Notes and documents index
- The folder(s) the user picks are indexed by meaning with e5 (v3's `meaning.rs`), rescanned on change.
- PDFs are read as text through a cross-platform Rust PDF crate (eval). pdf.js in the shell's webview is the fallback (already in CAPABILITY_REGISTRY).
- **Documents** ("kaagaz kahan hai"): the kind (Aadhaar, PAN, passport, licence, insurance, marksheet, bill) and the expiry date are found by rules first, then by the model. The user confirms with y/N before a date is stored. Reminders fire 30 days and 7 days before expiry.
- Answers about notes and documents go through v3's grounding check (`grounding.rs`). An answer that is not grounded says so.

### 4.7 Long conversation engine
The prompt has a budget per device, taken from the measured prefill speed. On this laptop Gemma prefills 58–80 tok/s (meas.), so 8K tokens ≈ 2 min before the first word. That is why the budget is small, and why the whole context window is not used: Gemma E2B also scores only 19.1% on a 128K long-context test.

Prompt = fixed prefix (KV state reused, v3's `prime_cached`) + rolling summary of older turns + the last turns verbatim + the closest note chunks. The summary is refreshed in the background when the conversation passes a turn threshold.

Test: in a 20+ turn conversation, a fact from turn 2 is still used correctly, and every notes claim is grounded.

### 4.8 Network process `kestrel-net` (new binary)
- **Start:** started by the core on demand and stopped when idle. It talks JSON lines over stdio, like v3's model worker.
- **Allowlisted operations:**
  - `search(query)` through a **search provider slot**: SearXNG (ADR-049) or a provider the user configures
  - `fetch(url)`
  - `weather(lat, lon)` through Open-Meteo (free, no key, non-commercial)
  - `rss(feed)`
  - `smtp_send(message, approval)` through Gmail SMTP with an app password from the keyring
  - `download(model)`
- **Log first:** the core writes the outbound log *before* dispatch, with host, kind and the exact query or recipient and subject. The result is added after. The page shows the log.
- **Approval tokens:** an outbound action carries a one-time approval token from the y/N gate. `kestrel-net` refuses an action without one.
- **Injection rules** (the "lethal trifecta": private data + untrusted content + a way out):
  - A turn that read fetched text is *tainted*.
  - Fetched text is quoted as data in prompts, and its instructions are never followed.
  - In a tainted turn, an outbound action shows the full payload and recipient before y/N.
  - Recipients come only from the user's contacts.
  - Note content is never added to a search query automatically.
- **Internet is off at install.** Onboarding asks, and Settings can change it.
- **Open item, before Phase D:** SearXNG needs Docker or Python, which is fine on Linux but heavy for normal Windows and macOS users. Options are a SearXNG instance the user trusts, or a user-supplied key for a paid API such as Brave (~$5/month credit; paid, so not a default). Until one is chosen, web search shows 🔧 there.

### 4.9 Code sandbox (new crate `kestrel-sandbox`)
CPython built for WASI (`python.wasm`, standard library only) runs inside wasmtime, with the same behaviour on all three OSes. The limits match v3's bubblewrap:
- one preopened work folder
- no sockets: WASI preview 1 has none, and the WASI build has no `socket` and no `subprocess`
- 512 MiB of memory through a resource limiter
- a 10 s CPU budget through fuel or epochs
- capped output

Before it replaces v3's sandbox, it must reach the same result as bubblewrap on CodeTasks-v0 dev (eval). Pin a wasmtime version without known path-handling advisories (one was published for 49.0.0).

### 4.10 Desktop shell (`apps/desktop`, Tauri 2)
**What it is:** a thin shell, with the core as a sidecar binary per target triple:
- a window that shows the existing page from the core's loopback URL
- tray
- autostart at login
- single instance
- notifications for reminders and expiries, received from `GET /api/events`
- onboarding: pick the notes folder → internet on/off → probe → model download or import → speed test → capability list
- Settings: model, internet, mail, search provider, export

**Installers:** Tauri's bundler builds `.msi` / NSIS `.exe` (Windows), `.dmg` (macOS), and AppImage, `.deb` and `.rpm` (Linux). The installer holds the binaries, e5 and python.wasm. Gemma is downloaded or imported after install.

**Updates:** the Tauri updater with its own signing key (free), GitHub Releases as the host. **Off by default** (it is a network call outside `kestrel-net`). Enabling it is the user's choice and is shown in the log.

**Signing:**
- The pilot ships unsigned, with written steps: on macOS, System Settings → Privacy & Security → Open Anyway (once); on Windows, the SmartScreen "More info → Run anyway".
- Windows code signing is free through SignPath Foundation if Kestrel is open source.
- Apple notarization needs the Apple Developer Program ($99/year, paid: ask).

### 4.11 API (loopback HTTP, token)
**Existing:** `GET /`, `GET /api/tasks`, `POST /api/say` (streamed JSON lines), `POST /api/answer`. The Host check, CSP and the token kept out of the URL sent to the server all stay as in v3.

**New:**
- `GET /api/device`
- `GET /api/capabilities` and `POST /api/capabilities/refresh`
- `GET /api/events?after=N` (due reminders now; expiries and downloads later): answered at once, asked every few seconds by the app; each event kept 10 min. Not a long poll: the service answers one request at a time (built 2026-10-31, `apps/cli/src/events.rs`)
- `GET /api/conversations`, `GET /api/documents`, `GET /api/outbound`
- `GET` / `POST /api/settings`

### 4.12 Pilot telemetry
v3's content-free records (`telemetry.rs`) gain daily counters: skills used, reminders, documents, capability states. They are stored locally only. A participant may export the counts file and send it. Nothing is sent automatically. Participants give written consent if the pilot feeds the paper.

---

## 5. Capability catalog (v4 and v4.1)

All entries are estimates. The capability system replaces them with measurements on each device. "Typical Windows" = 8 GB RAM, no GPU.

| Capability | Needs | This laptop (14 GB) | M4 16 GB | Typical Windows |
|---|---|---|---|---|
| Tasks, reminders | core | ✅ | ✅ | ✅ |
| Notes search by meaning | core + e5 | ✅ | ✅ | ✅ |
| Documents + expiry | core + writer model | ✅ | ✅ | ✅ slow? |
| Long conversation over notes | writer + gates | ✅ (E2B) | ✅ (12B?) | ✅ slow? |
| Mail draft | writer | ✅ | ✅ | ✅ |
| Mail send | mail draft + internet + app password | 🔧 | 🔧 | 🔧 |
| Weather reminders, news, morning brief | internet | 🔧 | 🔧 | 🔧 |
| Web search + summary | internet + search provider | 🔧 (~1 min/page) | 🔧 | 🔧 (provider open) |
| Code check (small scripts) | writer + python.wasm | ✅ | ✅ | ✅ |
| Ticket "prepare + file" | writer + documents | ✅ | ✅ | ✅ |
| Research assistant (v4.1) | web search + writer gates | ✅ slow | ✅ | ✅ slow? |
| "Why this answer" (v4.1) | core | ✅ | ✅ | ✅ |
| Phone via Tailscale (v4.1) | Tailscale installed | 🔧 | 🔧 | 🔧 |
| Browser agent (allowed sites only) | Tools-v0 gate (Tau2-class model) | 🔒 | measure (12B) | 🔒 |
| Voice, photo | newer crate + audio/vision model | 🔒 (not in v4) | 🔒 | 🔒 |

---

## 6. Data, security, privacy

**Where data lives:** the per-OS data dir (§4.1). That covers the memory file, indexes, `device_profile.json`, the outbound log and models. The notes folder stays wherever the user keeps it.

**Secrets:** in the OS keyring only. Never in files or logs; v3's canary test is extended to the mail password.

**Threats covered:**
- **Web pages:** loopback only by default, Host check and token against other sites and DNS rebinding.
- **Other users of the same computer:** per-user dirs and permissions.
- **Fetched content:** the injection rules of §4.8.
- **Model and file integrity:** SHA-256 checks on downloads.
- **Hostile PDFs:** the parser runs in the core with Rust memory safety. Moving it into the sandbox is a later option.

**Not covered in v4:** malware running as the same user; disk encryption (SQLCipher stays a later option); the LAN page (off by default, ADR-047).

**Backup:** `kestrel memory export` plus a copy of the notes folder. The page links to both.

**Privacy claim, restated for v4:** "No note, document, memory or conversation is sent to any AI model or API. Internet use happens only through one process, only for the listed operations, and every request is logged on the device." It is checked on Linux with `unshare -n`: the core passes all tests with no network. On every OS it is checked with a `cargo tree` audit of the core and the outbound-log tests.

---

## 7. Performance budgets (tested)

| What | Target | Reference |
|---|---|---|
| Routing decision | ≤ 0.1 s | e5 pass ~10 ms (meas.) |
| Worker ready (warm, saved prefix) | ≤ 3 s | 2.0 s (meas., v3) |
| First words of a chat answer (warm) | ≤ 2 s | — |
| Mail draft (~400 tokens) | ≤ 60 s on the slowest ✅ device | ~30 s at 12.8 tok/s (meas. speed) |
| Model unload after idle | 10 min (setting) | v3 |
| Idle memory (no model loaded) | measure first; target ≤ 400 MB | — |
| Model resident (E2B, mmap) | ~1.1 GiB pinned + 2.7 GiB reclaimable file pages | (meas.) |
| Default download | ≤ 3 GB | E2B 2.84 GB |
| CPU when idle | ~0 | — |

---

## 8. Code layout, build, release

```
crates/   kestrel-core, -nlu, -router, -inference, -memory, -tools, -learning, -bench   (existing)
          kestrel-platform      NEW  OS differences (§4.1)
          kestrel-capability    NEW  probe, cards, evaluation, profile (§4.2)
          kestrel-sandbox       NEW  wasmtime + python.wasm (§4.9)
apps/     cli      `kestrel`      existing: serve, chat, say, workers, calibrate, capabilities
          net      `kestrel-net`  NEW  the only network binary (§4.8)
          desktop  Tauri 2 shell  NEW  (§4.10)
capabilities/*.json               NEW  one card per capability
models/catalog.json               NEW  model ladder
```

**Features:** `llama` (as today), plus `metal` and `vulkan` passed through to `llama-cpp-2`.

**CI:** GitHub Actions matrix on Linux, macOS and Windows: fmt, clippy, and tests without models on every push. Installers are built on a tag. Model-backed tests run on the laptop and in the weekly Mac session.

**CI cost:** free for a public repo. A private repo gets 2,000 min/month, and macOS minutes cost ~10× Linux. CI runs only when the human pushes.

**The weekly Mac session:** a checklist prepared on the laptop: pull, build with Metal, probe, speed test, `calibrate`, the capability list compared with expectations, logs committed.

---

## 9. Testing

- **Test-first** for every new module, as in v3.
- **Capability evaluation with fake probes:** 8 GB Windows with no GPU, 14 GB Linux, 16 GB M4, 32 GB + 8 GB GPU. Each case asserts the exact ✅ / 🔧 / 🔒 states and their reasons.
- **Platform tests per OS:** paths and path safety, dirs, keyring round trip, loopback API, the notification event stream.
- **Network boundary:** a `cargo tree` audit of the core on all OSes. On Linux, the core suite under `unshare -n`. Every outbound path shows up in the log, and an action without approval is refused.
- **Sandbox parity:** wasmtime and bubblewrap on CodeTasks-v0 dev.
- **Acceptance run per phase and per OS before a freeze,** as in v3 (ADR-048). The page is checked headless and headed.
- **The pilot** (§10) is the last test.

---

## 10. Build order

Sizes: S ≈ a v3 step, M ≈ 2–3 steps, L ≈ a v3 stage. Each phase ends with its exit check. Research comes first from the week of 2026-11-01, and heavy runs are never run together with it.

| Phase | Content | Exit check |
|---|---|---|
| **A1** (M) | Platform layer; CI matrix; v3 tests passing on Linux, macOS and Windows | green CI on 3 OSes; the laptop service unchanged |
| **A2** (M) | Part 1: probe, cards, evaluation, speed test, model slot + catalog, `calibrate`, page section, `kestrel capabilities` | correct states on the laptop, the M4 and the fake probes |
| **A3** (M) | Tauri shell, onboarding, events, installers for 3 OSes | a fresh install on each OS reaches the capability list |
| **A4** (S) | wasmtime + python.wasm sandbox | parity with bubblewrap on CodeTasks-v0 dev |
| **B** (M) | Part 2: long conversation + PDFs; documents + expiry (v4.1 #1, pulled forward for the pilot); Kinds-v3 first round | 20-turn memory test; expiry reminders; grounded answers |
| **Pilot** | students and acquaintances, 10–20 people, 2 weeks, unsigned installers | metrics of §1; the "very disappointed" share |
| **C** (M) | Part 3: `kestrel-net`, outbound log, approval tokens, Gmail send | nothing leaves without y/N; every request in the log |
| **D** (M) | Part 4: search provider slot, weather, RSS, morning brief, weather reminders | answers with sources; injection tests pass |
| **E** (S) | Part 5: ticket "prepare + file" | the PDF filed and a reminder set from a real booking |
| **F** (M) | v4.1: phone via Tailscale, "why this answer", research assistant | per card |

---

## 11. New dependencies (proposals: CAPABILITY_REGISTRY entry and the human's approval before install)

| Dependency | For | Why this one | Phase |
|---|---|---|---|
| Tauri 2 + plugins (autostart, single-instance, notification, updater) | desktop shell, installers | Rust, small, one bundler for 3 OSes, mobile later | A3 |
| wasmtime + wasmtime-wasi, CPython WASI build | code sandbox on 3 OSes | same isolation everywhere, no OS sandbox per platform | A4 |
| a keyring crate | mail password in the OS keyring | Secret Service / Keychain / Credential Manager | C |
| a dirs crate, a time-zone crate (eval) | platform layer | replaces XDG-only paths and `date` | A1 |
| ~~a system-info crate~~ (not needed, ADR-053) | probe: RAM, CPU, OS | llama.cpp's device list gives RAM and GPUs; /proc/meminfo, std for the rest | A2 |
| a file-watcher crate (eval) | notes index rescans | cross-platform change events | B |
| a Rust PDF text crate (eval) | PDFs and documents | `pdftotext` is not on Windows or macOS | B |
| an HTTP client with rustls; an SMTP crate (`kestrel-net` only) | fetch, search, mail | kept out of the core | C |
| `llama-cpp-2` newer than 0.1.157 (`feat/v4` only) | Metal/Vulkan, auto-fit, Gemma 4 | research keeps the pin | A2 |
| SearXNG (where it can run) | search | free, self-hosted | D |
| Tailscale (user installs) | phone access | free personal plan, encrypted | F |

---

## 12. Risks and open questions

| # | Item | Owner / when |
|---|---|---|
| 1 | **No Windows machine known for testing**: CI covers tests, not the real UI | human: is there a Windows PC to borrow? before A3 |
| 2 | **Repo public or private?** It decides CI cost and SignPath eligibility (open source only) | human, before A1 CI |
| 3 | **Web search on Windows/macOS** for normal users (§4.8) | decide before D |
| 4 | **macOS signing is paid** ($99/year); unsigned means "Open Anyway" once | human, before a wider release |
| 5 | **Windows build of llama.cpp** through the crate (clang, MSVC, Vulkan SDK in CI) | A1/A2, CI first |
| 6 | **8 GB Windows laptops:** the writer model may be 🔒 or slow | measured in A2; the pilot shows real devices |
| 7 | **A 2.84 GB download on slow internet** | resumable download + import from file |
| 8 | **Hinglish quality** of new skills | Kinds-v3 review + the pilot |
| 9 | **Prompt injection** is not fully solvable; the y/N gate is the real defence | tests in D |
| 10 | **The Mac is available weekly:** macOS issues surface late | weekly checklist (§8) |

---

## 13. Decisions

- **ADR-049:** mission, any device, ✅ / 🔧 / 🔒 by measurement, option A, Parts 1–5, SearXNG, Gmail.
- **ADR-050:** the v4.1 list; the Mac is an M4 with 16 GB.
- **ADR-051:** Linux, macOS and Windows; Kestrel sits next to cloud AI, not in place of it; the pilot (students, after Part 2 + documents).
- **ADR-052:** this architecture (PROPOSED until the human accepts). Its main choices are processes + a loopback API, the platform layer, capability cards with measured gates, the model slot with a loopback-only client, `kestrel-net` as the only network path, the WASI sandbox, the Tauri shell, phases A–F.

Sources for outside facts: Gemma 4 model card (ai.google.dev/gemma/docs/core/model_card_4); llama.cpp multimodal and auto-fit docs; IRCTC terms (contents.irctc.co.in); Simon Willison, "The lethal trifecta" (2025-06-16); Open-Meteo (open-meteo.com/en/about); Tauri 2 docs (v2.tauri.app); CPython `Platforms/WASI`; GitHub Actions pricing write-ups (2026); SignPath Foundation (signpath.org); Apple developer news on Sequoia Gatekeeper; ChatGPT India pricing and Jio–Google AI Pro reports (2026); DPDP Rules (PIB, 2025-11-13).
