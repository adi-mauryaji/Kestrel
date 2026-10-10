# Environment record (Ch. 34.4)

Recorded 2026-10-10. Copy into every experiment manifest. Rows marked *pending* are filled when that component exists.

| Field | Value |
|---|---|
| CPU model / cores / threads / base–max frequency | AMD Ryzen 5 5500U · 6 cores / 12 threads · 413.6–4056.8 MHz (scaling range) |
| RAM (size, speed) / swap setting | 14 GiB usable (speed: needs `sudo dmidecode -t memory`) · swap: 8 GiB zram, priority 100 |
| OS + kernel version | Fedora 44 · Linux 7.2.6-200.fc44.x86_64 |
| CPU governor / turbo setting | `powersave` · boost = 1 (on) — **set a fixed policy before any timing run (Ch. 37.3)** |
| Storage type | NVMe SSD (Phison ESO512GMLCT-E9C) |
| Power source (AC/battery) / cooling state | Laptop; on battery when recorded — **benchmarks on AC only** |
| Rust toolchain (`rustc -V`), `Cargo.lock` hash | rustc 1.99.0 (b940084d7 2026-09-28) · cargo 1.99.0 · lock hash *pending* |
| Python version, package lock hash | Python 3.14.7 · `requirements.lock` sha256 *pending first commit* |
| llama.cpp commit; `llama-cpp-2` exact version | `26394b4e6749` (2026-09-21) = submodule of `llama-cpp-2 =0.1.157` · CLI built by `scripts/build_llama.sh`: Release, CPU-only, `GGML_NATIVE=ON` (tuned to this CPU; experiments only) |
| Native toolchain | gcc 16.2.1 · clang 22.1.8 (+ clang-devel for bindgen) · cmake 4.3.0 |
| Model files: name, quantization, SHA-256 | `models/MANIFEST.toml` (source of truth) · verify offline: `cd models && sha256sum -c SHA256SUMS` |
| Dataset fingerprints (Ch. 32) | *pending (Week 4)* |
| Kestrel git commit; config hash | *pending* |
| Energy counter | `/sys/class/powercap/intel-rapl:0` present; `energy_uj` readable by root only (Week-2 check, Ch. 37.2) |
