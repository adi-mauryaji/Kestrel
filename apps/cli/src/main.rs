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
//! confidence against the tier's accept threshold, its time and the decision (src/ask.rs).

// Without the llama feature nothing builds tiers from the config, but its tests still run.
// The ask report is plain code with tests; only reading requests needs the models.
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod ask;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod config;
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod labels;
#[cfg(feature = "llama")]
mod route_items;

use std::process::ExitCode;

const USAGE: &str = "usage: kestrel route-items CONFIG.json ITEMS.jsonl OUT.jsonl\n       kestrel ask CONFIG.json [TEXT...]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("route-items") => route(&args[1..]),
        Some("ask") => ask(&args[1..]),
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

#[cfg(feature = "llama")]
fn route(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let [config, items, out] = args else {
        return Err("usage: kestrel route-items CONFIG.json ITEMS.jsonl OUT.jsonl".into());
    };
    route_items::run(config.as_ref(), items.as_ref(), out.as_ref())
}

#[cfg(feature = "llama")]
fn ask(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let Some((config, text)) = args.split_first() else {
        return Err(USAGE.into());
    };
    let text = text.join(" ");
    ask::run(config.as_ref(), (!text.is_empty()).then_some(text.as_str()))
}

#[cfg(not(feature = "llama"))]
fn ask(_: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    route(&[])
}

#[cfg(not(feature = "llama"))]
fn route(_: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    Err(
        "built without the llama feature: cargo build --release -p kestrel-cli --features llama"
            .into(),
    )
}
