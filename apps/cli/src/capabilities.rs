//! `kestrel capabilities [--measure] [--hi] [CONFIG]` (v4 A2; ADR-049, ADR-053): what Kestrel can
//! do on this device, card by card: ready, setup (with the steps) or locked (with the reasons).
//!
//! The probe: RAM and GPUs from llama.cpp's devices (a llama build; otherwise the RAM from
//! /proc/meminfo on Linux); the programs the cards need, on PATH; which catalog model files are in
//! models/; the writer = the catalog model whose file is CONFIG's tier-4 model (the service's
//! config by default). Speeds: `--measure` runs the speed test (the writer loaded as the service
//! loads it; reading and writing timed) and keeps them in device_profile.json in Kestrel's data
//! folder, valid while the model file and the binary stay the same. Run from the repo root, like
//! the other commands.

use std::error::Error;
use std::io::Write;
use std::path::Path;

use kestrel_capability::{Card, Catalog, Device, Gpu, Verdict};
use kestrel_inference::BackendDevice;
use kestrel_platform::Os;

/// The installed service's config (scripts/install_service.sh).
pub const DEFAULT_CONFIG: &str = "experiments/configs/router_intents_v0.json";

/// RAM (MiB) and GPUs from llama.cpp's devices.
pub fn from_backends(os: Os, devices: &[BackendDevice]) -> (Option<u64>, Vec<Gpu>) {
    use kestrel_capability::GpuKind;
    use kestrel_inference::DeviceKind;
    let mib = |bytes: u64| bytes >> 20;
    let ram = devices
        .iter()
        .find(|d| d.kind == DeviceKind::Cpu)
        .map(|d| mib(d.memory_total));
    let gpus = devices
        .iter()
        .filter(|d| matches!(d.kind, DeviceKind::Gpu | DeviceKind::IntegratedGpu))
        .map(|d| Gpu {
            name: match d.description.is_empty() {
                true => d.backend.clone(),
                false => d.description.clone(),
            },
            memory_mib: mib(d.memory_total),
            // Apple's GPU shares the unified memory; Metal reports the share it may use.
            kind: if os == Os::MacOs && d.backend.contains("Metal") {
                GpuKind::AppleUnified
            } else if d.kind == DeviceKind::IntegratedGpu {
                GpuKind::Integrated
            } else {
                GpuKind::Discrete
            },
        })
        .collect();
    (ram, gpus)
}

/// The tier-4 model file a router config names.
pub fn writer_file(config: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(config).ok()?;
    v["models"]["tier4"]["model_file"]
        .as_str()
        .map(String::from)
}

/// What the probe read from the machine.
pub struct Found<'a> {
    pub os: Os,
    pub backends: &'a [BackendDevice],
    /// /proc/meminfo (Linux).
    pub meminfo: Option<&'a str>,
    /// Physical memory in MiB from the OS (macOS, Windows), for a build without llama's devices.
    pub os_memory_mib: Option<u64>,
    pub writer_file: Option<&'a str>,
}

/// The device as the cards see it: `on_path` finds a program, `have_file` a model file.
pub fn device(
    found: Found<'_>,
    cards: &[Card],
    catalog: &Catalog,
    on_path: &dyn Fn(&str) -> bool,
    have_file: &dyn Fn(&str) -> bool,
) -> Device {
    let (ram, gpus) = from_backends(found.os, found.backends);
    let ram = ram.or_else(|| {
        found
            .meminfo
            .and_then(kestrel_platform::parse_meminfo)
            .map(|(total, _)| total)
    });
    let ram = ram.or(found.os_memory_mib);
    let mut tools: Vec<String> = cards
        .iter()
        .flat_map(|c| c.needs.tools.iter().cloned())
        .filter(|t| on_path(t))
        .collect();
    tools.sort();
    tools.dedup();
    let writer = found.writer_file.and_then(|f| {
        catalog
            .models
            .iter()
            .find(|m| m.file == f && m.role == kestrel_capability::Role::Writer)
            .map(|m| m.id.clone())
    });
    Device {
        os: found.os,
        ram_total_mib: ram.unwrap_or(0),
        gpus,
        tools,
        models: catalog
            .models
            .iter()
            .filter(|m| have_file(&m.file))
            .map(|m| m.id.clone())
            .collect(),
        writer,
        internet: false, // the setting comes with the net process (v4 phase C)
        setup_done: vec![],
    }
}

/// A header with the device, then a line per card and its steps or reasons below it.
pub fn render(d: &Device, verdicts: &[Verdict], hi: bool) -> String {
    use kestrel_capability::{GpuKind, State, size};
    let pick = |t: &kestrel_capability::Text| if hi { t.hi.clone() } else { t.en.clone() };
    let gpus = match d.gpus.is_empty() {
        true => (if hi { "GPU nahi" } else { "no GPU" }).to_string(),
        false => d
            .gpus
            .iter()
            .map(|g| match g.kind {
                GpuKind::Integrated if hi => format!("GPU {} (RAM baant ke)", g.name),
                GpuKind::Integrated => format!("GPU {} (shares the RAM)", g.name),
                _ => format!("GPU {} ({})", g.name, size(g.memory_mib)),
            })
            .collect::<Vec<_>>()
            .join(", "),
    };
    let budget = size(kestrel_capability::memory_budget_mib(d));
    let ram = size(d.ram_total_mib);
    let os = d.os.label();
    let mut s = match (hi, &d.writer) {
        (false, w) => format!(
            "Kestrel on this device: {os}, {ram} RAM, {gpus}; a model may use {budget}; writer {}\n",
            w.as_deref().unwrap_or("none chosen yet")
        ),
        (true, w) => format!(
            "Is device pe Kestrel: {os}, {ram} RAM, {gpus}; model ko {budget} mil sakti hai; writer {}\n",
            w.as_deref().unwrap_or("abhi koi nahi")
        ),
    };
    for v in verdicts {
        let name = pick(&v.name);
        let (mark, below) = match &v.state {
            State::Ready { slow: false } => (format!("✅ {name}"), &[][..]),
            State::Ready { slow: true } => (
                format!("✅ {name} ({})", if hi { "dheema" } else { "slow" }),
                &[][..],
            ),
            State::Setup { steps } => (format!("🔧 {name}"), &steps[..]),
            State::Locked { reasons } => (format!("🔒 {name}"), &reasons[..]),
        };
        s.push_str(&mark);
        s.push('\n');
        for t in below {
            s.push_str(&format!("   - {}\n", pick(t)));
        }
    }
    s
}

pub fn run(args: &[String], out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let data = kestrel_platform::data_dir(Os::current(), &kestrel_platform::env);
    run_in(Path::new("."), data.as_deref(), args, out)
}

/// (bytes, modified as s since 1970) of a file.
pub fn file_id(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let at = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some((m.len(), at.as_secs()))
}

/// What Kestrel can do here: the device as probed and a verdict per card.
pub struct Assessment {
    pub device: Device,
    pub verdicts: Vec<Verdict>,
    /// How the writer was chosen ("automatic", "your choice", "your file").
    pub how: Option<&'static str>,
    /// The model the running service writes with (its config's), which can differ from the
    /// chosen one until `kestrel calibrate`.
    pub service_writer: Option<String>,
}

/// Everything the cards are judged with on this machine.
pub struct Context {
    pub cards: Vec<Card>,
    /// The catalog, with the user's own GGUF when the model slot names one.
    pub catalog: Catalog,
    /// The probe, its writer the model slot's (src/slot.rs).
    pub device: Device,
    pub measured: kestrel_capability::Measured,
    pub choice: kestrel_capability::Choice,
    pub how: Option<&'static str>,
    /// What the service writes with (`service_writer`).
    pub service: Option<ServiceWriter>,
    /// `kestrel calibrate`'s entries that still hold here (same model file and binary).
    pub calibrated: Vec<kestrel_capability::Calibrated>,
}

/// Probes this machine and reads the kept speeds and the model slot from `data` (no measuring).
/// `config` is the router config the service runs (its writer is the service's).
pub fn context(
    root: &Path,
    data: Option<&Path>,
    config: Option<&str>,
) -> Result<Context, Box<dyn Error>> {
    let cards = kestrel_capability::load_cards(&root.join("capabilities"))?;
    let mut catalog = kestrel_capability::load_catalog(&root.join("models/catalog.json"))?;
    kestrel_capability::check(&cards, &catalog)?;
    let settings = crate::slot::load(data);
    if let crate::slot::Writer::File { path } = &settings.writer
        && let Some((bytes, _)) = file_id(Path::new(path))
    {
        catalog
            .models
            .push(crate::slot::file_model(Path::new(path), bytes));
    }
    let os = Os::current();
    let config = std::fs::read_to_string(root.join(config.unwrap_or(DEFAULT_CONFIG))).ok();
    let writer = config.as_deref().and_then(writer_file);
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok();
    let backends = backends();
    // Only when nothing else knows the RAM: it costs a process on macOS / Windows.
    let os_memory_mib = if backends.is_empty() && meminfo.is_none() {
        os_memory(os)
    } else {
        None
    };
    let found = Found {
        os,
        backends: &backends,
        meminfo: meminfo.as_deref(),
        os_memory_mib,
        writer_file: writer.as_deref(),
    };
    let on_path = |t: &str| kestrel_platform::which(os, &kestrel_platform::env, t).is_some();
    let have_file = |f: &str| crate::slot::model_path(root, data, f).is_file();
    let mut device = device(found, &cards, &catalog, &on_path, &have_file);
    let config_writer = device.writer.take();
    let model_file = |id: &str| {
        let m = catalog.models.iter().find(|m| m.id == id)?;
        file_id(&crate::slot::model_path(root, data, &m.file))
    };
    let (profile, binary) = (load_profile(data), this_binary());
    let measured = profile.measured(&model_file, binary);
    let gates = kestrel_capability::writer_gates(&cards);
    let choice = kestrel_capability::choose(&catalog, &device, &measured, &gates);
    let chosen = crate::slot::effective(&settings, &choice);
    device.writer = chosen.as_ref().map(|(id, _)| id.clone());
    let service = service_writer(device.writer.as_deref(), config_writer.as_deref(), &|m| {
        profile.calibration(m, &model_file, binary).cloned()
    });
    let calibrated = (catalog.models.iter())
        .filter_map(|m| profile.calibration(&m.id, &model_file, binary).cloned())
        .collect();
    Ok(Context {
        cards,
        catalog,
        device,
        measured,
        choice,
        how: chosen.map(|(_, how)| how),
        service,
        calibrated,
    })
}

/// Probes this machine and evaluates the cards with the speeds kept in `data` (no measuring).
// The service's page (a llama build) asks for it; the tests use it everywhere.
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
pub fn assess(
    root: &Path,
    data: Option<&Path>,
    config: Option<&str>,
) -> Result<Assessment, Box<dyn Error>> {
    let c = context(root, data, config)?;
    let verdicts = kestrel_capability::evaluate(&c.cards, &c.catalog, &c.device, &c.measured);
    Ok(Assessment {
        device: c.device,
        verdicts,
        how: c.how,
        service_writer: c.service.map(|w| w.model),
    })
}

/// Under the header: whether `kestrel calibrate` has checked the writer on this device.
pub fn writer_check(
    writer: Option<&str>,
    calibrated: &[kestrel_capability::Calibrated],
    hi: bool,
) -> Option<String> {
    let w = writer?;
    let found = match (calibrated.iter().find(|c| c.model == w), hi) {
        (Some(c), _) => crate::pass_rates::summary(c, hi),
        (None, false) => "not checked here yet: `kestrel calibrate`".into(),
        (None, true) => "yahan abhi check nahi hua: `kestrel calibrate` chalao".into(),
    };
    Some(format!("   writer {w}: {found}"))
}

/// What the service writes with, and the kind fallback's numbers when this device measured them.
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceWriter {
    /// Catalog id.
    pub model: String,
    /// `kestrel calibrate`'s entry for it; None: the committed fallback file's numbers, which
    /// hold only for the model the file was measured on (the config's).
    pub calibrated: Option<kestrel_capability::Calibrated>,
    /// The slot's writer while it waits for `kestrel calibrate`.
    pub waiting: Option<String>,
}

/// The slot's writer (`chosen`) once `kestrel calibrate` has measured it here; until then the
/// config's. Routing is the same with any writer: the router's Tier 4 is its last tier, so its
/// confidence decides nothing; only the kind fallback's τ_g is per model (blueprint §4.3).
pub fn service_writer(
    chosen: Option<&str>,
    config: Option<&str>,
    calibration: &dyn Fn(&str) -> Option<kestrel_capability::Calibrated>,
) -> Option<ServiceWriter> {
    if let Some(c) = chosen
        && let Some(k) = calibration(c)
    {
        return Some(ServiceWriter {
            model: c.into(),
            calibrated: Some(k),
            waiting: None,
        });
    }
    let config = config?;
    Some(ServiceWriter {
        model: config.into(),
        calibrated: calibration(config),
        waiting: chosen.filter(|&c| c != config).map(String::from),
    })
}

/// When the chosen writer waits for `kestrel calibrate`, the service keeps its config's model.
pub fn service_note(c: &Context) -> Option<String> {
    let w = c.service.as_ref()?;
    let waiting = w.waiting.as_deref()?;
    Some(format!(
        "The service still writes with {}: switching to {waiting} waits for `kestrel calibrate`.",
        w.model
    ))
}

/// The page's `GET /api/capabilities`: the device in words and each card's state with its notes.
// The service (a llama build) serves it; the tests use it everywhere.
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
pub fn to_json(a: &Assessment) -> serde_json::Value {
    use kestrel_capability::{GpuKind, State, size};
    use serde_json::json;
    let d = &a.device;
    let note = |t: &kestrel_capability::Text| json!({"en": t.en, "hi": t.hi});
    let gpus: Vec<_> = d
        .gpus
        .iter()
        .map(|g| {
            let kind = match g.kind {
                GpuKind::Discrete => "discrete",
                GpuKind::AppleUnified => "apple",
                GpuKind::Integrated => "integrated",
            };
            json!({"name": g.name, "memory": size(g.memory_mib), "kind": kind})
        })
        .collect();
    let cards: Vec<_> = a
        .verdicts
        .iter()
        .map(|v| {
            let (state, notes) = match &v.state {
                State::Ready { slow: false } => ("ready", &[][..]),
                State::Ready { slow: true } => ("slow", &[][..]),
                State::Setup { steps } => ("setup", &steps[..]),
                State::Locked { reasons } => ("locked", &reasons[..]),
            };
            json!({
                "id": v.id,
                "name": note(&v.name),
                "state": state,
                "notes": notes.iter().map(note).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "device": {
            "os": d.os.label(),
            "ram": size(d.ram_total_mib),
            "budget": size(kestrel_capability::memory_budget_mib(d)),
            "writer": d.writer,
            "writer_how": a.how,
            "service_writer": a.service_writer,
            "gpus": gpus,
        },
        "cards": cards,
    })
}

pub fn load_profile(data: Option<&Path>) -> kestrel_capability::Profile {
    data.and_then(|d| std::fs::read_to_string(d.join("device_profile.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// The running kestrel binary, as a speed's validity is keyed.
pub fn this_binary() -> (u64, u64) {
    std::env::current_exe()
        .ok()
        .and_then(|e| file_id(&e))
        .unwrap_or((0, 0))
}

/// `run` with the repo at `root` and the profile in `data` (none: speeds are not kept).
pub fn run_in(
    root: &Path,
    data: Option<&Path>,
    args: &[String],
    out: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    const USE: &str = "usage: kestrel capabilities [--measure] [--hi] [CONFIG]";
    let (mut hi, mut measure) = (false, false);
    let mut config = None;
    for a in args {
        match a.as_str() {
            "--hi" => hi = true,
            "--measure" => measure = true,
            _ if a.starts_with("--") || config.is_some() => return Err(USE.into()),
            _ => config = Some(a.as_str()),
        }
    }
    if measure {
        measure_and_keep(root, data, config, out)?;
    }
    let c = context(root, data, config)?;
    let verdicts = kestrel_capability::evaluate(&c.cards, &c.catalog, &c.device, &c.measured);
    // The writer's check goes under the header, before the cards.
    let text = render(&c.device, &verdicts, hi);
    let (header, cards) = text.split_once('\n').unwrap_or((&text, ""));
    writeln!(out, "{header}")?;
    if let Some(line) = writer_check(c.device.writer.as_deref(), &c.calibrated, hi) {
        writeln!(out, "{line}")?;
    }
    out.write_all(cards.as_bytes())?;
    if let Some(note) = service_note(&c) {
        writeln!(out, "{note}")?;
    }
    Ok(())
}

/// The speed test for this device's writer, kept in `data`'s profile.
fn measure_and_keep(
    root: &Path,
    data: Option<&Path>,
    config: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    let catalog = kestrel_capability::load_catalog(&root.join("models/catalog.json"))?;
    let config = std::fs::read_to_string(root.join(config.unwrap_or(DEFAULT_CONFIG))).ok();
    let n_ctx = config
        .as_deref()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
        .and_then(|v| v["models"]["tier4"]["n_ctx"].as_u64())
        .unwrap_or(2048) as u32;
    let file_name = config
        .as_deref()
        .and_then(writer_file)
        .ok_or("no writer model here to measure")?;
    let model = catalog
        .models
        .iter()
        .find(|m| m.file == file_name)
        .ok_or("the config's writer is not in the catalog")?;
    let path = crate::slot::model_path(root, data, &model.file);
    let file = file_id(&path).ok_or_else(|| format!("{} is missing", path.display()))?;
    let id = model.id.clone();
    writeln!(out, "measuring {id} (about a minute)...")?;
    let (speed, threads) = measure_speed(&path, n_ctx)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mut profile = load_profile(data);
    profile.record(kestrel_capability::Entry {
        model: id.clone(),
        file,
        binary: this_binary(),
        decode_tok_s: speed.decode_tok_s,
        prefill_tok_s: speed.prefill_tok_s,
        threads,
        measured_at: now,
    });
    writeln!(
        out,
        "{id}: writes {:.1} tokens/s, reads {:.0} tokens/s ({threads} threads)",
        speed.decode_tok_s, speed.prefill_tok_s
    )?;
    let dir = data.ok_or("no data folder to keep the speeds in")?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("device_profile.json"),
        serde_json::to_string_pretty(&profile)? + "\n",
    )?;
    Ok(())
}

/// About 300 tokens of plain prose to read: the prefill half of the speed test.
#[cfg(feature = "llama")]
const READING: &str = "The kestrel is a small falcon that hovers in the wind above open fields, \
     its head perfectly still while its wings beat fast, watching the grass below for the movement \
     of a mouse or a beetle. When it sees one it drops in short steps, stops, hovers again, and \
     only then falls the last few metres with its feet forward. Farmers have long liked it, \
     because a pair raising young can take thousands of voles in a summer. It nests in old crow \
     nests, in holes in trees and cliffs, and more and more on buildings in towns, where ledges \
     and boxes put up for it take the place of rock. In winter the birds of the north move south, \
     while those of milder places stay all year near the same few fields. A kestrel sees \
     ultraviolet light, and the trails that voles leave with their urine shine in it, so the bird \
     can tell from the air where its prey has been running. It is not fast like a peregrine in a \
     dive; it is patient, and its whole way of hunting is to wait, look and hang in the air at no \
     cost, using the wind the way a sailor uses a sail. Watching one for a few minutes on a windy \
     day is one of the quiet pleasures of a walk in open country.";

/// The speed test (v4 A2): the writer loaded as the service loads it (mapped, the config's
/// context, the adapter's threads); a short warm-up answer, then reading `READING` (prefill)
/// and writing up to 64 tokens of a long answer (decode). (speed, threads).
#[cfg(feature = "llama")]
fn measure_speed(
    model: &Path,
    n_ctx: u32,
) -> Result<(kestrel_capability::Speed, i32), Box<dyn Error>> {
    use kestrel_core::resources::{CancelToken, Deadline};
    use kestrel_inference::llama::{LlamaConfig, LlamaSlm};
    use kestrel_inference::prompt::{ChatFormat, ChatPrompt};
    use kestrel_inference::{GenParams, Generator};
    let cfg = LlamaConfig {
        n_ctx,
        mmap: true,
        ..LlamaConfig::default()
    };
    let threads = cfg.n_threads;
    let slm = LlamaSlm::load(model, cfg)?;
    let format = ChatFormat::for_architecture(&slm.architecture()?)
        .ok_or("no chat format known for this model")?;
    let chat = ChatPrompt::new(format, "You are a helpful assistant.");
    let ask = |user: &str| format!("{}{}", chat.prefix(), chat.suffix(user));
    let (cancel, deadline) = (
        CancelToken::new(),
        Deadline::after(std::time::Duration::from_secs(600)),
    );
    // Warm-up: the weights into memory, the threads started.
    slm.session()?.generate(
        &ask("Say hi."),
        &GenParams::greedy(8),
        &cancel,
        &deadline,
        &mut |_| {},
    )?;
    let mut reading = slm.session()?;
    let t = std::time::Instant::now();
    let n = reading.prime(READING)?;
    let prefill = n as f64 / t.elapsed().as_secs_f64();
    let stats = slm.session()?.generate(
        &ask("Write the numbers from one to forty in words, separated by commas."),
        &GenParams::greedy(64),
        &cancel,
        &deadline,
        &mut |_| {},
    )?;
    let decode = stats
        .decode_tokens_per_s()
        .filter(|_| stats.tokens >= 16)
        .ok_or("the model stopped too early to time its writing")?;
    Ok((
        kestrel_capability::Speed {
            decode_tok_s: decode,
            prefill_tok_s: prefill,
        },
        threads,
    ))
}

#[cfg(not(feature = "llama"))]
fn measure_speed(_: &Path, _: u32) -> Result<(kestrel_capability::Speed, i32), Box<dyn Error>> {
    Err("the speed test needs the llama build: cargo build --release -p kestrel-cli --features llama".into())
}

/// The OS's own answer for the physical memory (sysctl, PowerShell); None where it cannot say.
fn os_memory(os: Os) -> Option<u64> {
    let out = kestrel_platform::memory_total_command(os)?.output().ok()?;
    kestrel_platform::parse_memory_total(&String::from_utf8_lossy(&out.stdout))
}

/// llama.cpp's devices; a build without it has none (the RAM then comes from the OS).
#[cfg(feature = "llama")]
fn backends() -> Vec<BackendDevice> {
    kestrel_inference::llama::backend_devices().unwrap_or_default()
}

#[cfg(not(feature = "llama"))]
fn backends() -> Vec<BackendDevice> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_capability::{GpuKind, Measured, State, Text};
    use kestrel_inference::DeviceKind;

    const GIB: u64 = 1 << 30;

    fn dev(backend: &str, description: &str, kind: DeviceKind, gib: u64) -> BackendDevice {
        BackendDevice {
            backend: backend.into(),
            description: description.into(),
            kind,
            memory_total: gib * GIB,
            memory_free: gib * GIB,
        }
    }

    fn repo() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn the_cpu_device_is_the_ram_and_gpus_come_by_kind() {
        let cpu = dev("CPU", "AMD Ryzen 5 5500U", DeviceKind::Cpu, 16);
        assert_eq!(
            from_backends(Os::Linux, std::slice::from_ref(&cpu)),
            (Some(16_384), vec![])
        );
        let (ram, gpus) = from_backends(
            Os::Windows,
            &[
                cpu.clone(),
                dev("Vulkan", "NVIDIA GeForce RTX 4060", DeviceKind::Gpu, 8),
                dev(
                    "Vulkan",
                    "AMD Radeon Graphics",
                    DeviceKind::IntegratedGpu,
                    4,
                ),
                dev("BLAS", "OpenBLAS", DeviceKind::Accelerator, 0),
            ],
        );
        assert_eq!(ram, Some(16_384));
        assert_eq!(
            gpus,
            [
                Gpu {
                    name: "NVIDIA GeForce RTX 4060".into(),
                    memory_mib: 8_192,
                    kind: GpuKind::Discrete
                },
                Gpu {
                    name: "AMD Radeon Graphics".into(),
                    memory_mib: 4_096,
                    kind: GpuKind::Integrated
                },
            ]
        );
        // Apple's GPU is Metal's, sharing the unified memory, whatever type llama.cpp gives it.
        for kind in [DeviceKind::Gpu, DeviceKind::IntegratedGpu] {
            let (_, gpus) = from_backends(
                Os::MacOs,
                &[cpu.clone(), dev("Metal", "Apple M4", kind, 11)],
            );
            assert_eq!(gpus[0].kind, GpuKind::AppleUnified);
        }
        assert_eq!(from_backends(Os::Linux, &[]), (None, vec![]));
    }

    #[test]
    fn the_writer_is_the_configs_tier4_model() {
        let cfg = r#"{"models": {"tier4": {"model_file": "gemma-4-E2B-it-Q4_0.gguf"}}}"#;
        assert_eq!(
            writer_file(cfg).as_deref(),
            Some("gemma-4-E2B-it-Q4_0.gguf")
        );
        assert_eq!(writer_file(r#"{"models": {}}"#), None);
        assert_eq!(writer_file("not json"), None);
        let service = std::fs::read_to_string(repo().join(DEFAULT_CONFIG)).unwrap();
        assert_eq!(
            writer_file(&service).as_deref(),
            Some("gemma-4-E2B-it-Q4_0.gguf")
        );
    }

    fn shipped() -> (Vec<Card>, Catalog) {
        (
            kestrel_capability::load_cards(&repo().join("capabilities")).unwrap(),
            kestrel_capability::load_catalog(&repo().join("models/catalog.json")).unwrap(),
        )
    }

    #[test]
    fn the_device_is_put_together_from_what_was_found() {
        let (cards, catalog) = shipped();
        let meminfo = "MemTotal: 15675420 kB\nMemAvailable: 8297732 kB\n";
        let found = Found {
            os: Os::Linux,
            backends: &[],
            meminfo: Some(meminfo),
            os_memory_mib: Some(1),
            writer_file: Some("gemma-4-E2B-it-Q4_0.gguf"),
        };
        let d = device(found, &cards, &catalog, &|t| t == "bwrap", &|f| {
            f == "gemma-4-E2B-it-Q4_0.gguf"
        });
        assert_eq!(d.os, Os::Linux);
        assert_eq!(
            d.ram_total_mib, 15_308,
            "from /proc/meminfo without llama's devices"
        );
        assert_eq!(d.tools, ["bwrap"], "the cards' programs that are on PATH");
        assert_eq!(d.models, ["gemma-4-E2B-it-Q4_0"]);
        assert_eq!(d.writer.as_deref(), Some("gemma-4-E2B-it-Q4_0"));
        assert!(!d.internet && d.setup_done.is_empty());
        // llama.cpp's CPU device wins over /proc/meminfo; an unknown model file is no writer.
        let backends = [dev("CPU", "x", DeviceKind::Cpu, 16)];
        let found = Found {
            os: Os::Linux,
            backends: &backends,
            meminfo: Some(meminfo),
            os_memory_mib: Some(1),
            writer_file: Some("other.gguf"),
        };
        let d = device(found, &cards, &catalog, &|_| false, &|_| false);
        assert_eq!(d.ram_total_mib, 16_384);
        assert_eq!(d.writer, None);
        assert!(d.tools.is_empty() && d.models.is_empty());
        // A non-llama build on macOS / Windows: no backends, no /proc/meminfo, only the OS's answer.
        let found = Found {
            os: Os::MacOs,
            backends: &[],
            meminfo: None,
            os_memory_mib: Some(16_384),
            writer_file: None,
        };
        let d = device(found, &cards, &catalog, &|_| false, &|_| false);
        assert_eq!(d.ram_total_mib, 16_384);
    }

    fn text(en: &str, hi: &str) -> Text {
        Text {
            en: en.into(),
            hi: hi.into(),
        }
    }

    fn verdict(id: &str, en: &str, hi: &str, state: State) -> Verdict {
        Verdict {
            id: id.into(),
            name: text(en, hi),
            state,
        }
    }

    #[test]
    fn the_list_reads_like_the_page_will() {
        let d = Device {
            os: Os::Linux,
            ram_total_mib: 15_308,
            gpus: vec![Gpu {
                name: "AMD Radeon Graphics".into(),
                memory_mib: 4_096,
                kind: GpuKind::Integrated,
            }],
            tools: vec![],
            models: vec![],
            writer: Some("gemma-4-E2B-it-Q4_0".into()),
            internet: false,
            setup_done: vec![],
        };
        let v = [
            verdict("tasks", "Tasks", "Tasks", State::Ready { slow: false }),
            verdict("chat", "Chat", "Baatcheet", State::Ready { slow: true }),
            verdict(
                "email.draft",
                "Email drafts",
                "Email draft",
                State::Setup {
                    steps: vec![text(
                        "run the speed test (1–2 min)",
                        "speed test chalao (1–2 min)",
                    )],
                },
            ),
            verdict(
                "code.check",
                "Code check",
                "Code check",
                State::Locked {
                    reasons: vec![text("Linux only", "sirf Linux")],
                },
            ),
        ];
        assert_eq!(
            render(&d, &v, false),
            "Kestrel on this device: linux, 14.9 GB RAM, GPU AMD Radeon Graphics (shares the RAM); \
             a model may use 11.9 GB; writer gemma-4-E2B-it-Q4_0\n\
             ✅ Tasks\n\
             ✅ Chat (slow)\n\
             🔧 Email drafts\n   - run the speed test (1–2 min)\n\
             🔒 Code check\n   - Linux only\n"
        );
        let none = Device {
            gpus: vec![],
            writer: None,
            ..d
        };
        assert_eq!(
            render(&none, &v[..2], true),
            "Is device pe Kestrel: linux, 14.9 GB RAM, GPU nahi; model ko 11.9 GB mil sakti hai; \
             writer abhi koi nahi\n\
             ✅ Tasks\n\
             ✅ Baatcheet (dheema)\n"
        );
    }

    #[test]
    fn the_page_gets_the_device_in_words_and_each_cards_state() {
        let a = Assessment {
            device: Device {
                os: Os::Windows,
                ram_total_mib: 7_987,
                gpus: vec![Gpu {
                    name: "NVIDIA GeForce RTX 4060".into(),
                    memory_mib: 8_192,
                    kind: GpuKind::Discrete,
                }],
                tools: vec![],
                models: vec![],
                writer: None,
                internet: false,
                setup_done: vec![],
            },
            verdicts: vec![
                verdict("tasks", "Tasks", "Tasks", State::Ready { slow: false }),
                verdict("chat", "Chat", "Baatcheet", State::Ready { slow: true }),
                verdict(
                    "email.draft",
                    "Email drafts",
                    "Email draft",
                    State::Setup {
                        steps: vec![text("choose a model", "model chuno")],
                    },
                ),
                verdict(
                    "code.check",
                    "Code check",
                    "Code check",
                    State::Locked {
                        reasons: vec![text("Linux only", "sirf Linux")],
                    },
                ),
            ],
            how: None,
            service_writer: None,
        };
        let note = |en: &str, hi: &str| serde_json::json!({"en": en, "hi": hi});
        assert_eq!(
            to_json(&a),
            serde_json::json!({
                "device": {
                    "os": "windows", "ram": "7.8 GB", "budget": "7.5 GB", "writer": null,
                    "writer_how": null, "service_writer": null,
                    "gpus": [{"name": "NVIDIA GeForce RTX 4060", "memory": "8.0 GB", "kind": "discrete"}]
                },
                "cards": [
                    {"id": "tasks", "name": note("Tasks", "Tasks"), "state": "ready", "notes": []},
                    {"id": "chat", "name": note("Chat", "Baatcheet"), "state": "slow", "notes": []},
                    {"id": "email.draft", "name": note("Email drafts", "Email draft"), "state": "setup",
                     "notes": [note("choose a model", "model chuno")]},
                    {"id": "code.check", "name": note("Code check", "Code check"), "state": "locked",
                     "notes": [note("Linux only", "sirf Linux")]}
                ]
            })
        );
    }

    #[test]
    fn the_model_slot_sets_the_writer_and_a_note_says_when_the_service_differs() {
        use crate::slot::{Settings, Writer, save};
        let root = repo();
        let data = std::env::temp_dir().join(format!("kestrel_slotcap_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        let e2b = "gemma-4-E2B-it-Q4_0";
        let c = context(&root, Some(&data), None).unwrap();
        assert_eq!(
            c.service.as_ref().map(|w| w.model.as_str()),
            Some(e2b),
            "the service's config"
        );
        if root.join("models").join(format!("{e2b}.gguf")).is_file() {
            assert_eq!(c.device.writer.as_deref(), Some(e2b));
            assert_eq!(c.how, Some("automatic"));
            assert_eq!(service_note(&c), None);
        }
        let e4b = "gemma-4-E4B-it-Q4_0";
        let chosen = Settings {
            writer: Writer::Catalog { id: e4b.into() },
            ..Settings::default()
        };
        save(&data, &chosen).unwrap();
        let c = context(&root, Some(&data), None).unwrap();
        assert_eq!(c.device.writer.as_deref(), Some(e4b));
        assert_eq!(c.how, Some("your choice"));
        assert!(
            service_note(&c)
                .unwrap()
                .contains(&format!("still writes with {e2b}"))
        );
        let a = assess(&root, Some(&data), None).unwrap();
        let chat = a.verdicts.iter().find(|v| v.id == "chat").unwrap();
        // Whether E4B is a download here depends on this machine's memory (a CI runner may be small).
        let catalog = kestrel_capability::load_catalog(&root.join("models/catalog.json")).unwrap();
        let need = catalog
            .models
            .iter()
            .find(|m| m.id == e4b)
            .unwrap()
            .memory_mib;
        let have_file = root.join("models/gemma-4-E4B-it-Q4_0.gguf").is_file();
        if kestrel_capability::memory_budget_mib(&c.device) >= need && !have_file {
            assert_eq!(
                chat.state,
                State::Setup {
                    steps: vec![text(
                        &format!("download {e4b} (4.2 GB)"),
                        &format!("{e4b} download karo (4.2 GB)")
                    )]
                }
            );
        } else if !have_file {
            assert!(
                !matches!(&chat.state, State::Setup { steps } if steps[0].en.starts_with("download")),
                "too big for this device is no download: {:?}",
                chat.state
            );
        }
        let _ = std::fs::remove_dir_all(&data);
    }

    fn calibrated(model: &str, tau_g: f32) -> kestrel_capability::Calibrated {
        kestrel_capability::Calibrated {
            model: model.into(),
            file: (1, 1),
            binary: (1, 1),
            temperature: 1.7,
            tau: 0.8,
            tau_g,
            val_n: 152,
            val_wrong: 4,
            val_asked: 7,
            measured_at: 1,
            notes_qa: None,
            code_tasks: None,
        }
    }

    #[test]
    fn the_service_switches_to_the_slot_s_writer_only_once_it_is_calibrated_here() {
        let (e2b, e4b) = ("gemma-4-E2B-it-Q4_0", "gemma-4-E4B-it-Q4_0");
        let none = |_: &str| None;
        let both = |m: &str| Some(calibrated(m, if m == e4b { 0.6 } else { 0.7 }));
        let only = |id: &'static str| move |m: &str| (m == id).then(|| calibrated(m, 0.6));
        // The config's own writer: the committed fallback file, or this device's measurement.
        assert_eq!(
            service_writer(Some(e2b), Some(e2b), &none),
            Some(ServiceWriter {
                model: e2b.into(),
                calibrated: None,
                waiting: None
            })
        );
        assert_eq!(
            service_writer(Some(e2b), Some(e2b), &both)
                .unwrap()
                .calibrated,
            Some(calibrated(e2b, 0.7))
        );
        // Another writer waits for `kestrel calibrate`; then it runs with its own numbers.
        assert_eq!(
            service_writer(Some(e4b), Some(e2b), &none),
            Some(ServiceWriter {
                model: e2b.into(),
                calibrated: None,
                waiting: Some(e4b.into())
            })
        );
        assert_eq!(
            service_writer(Some(e4b), Some(e2b), &only(e4b)),
            Some(ServiceWriter {
                model: e4b.into(),
                calibrated: Some(calibrated(e4b, 0.6)),
                waiting: None
            })
        );
        assert_eq!(
            service_writer(Some(e4b), Some(e2b), &only(e2b)).unwrap(),
            ServiceWriter {
                model: e2b.into(),
                calibrated: Some(calibrated(e2b, 0.6)),
                waiting: Some(e4b.into())
            }
        );
        // No writer here: the config's; no config writer either: none.
        assert_eq!(service_writer(None, Some(e2b), &none).unwrap().model, e2b);
        assert_eq!(service_writer(None, None, &both), None);
        assert_eq!(
            service_writer(Some(e4b), None, &none),
            None,
            "uncalibrated, nothing to keep"
        );
    }

    #[test]
    fn a_calibrated_gguf_of_the_user_s_becomes_the_service_s_writer_and_the_note_goes() {
        use crate::slot::{Settings, Writer, save};
        let root = repo();
        let data = std::env::temp_dir().join(format!("kestrel_svcwriter_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        let gguf = data.join("my-writer.gguf");
        std::fs::write(&gguf, b"GGUF not really").unwrap();
        let chosen = Settings {
            writer: Writer::File {
                path: gguf.to_string_lossy().into_owned(),
            },
            ..Settings::default()
        };
        save(&data, &chosen).unwrap();
        let c = context(&root, Some(&data), None).unwrap();
        let w = c.service.as_ref().unwrap();
        assert_eq!(w.model, "gemma-4-E2B-it-Q4_0", "not calibrated yet");
        assert_eq!(w.waiting.as_deref(), Some("my-writer"));
        assert!(
            service_note(&c)
                .unwrap()
                .contains("waits for `kestrel calibrate`")
        );
        let mut profile = kestrel_capability::Profile::default();
        profile.calibrate(kestrel_capability::Calibrated {
            file: file_id(&gguf).unwrap(),
            binary: this_binary(),
            ..calibrated("my-writer", 0.6)
        });
        std::fs::write(
            data.join("device_profile.json"),
            serde_json::to_string(&profile).unwrap(),
        )
        .unwrap();
        let c = context(&root, Some(&data), None).unwrap();
        let w = c.service.as_ref().unwrap();
        assert_eq!(
            (w.model.as_str(), w.waiting.as_deref()),
            ("my-writer", None)
        );
        assert_eq!(w.calibrated.as_ref().unwrap().tau_g, 0.6);
        assert_eq!(service_note(&c), None);
        // Its file changes: measured again first.
        std::fs::write(&gguf, b"GGUF another file").unwrap();
        let c = context(&root, Some(&data), None).unwrap();
        assert_eq!(c.service.unwrap().waiting.as_deref(), Some("my-writer"));
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_line_under_the_header_says_whether_the_writer_was_checked_here() {
        let k = calibrated("e2b", 0.7);
        assert_eq!(
            writer_check(Some("e2b"), std::slice::from_ref(&k), false).unwrap(),
            "   writer e2b: checked here: the request kinds only; `kestrel calibrate` again adds \
             notes and code"
        );
        assert_eq!(
            writer_check(Some("e4b"), std::slice::from_ref(&k), false).unwrap(),
            "   writer e4b: not checked here yet: `kestrel calibrate`"
        );
        assert_eq!(
            writer_check(Some("e4b"), &[], true).unwrap(),
            "   writer e4b: yahan abhi check nahi hua: `kestrel calibrate` chalao"
        );
        assert_eq!(writer_check(None, &[k], false), None);
    }

    #[test]
    fn the_command_lists_every_shipped_card_on_this_machine() {
        let mut out = Vec::new();
        run_in(&repo(), None, &[], &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let mut lines = text.lines();
        assert!(
            lines
                .next()
                .unwrap()
                .starts_with("Kestrel on this device: ")
        );
        let mut lines = lines.peekable();
        // With a writer here, its check comes before the cards.
        lines.next_if(|l| l.starts_with("   writer "));
        let cards = lines.filter(|l| !l.starts_with("   - ")).count();
        assert_eq!(cards, shipped().0.len());
        assert!(run_in(&repo(), None, &["--nope".into()], &mut Vec::new()).is_err());
    }

    #[test]
    fn a_kept_speed_unlocks_the_writing_cards_while_the_model_and_binary_stay() {
        let root = repo();
        let catalog = kestrel_capability::load_catalog(&root.join("models/catalog.json")).unwrap();
        let e2b = catalog
            .models
            .iter()
            .find(|m| m.role == kestrel_capability::Role::Writer)
            .unwrap();
        let Some(file) = file_id(&root.join("models").join(&e2b.file)) else {
            return; // a checkout without the model (CI): nothing to unlock
        };
        let binary = file_id(&std::env::current_exe().unwrap()).unwrap();
        let data = std::env::temp_dir().join(format!("kestrel_profile_{}", std::process::id()));
        std::fs::create_dir_all(&data).unwrap();
        let mut p = kestrel_capability::Profile::default();
        p.record(kestrel_capability::Entry {
            model: e2b.id.clone(),
            file,
            binary,
            decode_tok_s: 12.8,
            prefill_tok_s: 58.5,
            threads: 6,
            measured_at: 0,
        });
        std::fs::write(
            data.join("device_profile.json"),
            serde_json::to_string(&p).unwrap(),
        )
        .unwrap();
        let mut out = Vec::new();
        run_in(&root, Some(&data), &[], &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("✅ Chat\n"), "{text}");
        assert!(!text.contains("run the speed test"), "{text}");
        // Another binary (a new build) measures again.
        p.entries[0].binary = (1, 1);
        std::fs::write(
            data.join("device_profile.json"),
            serde_json::to_string(&p).unwrap(),
        )
        .unwrap();
        let mut out = Vec::new();
        run_in(&root, Some(&data), &[], &mut out).unwrap();
        assert!(
            String::from_utf8(out)
                .unwrap()
                .contains("run the speed test")
        );
        let _ = std::fs::remove_dir_all(&data);
        let _ = Measured::new();
    }

    #[cfg(not(feature = "llama"))]
    #[test]
    fn the_speed_test_needs_the_llama_build() {
        let root = repo();
        let have_model = root.join("models/gemma-4-E2B-it-Q4_0.gguf").is_file();
        let e = run_in(&root, None, &["--measure".into()], &mut Vec::new()).unwrap_err();
        let want = if have_model { "llama build" } else { "missing" };
        assert!(e.to_string().contains(want), "{e}");
    }
}
