//! Kestrel-Memory-v0 (ADR-026) for the retrieval baseline (Ch. 18, Week 10 R/E): load the built
//! store and queries (datasets/kestrel_memory_v0/build, from experiments/data/memory_v0.py), seed a
//! `Memory` through the public write path, rank every query, score recall@k.
//!
//! recall@k of a query = expected memories in the top k / expected memories; MRR uses the first
//! expected one in the top `K_MAX`. Negatives (nothing should come back) are scored apart: how often
//! nothing comes back, and how many items do. A trap (a pending guess, an expired row) in any
//! result is a leak; the store makes leaks impossible, and the evaluation counts them anyway.

use std::collections::HashMap;
use std::path::Path;

use kestrel_core::KestrelError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::retrieval::{self, Weights};
use crate::store::{Draft, Kind, Memory, Source};

/// How deep each query is ranked.
pub const K_MAX: usize = 10;
pub const KS: [usize; 3] = [1, 3, 5];

#[derive(Debug, Clone, Deserialize)]
pub struct StoreRow {
    pub row: i64,
    pub id: String,
    pub group: String,
    pub kind: String,
    pub key: String,
    pub value: String,
    pub source: String,
    pub importance: u8,
    pub approved: bool,
    pub updated_at: i64,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QueryRow {
    pub id: String,
    pub group: String,
    pub lang: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub text: String,
    pub expect: Vec<String>,
    pub trap: Vec<String>,
    pub split: String,
}

pub struct Dataset {
    pub now: i64,
    /// "draft" until the human's review (manifest.json).
    pub status: String,
    /// build/manifest.json as written by the builder (hashes, counts).
    pub manifest: Value,
    pub store: Vec<StoreRow>,
    pub queries: Vec<QueryRow>,
}

/// One query's ranking.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ranked {
    pub id: String,
    pub lang: String,
    pub kind: String,
    pub terms: Vec<String>,
    pub expect: Vec<String>,
    pub trap: Vec<String>,
    /// Dataset ids, best first, at most K_MAX.
    pub retrieved: Vec<String>,
}

/// Reads build/manifest.json, store.jsonl and queries.jsonl.
pub fn load(dir: &Path) -> Result<Dataset, KestrelError> {
    let read = |name: &str| {
        std::fs::read_to_string(dir.join(name))
            .map_err(|e| KestrelError::Storage(format!("{}: {e}", dir.join(name).display())))
    };
    let bad = |name: &str, e: serde_json::Error| KestrelError::Invalid(format!("{name}: {e}"));
    let manifest: Value =
        serde_json::from_str(&read("manifest.json")?).map_err(|e| bad("manifest.json", e))?;
    let lines = |name: &str| -> Result<Vec<String>, KestrelError> {
        Ok(read(name)?.lines().map(str::to_string).collect())
    };
    let store = lines("store.jsonl")?
        .iter()
        .map(|l| serde_json::from_str(l).map_err(|e| bad("store.jsonl", e)))
        .collect::<Result<_, _>>()?;
    let queries = lines("queries.jsonl")?
        .iter()
        .map(|l| serde_json::from_str(l).map_err(|e| bad("queries.jsonl", e)))
        .collect::<Result<_, _>>()?;
    Ok(Dataset {
        now: manifest["now_unix"]
            .as_i64()
            .ok_or_else(|| KestrelError::Invalid("manifest.json: now_unix".into()))?,
        status: manifest["status"].as_str().unwrap_or("draft").to_string(),
        manifest,
        store,
        queries,
    })
}

/// Seeds an empty `memory` with the store in file order, through `propose` (and `approve` for a
/// model guess the user approved), at each row's own time. Returns row id -> dataset id; every
/// row must get the id the builder wrote.
pub fn seed(memory: &mut Memory, store: &[StoreRow]) -> Result<HashMap<i64, String>, KestrelError> {
    let mut ids = HashMap::new();
    for r in store {
        let bad = |what: &str| KestrelError::Invalid(format!("{}: {what}", r.id));
        let kind = Kind::parse(&r.kind).ok_or_else(|| bad("kind"))?;
        let source = Source::parse(&r.source).ok_or_else(|| bad("source"))?;
        let mut d = Draft::new(kind, &r.key, &r.value).importance(r.importance);
        if let Some(at) = r.expires_at {
            d = d.expires_at(at);
        }
        let id = memory.propose(&d, source, r.updated_at)?.id();
        if source == Source::ModelInferred && r.approved {
            memory.approve(id, r.updated_at)?;
        }
        if id != r.row {
            return Err(bad(&format!("stored as row {id}, built as {}", r.row)));
        }
        ids.insert(id, r.id.clone());
    }
    Ok(ids)
}

/// Every query ranked at `now`, K_MAX deep.
pub fn rank_all(
    memory: &Memory,
    ids: &HashMap<i64, String>,
    queries: &[&QueryRow],
    now: i64,
    w: &Weights,
) -> Result<Vec<Ranked>, KestrelError> {
    queries
        .iter()
        .map(|q| {
            let terms = retrieval::terms(&q.text);
            let hits = memory.retrieve(&terms, now, K_MAX, w)?;
            let name = |id: i64| ids.get(&id).cloned().unwrap_or_else(|| format!("row {id}"));
            Ok(Ranked {
                id: q.id.clone(),
                lang: q.lang.clone(),
                kind: q.kind.clone(),
                terms,
                expect: q.expect.clone(),
                trap: q.trap.clone(),
                retrieved: hits.iter().map(|s| name(s.item.id)).collect(),
            })
        })
        .collect()
}

/// Share of the expected memories in the top `k`; None for a negative.
pub fn recall_at(r: &Ranked, k: usize) -> Option<f64> {
    if r.expect.is_empty() {
        return None;
    }
    let top = &r.retrieved[..k.min(r.retrieved.len())];
    let found = r.expect.iter().filter(|e| top.contains(e)).count();
    Some(found as f64 / r.expect.len() as f64)
}

/// 1 / rank of the first expected memory (0 if none is retrieved); None for a negative.
pub fn reciprocal_rank(r: &Ranked) -> Option<f64> {
    if r.expect.is_empty() {
        return None;
    }
    let first = r.retrieved.iter().position(|x| r.expect.contains(x));
    Some(first.map_or(0.0, |i| 1.0 / (i + 1) as f64))
}

/// How many trap memories came back.
pub fn leaks(r: &Ranked) -> usize {
    r.retrieved.iter().filter(|x| r.trap.contains(x)).count()
}

/// Recall@k and MRR overall, by query type, by language and by both; negatives apart; leaks.
pub fn summary(rows: &[Ranked]) -> Value {
    let scored = |keep: &dyn Fn(&Ranked) -> bool| {
        let v: Vec<&Ranked> = rows
            .iter()
            .filter(|r| !r.expect.is_empty() && keep(r))
            .collect();
        let mean = |f: &dyn Fn(&Ranked) -> f64| {
            if v.is_empty() {
                Value::Null
            } else {
                json!(v.iter().map(|r| f(r)).sum::<f64>() / v.len() as f64)
            }
        };
        let mut o = json!({"n": v.len(), "mrr": mean(&|r| reciprocal_rank(r).unwrap_or(0.0))});
        for k in KS {
            o[format!("recall@{k}")] = mean(&|r| recall_at(r, k).unwrap_or(0.0));
        }
        o
    };
    let negatives: Vec<&Ranked> = rows.iter().filter(|r| r.expect.is_empty()).collect();
    let share = |f: &dyn Fn(&Ranked) -> f64| {
        if negatives.is_empty() {
            Value::Null
        } else {
            json!(negatives.iter().map(|r| f(r)).sum::<f64>() / negatives.len() as f64)
        }
    };
    let mut s = json!({
        "all": scored(&|_| true),
        "negative": {
            "n": negatives.len(),
            "nothing_returned": share(&|r| f64::from(u8::from(r.retrieved.is_empty()))),
            "mean_returned@5": share(&|r| r.retrieved.len().min(5) as f64),
        },
        "leaks": rows.iter().map(leaks).sum::<usize>(),
    });
    for kind in ["lexical", "paraphrase"] {
        s[kind] = scored(&|r| r.kind == kind);
        for lang in ["en", "hi-en"] {
            s[format!("{kind}/{lang}")] = scored(&|r| r.kind == kind && r.lang == lang);
        }
    }
    for lang in ["en", "hi-en"] {
        s[lang] = scored(&|r| r.lang == lang);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranked(kind: &str, lang: &str, expect: &[&str], retrieved: &[&str]) -> Ranked {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        Ranked {
            id: "q".into(),
            lang: lang.into(),
            kind: kind.into(),
            terms: vec![],
            expect: s(expect),
            trap: vec![],
            retrieved: s(retrieved),
        }
    }

    #[test]
    fn recall_and_reciprocal_rank_of_one_query() {
        let r = ranked("lexical", "en", &["a", "b"], &["x", "a", "y", "z", "b"]);
        assert_eq!(recall_at(&r, 1), Some(0.0));
        assert_eq!(recall_at(&r, 3), Some(0.5));
        assert_eq!(recall_at(&r, 5), Some(1.0));
        assert_eq!(reciprocal_rank(&r), Some(0.5));
        let miss = ranked("paraphrase", "en", &["a"], &[]);
        assert_eq!(
            (recall_at(&miss, 5), reciprocal_rank(&miss)),
            (Some(0.0), Some(0.0))
        );
        let neg = ranked("negative", "en", &[], &["x"]);
        assert_eq!((recall_at(&neg, 5), reciprocal_rank(&neg)), (None, None));
    }

    #[test]
    fn a_trap_in_the_results_is_a_leak() {
        let mut r = ranked("negative", "en", &[], &["x", "guess"]);
        assert_eq!(leaks(&r), 0);
        r.trap = vec!["guess".into()];
        assert_eq!(leaks(&r), 1);
    }

    #[test]
    fn the_summary_is_by_type_and_language_with_negatives_apart() {
        let rows = vec![
            ranked("lexical", "en", &["a"], &["a"]),
            ranked("lexical", "hi-en", &["b"], &["x", "y", "b"]),
            ranked("paraphrase", "en", &["c"], &[]),
            ranked("negative", "en", &[], &[]),
            ranked("negative", "hi-en", &[], &["x", "y"]),
        ];
        let s = summary(&rows);
        assert_eq!(s["all"]["n"], 3);
        assert_eq!(s["all"]["recall@1"], 1.0 / 3.0);
        assert_eq!(s["all"]["recall@3"], 2.0 / 3.0);
        assert_eq!(s["lexical"]["n"], 2);
        assert_eq!(s["lexical"]["mrr"], (1.0 + 1.0 / 3.0) / 2.0);
        assert_eq!(s["paraphrase"]["recall@5"], 0.0);
        assert_eq!(s["hi-en"]["n"], 1);
        assert_eq!(s["negative"]["n"], 2);
        assert_eq!(s["negative"]["nothing_returned"], 0.5);
        assert_eq!(s["negative"]["mean_returned@5"], 1.0);
        assert_eq!(s["leaks"], 0);
    }
}
