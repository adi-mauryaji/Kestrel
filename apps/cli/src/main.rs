//! `kestrel`: the command-line entry point (blueprint Ch. 24): wires the tiers into the cascade.
//!
//!     kestrel route-items CONFIG.json ITEMS.jsonl OUT.jsonl      (needs --features llama)
//!     kestrel ask CONFIG.json [TEXT...]                          (needs --features llama)
//!
//! route-items builds the config's tiers (Tier 1 rules, Tier 3 encoder + head, Tier 4 SLM) from a
//! generated router config (experiments/router/make_config.py), routes every {"id", "text"} item
//! and writes one line per item: the decision, the trace and the wall time. It answers in the
//! config's label list (Kestrel-Intents-v0's 9 intents, CLINC150's 150 + oos; ADR-019). Run it from
//! the repository root: the config's model, head, prompt and label names are resolved under
//! models/, experiments/artifacts/tier3/ and datasets/contracts/.
//!
//! ask builds the same cascade and routes typed requests, showing each tier's answer, its
//! confidence against the tier's accept threshold, its time and the decision (src/ask.rs); with
//! memory (src/personal.rs): corrections, the memory context, stated preferences.
//!
//! bench-items is E1's inner loop (src/bench_items.rs, ADR-020): several configs (R0, R1, R3)
//! over one shared instance of each tier, every item repeated with the systems rotated.
//!
//! plan-eval is Week 9d (src/planner.rs, ADR-023): the Tier-4 planner under the plan grammar on
//! Kestrel-Tasks-v0 dev compound requests, or `--check-grammar` on every gold plan.
//!
//! memory is Week 10 (src/memory.rs): list, approve, delete and export the memory file; no model.
//!
//! e4-eval is Week 11's E4 (src/e4.rs, ADR-028): personalization off vs memory after K taught
//! aliases on Kestrel-Personas-v0, gold intents, no model.

// Without the llama feature nothing builds tiers from the config, but its tests still run.
// The ask report is plain code with tests; only reading requests needs the models.
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod act;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod ask;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod assistant;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod bench_items;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod clock;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod commands;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod config;
mod e4;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod labels;
mod memory;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod personal;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod planner;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod remind;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod remote;
#[cfg(feature = "llama")]
mod route_items;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod service;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod telemetry;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod times;

use std::process::ExitCode;

const USAGE: &str = "usage: kestrel route-items CONFIG.json ITEMS.jsonl OUT.jsonl\n       kestrel ask [--no-memory] [--notes DIR] [--brief] CONFIG.json [TEXT...]\n       kestrel bench-items ITEMS.jsonl OUT.jsonl CONFIG.json...\n       kestrel plan-eval [--check-grammar | --hybrid] [OUT.json]\n       kestrel memory [--db PATH] list | pending | tasks | approve ID | delete ID | export | purge | vacuum\n       kestrel e4-eval [--split dev|test] [--cascade CONFIG] [OUT.json]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("route-items") => route(&args[1..]),
        Some("ask") => ask(&args[1..]),
        Some("bench-items") => bench(&args[1..]),
        Some("plan-eval") => plan_eval(&args[1..]),
        Some("memory") => memory::run(&args[1..], &mut std::io::stdout()),
        Some("remind") => remind::run(&args[1..], &mut std::io::stdout()),
        Some("chat") => service::chat(None),
        Some("say") if args.len() > 1 => service::chat(Some(&args[1..].join(" "))),
        Some("serve") => serve(&args[1..]),
        Some("tier4-worker") => tier4_worker(&args[1..]),
        Some("e4-eval") => e4_eval(&args[1..]),
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kestrel: {e}");
            ExitCode::FAILURE
        }
    }
}

fn e4_eval(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    const USE: &str = "usage: kestrel e4-eval [--split dev|test] [--cascade CONFIG] [OUT.json]";
    let mut split = "dev";
    let mut cascade = None;
    let mut out = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--split" => split = it.next().ok_or(USE)?.as_str(),
            "--cascade" => cascade = Some(it.next().ok_or(USE)?.as_str()),
            _ if out.is_none() && !a.starts_with("--") => out = Some(a.as_str()),
            _ => return Err(USE.into()),
        }
    }
    if !matches!(split, "dev" | "test") {
        return Err(USE.into());
    }
    let default = match cascade {
        None => format!("experiments/logs/E4/e4_{split}.json"),
        Some(_) => format!("experiments/logs/E4/e4_{split}_cascade.json"),
    };
    let out = std::path::Path::new(out.unwrap_or(&default));
    match cascade {
        None => e4::run(split, out, None),
        Some(config) => e4_cascade(split, out, config.as_ref()),
    }
}

/// E4 with each request routed by Tier 1 + Tier 3 of `config` (ADR-028's secondary check).
#[cfg(feature = "llama")]
fn e4_cascade(
    split: &str,
    out: &std::path::Path,
    config: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use kestrel_router::{Cascade, Outcome};
    let models = route_items::Models::load_for(config, &[1, 3])?;
    let intents = labels::Intents;
    let tiers = models.tiers(&intents, |t| t, &[1, 3])?;
    let cascade = Cascade::new(tiers, models.router.clone())?;
    let route = |text: &str| match cascade.route(&kestrel_core::Request::new(0, text)).0 {
        Outcome::Accepted(p) => Some(p.intent),
        Outcome::Abstained { .. } => None,
    };
    let how = format!("Tier 1 + Tier 3 of {}", config.display());
    e4::run(split, out, Some((&route, &how)))
}

#[cfg(not(feature = "llama"))]
fn e4_cascade(
    _: &str,
    _: &std::path::Path,
    _: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    route(&[])
}

#[cfg(feature = "llama")]
fn route(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let [config, items, out] = args else {
        return Err("usage: kestrel route-items CONFIG.json ITEMS.jsonl OUT.jsonl".into());
    };
    route_items::run(config.as_ref(), items.as_ref(), out.as_ref())
}

#[cfg(feature = "llama")]
fn ask(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (mut no_memory, mut brief) = (false, false);
    let mut notes = std::env::var_os("KESTREL_NOTES")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from);
    let mut args = args;
    loop {
        match args.split_first() {
            Some((f, rest)) if f == "--no-memory" => (no_memory, args) = (true, rest),
            Some((f, rest)) if f == "--brief" => (brief, args) = (true, rest),
            Some((f, rest)) if f == "--notes" => {
                let (dir, rest) = rest.split_first().ok_or(USAGE)?;
                (notes, args) = (Some(dir.into()), rest);
            }
            _ => break,
        }
    }
    let memory = if no_memory {
        None
    } else {
        Some(memory::default_db(&|k| std::env::var_os(k))?)
    };
    let Some((config, text)) = args.split_first() else {
        return Err(USAGE.into());
    };
    let text = text.join(" ");
    ask::run(
        config.as_ref(),
        (!text.is_empty()).then_some(text.as_str()),
        memory.as_deref(),
        notes.as_deref(),
        brief,
    )
}

#[cfg(feature = "llama")]
fn bench(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let [items, out, configs @ ..] = args else {
        return Err(USAGE.into());
    };
    if configs.is_empty() {
        return Err(USAGE.into());
    }
    let configs: Vec<&std::path::Path> = configs.iter().map(|c| c.as_ref()).collect();
    bench_items::run(items.as_ref(), out.as_ref(), &configs)
}

#[cfg(feature = "llama")]
fn plan_eval(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let flag = |f: &str| args.iter().any(|a| a == f);
    let out = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(String::as_str);
    if flag("--hybrid") {
        planner::run_hybrid(
            out.unwrap_or("experiments/logs/tasks/planner_hybrid_dev.json")
                .as_ref(),
        )
    } else {
        planner::run(
            flag("--check-grammar"),
            out.unwrap_or("experiments/logs/tasks/planner_dev.json")
                .as_ref(),
        )
    }
}

#[cfg(not(feature = "llama"))]
fn plan_eval(_: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    route(&[])
}

/// `kestrel serve [--notes DIR] [--idle-minutes N] CONFIG` (ADR-034).
#[cfg(feature = "llama")]
fn serve(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut notes = std::env::var_os("KESTREL_NOTES")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from);
    let mut idle = 10u64;
    let mut args = args;
    loop {
        match args.split_first() {
            Some((f, rest)) if f == "--notes" => {
                let (dir, rest) = rest.split_first().ok_or(USAGE)?;
                (notes, args) = (Some(dir.into()), rest);
            }
            Some((f, rest)) if f == "--idle-minutes" => {
                let (n, rest) = rest.split_first().ok_or(USAGE)?;
                (idle, args) = (n.parse()?, rest);
            }
            _ => break,
        }
    }
    let [config] = args else {
        return Err(USAGE.into());
    };
    let notes = notes.ok_or("kestrel serve needs --notes DIR (or KESTREL_NOTES)")?;
    let memory = memory::default_db(&|k| std::env::var_os(k))?;
    service::serve(
        config.as_ref(),
        &notes,
        &memory,
        std::time::Duration::from_secs(idle * 60),
    )
}

#[cfg(feature = "llama")]
fn tier4_worker(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let [config] = args else {
        return Err("usage: kestrel tier4-worker CONFIG".into());
    };
    service::worker(config.as_ref())
}

#[cfg(not(feature = "llama"))]
fn serve(_: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    route(&[])
}

#[cfg(not(feature = "llama"))]
fn tier4_worker(_: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    route(&[])
}

#[cfg(not(feature = "llama"))]
fn ask(_: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    route(&[])
}

#[cfg(not(feature = "llama"))]
fn bench(_: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    route(&[])
}

#[cfg(not(feature = "llama"))]
fn route(_: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    Err(
        "built without the llama feature: cargo build --release -p kestrel-cli --features llama"
            .into(),
    )
}
