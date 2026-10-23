//! `kestrel e4-eval` (Week 11, E4; ADR-028): personalization off vs memory after K taught aliases on
//! Kestrel-Personas-v0, with gold intents and no model.
//!
//!     kestrel e4-eval [--split dev|test] [--cascade CONFIG] [OUT.json]
//!
//! The test split is refused while the dataset is a draft. `--cascade` (feature "llama") routes each
//! alias-resolved request with Tier 1 + Tier 3 of CONFIG instead of its gold intent; an abstention
//! fails the request (ADR-028's secondary check).
//!
//! Per persona and condition: a fresh memory; with K > 0, the persona's first K teaching events go
//! through the real path (alias::detect, the user says yes, alias::save). Then every request: the
//! normalizer, alias::resolve with the persona's stored aliases, the template planner with the gold
//! intent, the plan on a fresh copy of the fixture notes, Kestrel-Tasks-v0's judge. Reported per
//! persona and as the mean over personas: personalized success (exact and variant phrasings apart),
//! general success (the regression check), events detected, alias-resolution time.

use std::path::Path;
use std::time::Instant;

use kestrel_core::{Intent, Request};
use kestrel_learning::alias;
use kestrel_memory::Memory;
use kestrel_tools::eval::{self, Fixture};
use kestrel_tools::plan;
use kestrel_tools::templates::{self, Proposal};
use serde_json::{Value, json};

pub const BUILD: &str = "datasets/kestrel_personas_v0/build";
pub const KS: [usize; 3] = [1, 3, 5];

#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub persona: String,
    pub split: String,
    pub lang: String,
    pub personalized: bool,
    pub variant: bool,
    pub intent: Intent,
    pub text: String,
    pub canonical: String,
    pub expect: Value,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub persona: String,
    pub k: usize,
    pub text: String,
    pub phrase: String,
    pub target: String,
}

/// One request's result under one condition, as the summary counts it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub persona: String,
    pub condition: String,
    pub personalized: bool,
    pub variant: bool,
    pub ok: bool,
}

type Error = Box<dyn std::error::Error>;

fn jsonl(path: &Path) -> Result<Vec<Value>, Error> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines()
        .map(|l| serde_json::from_str(l).map_err(Into::into))
        .collect()
}

/// build/items.jsonl, events.jsonl, fixture.json (notes resolved against `repo`) and the status.
pub fn load(build: &Path, repo: &Path) -> Result<(Vec<Item>, Vec<Event>, Fixture, String), Error> {
    let s = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
    let items = jsonl(&build.join("items.jsonl"))?
        .iter()
        .map(|v| {
            let intent = Intent::from_label(&s(v, "intent"))
                .ok_or_else(|| format!("{}: intent {}", s(v, "id"), v["intent"]))?;
            Ok(Item {
                id: s(v, "id"),
                persona: s(v, "persona"),
                split: s(v, "split"),
                lang: s(v, "lang"),
                personalized: v["type"] == "personalized",
                variant: v["variant"] == true,
                intent,
                text: s(v, "text"),
                canonical: s(v, "canonical"),
                expect: v["expect"].clone(),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let events = jsonl(&build.join("events.jsonl"))?
        .iter()
        .map(|v| Event {
            persona: s(v, "persona"),
            k: v["k"].as_u64().unwrap_or_default() as usize,
            text: s(v, "text"),
            phrase: s(v, "phrase"),
            target: s(v, "target"),
        })
        .collect();
    let fixture = Fixture::load(&build.join("fixture.json"), repo)?;
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(build.join("manifest.json"))?)?;
    Ok((items, events, fixture, s(&manifest, "status")))
}

/// A fresh memory after the first `k` events: each detected one is saved as the user's yes.
/// Returns the memory and how many of the k events were detected as teaching.
pub fn teach(events: &[&Event], k: usize, now: i64) -> (Memory, usize) {
    let mut memory = Memory::in_memory().expect("an in-memory store opens");
    let mut detected = 0;
    for e in events.iter().filter(|e| e.k <= k) {
        if let Some(a) = alias::detect(&Request::new(0, &e.text).text)
            && alias::save(&mut memory, &a, now).is_ok()
        {
            detected += 1;
        }
    }
    (memory, detected)
}

/// The template plan for (`intent`, `text`) run on a fresh fixture copy and judged; Err = why not.
pub fn attempt(
    intent: Intent,
    text: &str,
    expect: &Value,
    fx: &Fixture,
    docs: &[String],
    scratch: &Path,
) -> Result<(), String> {
    let ctx = templates::Context {
        today: fx.today,
        documents: docs,
    };
    let wire = match templates::propose(intent, &Request::new(0, text).text, &ctx) {
        Proposal::Plan(v) => v,
        other => return Err(format!("no plan: {other:?}")),
    };
    let plan = plan::parse(&wire.to_string()).map_err(|e| format!("plan: {e}"))?;
    let end = eval::run(&plan, fx, scratch).map_err(|e| format!("run: {e}"))?;
    let problems = eval::judge(expect, fx, &end);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

/// Per condition the mean over personas of
/// each persona's success rate on exact personalized, variant personalized and general requests;
/// for K conditions also the gain over "off" and the general regression (off - K).
pub fn summarize(rows: &[Row]) -> Value {
    let mut personas: Vec<&str> = rows.iter().map(|r| r.persona.as_str()).collect();
    personas.sort();
    personas.dedup();
    let mut conditions: Vec<&str> = rows.iter().map(|r| r.condition.as_str()).collect();
    conditions.sort();
    conditions.dedup();
    let rate = |p: &str, c: &str, keep: &dyn Fn(&Row) -> bool| {
        let v: Vec<bool> = rows
            .iter()
            .filter(|r| r.persona == p && r.condition == c && keep(r))
            .map(|r| r.ok)
            .collect();
        (!v.is_empty()).then(|| v.iter().filter(|&&ok| ok).count() as f64 / v.len() as f64)
    };
    let mean = |c: &str, keep: &dyn Fn(&Row) -> bool| {
        let v: Vec<f64> = personas.iter().filter_map(|p| rate(p, c, keep)).collect();
        (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
    };
    let exact = |r: &Row| r.personalized && !r.variant;
    let variant = |r: &Row| r.personalized && r.variant;
    let general = |r: &Row| !r.personalized;
    let mut out = json!({"personas": personas.len()});
    for c in &conditions {
        out[*c] = json!({
            "personalized": mean(c, &exact),
            "variant": mean(c, &variant),
            "general": mean(c, &general),
        });
    }
    if out.get("off").is_some() {
        for c in conditions.iter().filter(|c| **c != "off") {
            let diff = |k: &str, sign: f64| match (out[*c][k].as_f64(), out["off"][k].as_f64()) {
                (Some(a), Some(b)) => json!(sign * (a - b)),
                _ => Value::Null,
            };
            let (gain, regression) = (diff("personalized", 1.0), diff("general", -1.0));
            out[*c]["delta_personalized"] = gain;
            out[*c]["general_regression"] = regression;
        }
    }
    out
}

/// Maps an alias-resolved request to an intent, or None for an abstention.
pub type Route<'a> = &'a dyn Fn(&str) -> Option<Intent>;

/// The E4 run on one split: every persona under off, K1, K3, K5; gold intents unless `route`.
pub fn run(split: &str, out: &Path, route: Option<(Route, &str)>) -> Result<(), Error> {
    let (items, events, fx, status) = load(Path::new(BUILD), Path::new("."))?;
    if split == "test" && status == "draft" {
        return Err(
            "Kestrel-Personas-v0 is a draft: the test split waits for the review and the freeze"
                .into(),
        );
    }
    let docs = fx.documents();
    let scratch = std::env::temp_dir().join(format!("kestrel_e4_{}", std::process::id()));
    let now = (fx.today.days()) * 86_400;
    let mut personas: Vec<&str> = items
        .iter()
        .filter(|i| i.split == split)
        .map(|i| i.persona.as_str())
        .collect();
    personas.dedup();
    let mut rows = Vec::new();
    let mut detail = Vec::new();
    let mut taught = json!({});
    for p in &personas {
        let mine: Vec<&Event> = events.iter().filter(|e| e.persona == *p).collect();
        for (cond, k) in
            std::iter::once(("off".to_string(), 0)).chain(KS.map(|k| (format!("K{k}"), k)))
        {
            let (memory, detected) = teach(&mine, k, now);
            let aliases = alias::aliases(&memory, now)?;
            let phrases: Vec<(&str, &str)> = mine
                .iter()
                .filter(|e| e.k <= k)
                .map(|e| (e.phrase.as_str(), e.target.as_str()))
                .collect();
            taught[*p][&cond] = json!({"events": k, "detected": detected,
                                       "aliases": aliases.len(), "taught": phrases});
            for it in items.iter().filter(|i| i.persona == *p) {
                let t0 = Instant::now();
                let resolved = alias::resolve(&Request::new(0, &it.text).text, &aliases);
                let resolve_us = t0.elapsed().as_secs_f64() * 1e6;
                let routed = match route {
                    None => Some(it.intent),
                    Some((f, _)) => f(&resolved),
                };
                let res = match routed {
                    Some(intent) => attempt(intent, &resolved, &it.expect, &fx, &docs, &scratch),
                    None => Err("the cascade abstained".into()),
                };
                rows.push(Row {
                    persona: p.to_string(),
                    condition: cond.clone(),
                    personalized: it.personalized,
                    variant: it.variant,
                    ok: res.is_ok(),
                });
                detail.push(json!({"id": it.id, "condition": cond, "lang": it.lang, "personalized": it.personalized,
                                   "variant": it.variant, "ok": res.is_ok(), "resolved": resolved,
                                   "canonical": it.canonical, "routed": routed.map(|i| i.label()), "problem": res.err(),
                                   "resolve_us": resolve_us}));
            }
        }
    }
    let us: Vec<f64> = detail
        .iter()
        .filter_map(|d| d["resolve_us"].as_f64())
        .collect();
    let report = json!({
        "experiment": "E4 (ADR-028): personalization off vs memory after K taught aliases; gold intents; no model",
        "dataset": "kestrel-personas-v0", "status": status, "split": split, "personas": personas,
        "routing": route.map_or("gold intents", |(_, how)| how),
        "summary": summarize(&rows), "taught": taught,
        "resolve_us_mean": us.iter().sum::<f64>() / us.len().max(1) as f64,
        "rows": detail,
    });
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, serde_json::to_string_pretty(&report)? + "\n")?;
    println!("{}", serde_json::to_string_pretty(&report["summary"])?);
    eprintln!(
        "{} requests x conditions -> {}",
        detail.len(),
        out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(k: usize, text: &str) -> Event {
        Event {
            persona: "p".into(),
            k,
            text: text.into(),
            phrase: String::new(),
            target: String::new(),
        }
    }

    #[test]
    fn teaching_stores_the_first_k_events_and_counts_the_detected_ones() {
        let evs = [
            event(1, "when I say standup doc, I mean team sync"),
            event(2, "this is not teaching anything"),
            event(3, "no, by the gang I mean riya and kabir"),
        ];
        let refs: Vec<&Event> = evs.iter().collect();
        let (m, detected) = teach(&refs, 2, 0);
        assert_eq!(detected, 1, "event 2 is not a teaching phrasing");
        let al = alias::aliases(&m, 1).unwrap();
        assert_eq!(al, [("standup doc".to_string(), "team sync".to_string())]);
        let (m3, d3) = teach(&refs, 3, 0);
        assert_eq!((d3, alias::aliases(&m3, 1).unwrap().len()), (2, 2));
        let (m0, d0) = teach(&refs, 0, 0);
        assert_eq!((d0, alias::aliases(&m0, 1).unwrap().len()), (0, 0));
    }

    #[test]
    fn the_summary_averages_over_personas_and_keeps_variants_and_general_apart() {
        // (persona, condition, personalized, variant, ok)
        let row = |p: &str, c: &str, personalized: bool, variant: bool, ok: bool| Row {
            persona: p.into(),
            condition: c.into(),
            personalized,
            variant,
            ok,
        };
        let rows = vec![
            row("a", "off", true, false, false),
            row("a", "off", true, false, false),
            row("a", "off", false, false, true),
            row("a", "K5", true, false, true),
            row("a", "K5", true, false, false),
            row("a", "K5", true, true, false),
            row("a", "K5", false, false, true),
            row("b", "off", true, false, true),
            row("b", "off", false, false, true),
            row("b", "K5", true, false, true),
            row("b", "K5", false, false, false),
        ];
        let s = summarize(&rows);
        // a: off 0/2, K5 1/2 exact; b: off 1/1, K5 1/1 -> means 0.5 and 0.75
        assert_eq!(s["off"]["personalized"], 0.5);
        assert_eq!(s["K5"]["personalized"], 0.75);
        assert_eq!(s["K5"]["variant"], 0.0);
        assert_eq!(s["off"]["general"], 1.0);
        assert_eq!(s["K5"]["general"], 0.5);
        assert_eq!(s["K5"]["delta_personalized"], 0.25);
        assert_eq!(s["K5"]["general_regression"], 0.5);
        assert_eq!(s["personas"], 2);
    }

    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

    fn dataset() -> (Vec<Item>, Vec<Event>, Fixture) {
        let root = Path::new(ROOT);
        let (items, events, fx, _) =
            load(&root.join(BUILD), root).expect("the built dataset loads");
        (items, events, fx)
    }

    #[test]
    fn every_canonical_request_reaches_its_expectation() {
        // Certifies the hand-written expectations (and that each pattern is within the planner's
        // reach when the user speaks canonically), on both splits: this is a data check, not E4.
        let (items, _, fx) = dataset();
        let docs = fx.documents();
        let scratch = std::env::temp_dir().join(format!("kestrel_e4_check_{}", std::process::id()));
        let wrong: Vec<String> = items
            .iter()
            .filter_map(|it| {
                attempt(it.intent, &it.canonical, &it.expect, &fx, &docs, &scratch)
                    .err()
                    .map(|e| format!("{} {:?}: {e}", it.id, it.canonical))
            })
            .collect();
        assert!(
            wrong.is_empty(),
            "{} of {}:\n{}",
            wrong.len(),
            items.len(),
            wrong.join("\n")
        );
    }

    #[test]
    fn every_event_teaches_its_alias_and_resolution_matches_the_labels() {
        let (items, events, _) = dataset();
        let mut wrong = Vec::new();
        for e in &events {
            let got = alias::detect(&Request::new(0, &e.text).text);
            let want = (e.phrase.to_lowercase(), e.target.to_lowercase());
            if got.as_ref().map(|a| (a.phrase.clone(), a.target.clone())) != Some(want) {
                wrong.push(format!(
                    "event {} k{} {:?} -> {got:?}",
                    e.persona, e.k, e.text
                ));
            }
        }
        let personas: std::collections::BTreeSet<&str> =
            events.iter().map(|e| e.persona.as_str()).collect();
        for p in personas {
            let mine: Vec<&Event> = events.iter().filter(|e| e.persona == p).collect();
            let (m, _) = teach(&mine, 5, 0);
            let al = alias::aliases(&m, 1).unwrap();
            for it in items.iter().filter(|i| i.persona == p) {
                let text = Request::new(0, &it.text).text;
                let resolved = alias::resolve(&text, &al);
                let canonical = Request::new(0, &it.canonical).text;
                let ok = match (it.personalized, it.variant) {
                    (true, false) => resolved == canonical,
                    (true, true) => resolved != canonical,
                    (false, _) => resolved == text,
                };
                if !ok {
                    wrong.push(format!(
                        "{} {:?} -> {resolved:?} (canonical {canonical:?})",
                        it.id, it.text
                    ));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
}
