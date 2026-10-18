//! `kestrel`: the command-line entry point (blueprint Ch. 24): wires the tiers into the cascade.
//!
//!     kestrel route-items CONFIG.json ITEMS.jsonl OUT.jsonl      (needs --features llama)
//!
//! route-items builds Tier 1 (rules), Tier 3 (encoder + head) and Tier 4 (SLM) from a generated
//! router config (experiments/router/make_config.py), routes every {"id", "text"} item and writes
//! one line per item: the decision, the trace and the wall time. Run it from the repository root:
//! the config's model, head and prompt names are resolved under models/, experiments/artifacts/
//! tier3/ and datasets/contracts/.

// Without the llama feature nothing builds tiers from the config, but its tests still run.
#[cfg_attr(not(feature = "llama"), allow(dead_code))]
mod config;
#[cfg(feature = "llama")]
mod route_items;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("route-items") => route(&args[1..]),
        _ => Err("usage: kestrel route-items CONFIG.json ITEMS.jsonl OUT.jsonl".into()),
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

#[cfg(not(feature = "llama"))]
fn route(_: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    Err(
        "built without the llama feature: cargo build --release -p kestrel-cli --features llama"
            .into(),
    )
}
