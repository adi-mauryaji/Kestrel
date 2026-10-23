//! Week 10 E: the lexical retrieval baseline's recall@k on Kestrel-Memory-v0 (ADR-026).
//!
//!     cargo run -p kestrel-memory --example recall_eval -- [--split dev|test] [OUT.json]
//!
//! Run from the repository root. Seeds an in-memory store with build/store.jsonl, ranks the
//! split's queries with the default weights (Ch. 18.1) at the dataset's `now`, and writes the
//! summary and every query's ranking to OUT (default experiments/logs/memory/recall_<split>.json).
//! The test split waits for the human's review and the freeze, as for Kestrel-Tasks-v0.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Instant;

use kestrel_memory::eval::{self, K_MAX, QueryRow};
use kestrel_memory::{Memory, Weights};
use serde_json::json;

const BUILD: &str = "datasets/kestrel_memory_v0/build";
const USAGE: &str = "usage: recall_eval [--split dev|test] [OUT.json]";

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (split, rest) = match args.as_slice() {
        [flag, split, rest @ ..] if flag == "--split" => (split.as_str(), rest),
        rest => ("dev", rest),
    };
    if !matches!(split, "dev" | "test") || rest.len() > 1 {
        return Err(USAGE.into());
    }
    let out = rest
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("experiments/logs/memory/recall_{split}.json")));
    let data = eval::load(Path::new(BUILD))?;
    if split == "test" && data.status == "draft" {
        return Err(
            "Kestrel-Memory-v0 is a draft: the test split waits for the review and the freeze"
                .into(),
        );
    }
    let mut memory = Memory::in_memory()?;
    let ids = eval::seed(&mut memory, &data.store)?;
    let queries: Vec<&QueryRow> = data.queries.iter().filter(|q| q.split == split).collect();
    let w = Weights::default();
    let t0 = Instant::now();
    let rows = eval::rank_all(&memory, &ids, &queries, data.now, &w)?;
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    let summary = eval::summary(&rows);
    let report = json!({
        "dataset": "kestrel-memory-v0",
        "status": data.status,
        "store_sha256": data.manifest["store_sha256"],
        "queries_sha256": data.manifest["queries_sha256"],
        "split": split,
        "now": data.now,
        "weights": {"relevance": w.relevance, "recency": w.recency, "importance": w.importance,
                    "half_life_days": w.half_life_days},
        "k_max": K_MAX,
        "summary": summary,
        "ms_total": ms,
        "note": "lexical baseline (Ch. 18.1), default weights, untuned; dev timing as found",
        "rows": rows,
    });
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&out, serde_json::to_string_pretty(&report)? + "\n")?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    eprintln!("{} queries in {ms:.2} ms -> {}", rows.len(), out.display());
    Ok(())
}
