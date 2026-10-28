//! `kestrel capabilities [--hi] [CONFIG]` (v4 A2; ADR-049, ADR-053): what Kestrel can do on this
//! device, card by card: ready, setup (with the steps) or locked (with the reasons).
//!
//! The probe: RAM and GPUs from llama.cpp's devices (a llama build; otherwise the RAM from
//! /proc/meminfo on Linux); the programs the cards need, on PATH; which catalog model files are in
//! models/; the writer = the catalog model whose file is CONFIG's tier-4 model (the service's
//! config by default). Speeds come from the first-run speed test, which is not built yet: until
//! then the writing cards ask for it. Run from the repo root, like the other commands.

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
    run_in(Path::new("."), args, out)
}

/// `run` with the repo at `root`.
pub fn run_in(root: &Path, args: &[String], out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    const USE: &str = "usage: kestrel capabilities [--hi] [CONFIG]";
    let mut hi = false;
    let mut config = None;
    for a in args {
        match a.as_str() {
            "--hi" => hi = true,
            _ if a.starts_with("--") || config.is_some() => return Err(USE.into()),
            _ => config = Some(a.as_str()),
        }
    }
    let cards = kestrel_capability::load_cards(&root.join("capabilities"))?;
    let catalog = kestrel_capability::load_catalog(&root.join("models/catalog.json"))?;
    kestrel_capability::check(&cards, &catalog)?;
    let os = Os::current();
    let config = std::fs::read_to_string(root.join(config.unwrap_or(DEFAULT_CONFIG))).ok();
    let writer = config.as_deref().and_then(writer_file);
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok();
    let backends = backends();
    let found = Found {
        os,
        backends: &backends,
        meminfo: meminfo.as_deref(),
        writer_file: writer.as_deref(),
    };
    let on_path = |t: &str| kestrel_platform::which(os, &kestrel_platform::env, t).is_some();
    let have_file = |f: &str| root.join("models").join(f).is_file();
    let d = device(found, &cards, &catalog, &on_path, &have_file);
    // Speeds: none until the first-run speed test is built (v4 A2's next step).
    let measured = kestrel_capability::Measured::new();
    let verdicts = kestrel_capability::evaluate(&cards, &catalog, &d, &measured);
    out.write_all(render(&d, &verdicts, hi).as_bytes())?;
    Ok(())
}

/// llama.cpp's devices; a build without it has none (the RAM then comes from /proc/meminfo).
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
            writer_file: Some("other.gguf"),
        };
        let d = device(found, &cards, &catalog, &|_| false, &|_| false);
        assert_eq!(d.ram_total_mib, 16_384);
        assert_eq!(d.writer, None);
        assert!(d.tools.is_empty() && d.models.is_empty());
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
    fn the_command_lists_every_shipped_card_on_this_machine() {
        let mut out = Vec::new();
        run_in(&repo(), &[], &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let mut lines = text.lines();
        assert!(
            lines
                .next()
                .unwrap()
                .starts_with("Kestrel on this device: ")
        );
        let cards = lines.filter(|l| !l.starts_with("   - ")).count();
        assert_eq!(cards, shipped().0.len());
        assert!(run_in(&repo(), &["--nope".into()], &mut Vec::new()).is_err());
        let _ = Measured::new(); // the speed test fills this later
    }
}
