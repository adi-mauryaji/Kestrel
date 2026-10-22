//! `kestrel bench-items`: the inner loop of the E1 harness (ADR-020). It loads the models once,
//! builds one cascade per config (R0, R1, R3) over one shared instance of each tier, so R0's
//! Tier 4 and the cascade's are the same object, and routes each item `reps` times with the
//! systems in rotated order. One row per (item, system, repetition).
//!
//!     kestrel bench-items ITEMS.jsonl OUT.jsonl CONFIG.json...
//!
//! Items are {"id", "text", "reps"?: n (default 1), "warmup"?: bool}. The protocol around this
//! (pinning, machine checks, chunks, which items are warm-ups or get 5 repetitions) belongs to
//! experiments/e1/run_e1.py. OUT.meta.json gets the systems, counts, load time and peak RSS.

use std::collections::HashSet;

use kestrel_core::{KestrelError, Request, Tier, TierModel};
use kestrel_router::{Outcome, Trace};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::ask::Step;
use crate::config;
use crate::labels::LabelSpace;

/// Fields every config must share: the same dataset, models, prompt, shots, temperatures and
/// tier costs, so the systems differ only in routing (PREREG §1).
const SHARED: [&str; 4] = ["dataset", "models", "calibration", "est_cost_ms"];

/// The system order for item `i`, repetition `r` (both 0-based): rotated by (i + r) mod n, so
/// over n consecutive items every system runs once in every position.
pub fn rotation(i: usize, r: usize, n: usize) -> Vec<usize> {
    (0..n).map(|k| (i + r + k) % n).collect()
}

/// Refuses configs that differ in any `SHARED` field or repeat a policy.
pub fn check_compatible(configs: &[Value]) -> Result<(), String> {
    let first = configs.first().ok_or("no config given")?;
    for (k, c) in configs.iter().enumerate().skip(1) {
        if let Some(f) = SHARED.iter().find(|&&f| c[f] != first[f]) {
            return Err(format!("config {} differs from config 1 in {f:?}", k + 1));
        }
    }
    let mut seen = HashSet::new();
    for c in configs {
        let p = c["policy"].as_str().ok_or("a config without a policy")?;
        if !seen.insert(p) {
            return Err(format!("policy {p} given twice"));
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Item {
    pub id: Value,
    pub text: String,
    #[serde(default = "one")]
    pub reps: u32,
    #[serde(default)]
    pub warmup: bool,
}

fn one() -> u32 {
    1
}

pub fn parse_item(line: &str) -> Result<Item, String> {
    let item: Item = serde_json::from_str(line).map_err(|e| format!("item {line:?}: {e}"))?;
    if item.reps == 0 {
        return Err(format!("item {}: reps must be at least 1", item.id));
    }
    Ok(item)
}

/// A tier owned elsewhere, so several cascades can consult one instance.
pub struct Shared<'a, L>(pub &'a dyn TierModel<L>);

impl<L> TierModel<L> for Shared<'_, L> {
    fn tier(&self) -> Tier {
        self.0.tier()
    }

    fn est_cost_ms(&self) -> u32 {
        self.0.est_cost_ms()
    }

    fn classify(&self, req: &Request) -> Result<Vec<(L, f32)>, KestrelError> {
        self.0.classify(req)
    }
}

/// One output row: `config::row` plus the system, repetition, position in the rotation,
/// warm-up flag and every consulted tier's top answer and time.
#[allow(clippy::too_many_arguments)]
pub fn bench_row<S: LabelSpace>(
    space: &S,
    item: &Item,
    system: &str,
    rep: u32,
    pos: usize,
    outcome: &Outcome<S::Label>,
    trace: &Trace,
    steps: &[Step<S::Label>],
    ms: f64,
) -> Value {
    let mut r = config::row(space, &item.id, outcome, trace, ms);
    r["system"] = json!(system);
    r["rep"] = json!(rep);
    r["pos"] = json!(pos);
    r["warmup"] = json!(item.warmup);
    let steps = steps.iter().map(|st| {
        let mut s = json!({"tier": st.tier as u8, "label": null, "confidence": null, "ms": st.ms});
        match &st.result {
            Ok(Some((l, p))) => {
                s["label"] = json!(space.name(*l));
                s["confidence"] = json!(p);
            }
            Ok(None) => {}
            Err(e) => s["error"] = json!(e),
        }
        s
    });
    r["steps"] = Value::Array(steps.collect());
    r
}

/// Loads the union of the configs' tiers once, then routes every item as described above.
#[cfg(feature = "llama")]
pub fn run(
    items: &std::path::Path,
    out: &std::path::Path,
    configs: &[&std::path::Path],
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, BufReader, BufWriter, Write};
    use std::time::Instant;

    use kestrel_bench::rss::{kib_to_mib, rss_breakdown};
    use kestrel_router::Cascade;

    use crate::ask::{Log, Recorder};
    use crate::labels::LabelList;
    use crate::route_items::Models;

    let texts = configs
        .iter()
        .map(std::fs::read_to_string)
        .collect::<Result<Vec<_>, _>>()?;
    let values = texts
        .iter()
        .map(|t| serde_json::from_str(t))
        .collect::<Result<Vec<Value>, _>>()?;
    check_compatible(&values)?;
    let parsed = texts
        .iter()
        .map(|t| config::load(t))
        .collect::<Result<Vec<_>, _>>()?;
    let mut union: Vec<u8> = parsed.iter().flat_map(|(_, rt)| rt.tiers.clone()).collect();
    union.sort_unstable();
    union.dedup();
    let mut list = Vec::new();
    for line in BufReader::new(std::fs::File::open(items)?).lines() {
        list.push(parse_item(&line?)?);
    }

    let t0 = Instant::now();
    let models = Models::load_for(configs[0], &union)?;
    let space = LabelList::new(models.labels.clone())?;
    let log: Log<usize> = Log::default();
    let tiers = models.tiers(&space, |t| Box::new(Recorder::new(t, log.clone())), &union)?;
    let mut cascades = Vec::new();
    for (router, rt) in parsed {
        let own = tiers
            .iter()
            .filter(|t| rt.tiers.contains(&(t.tier() as u8)))
            .map(|t| Box::new(Shared(t.as_ref())) as Box<dyn TierModel<usize> + '_>)
            .collect();
        cascades.push(Cascade::new(own, router)?);
    }
    let load_s = t0.elapsed().as_secs_f64();
    let names: Vec<&str> = values
        .iter()
        .map(|v| v["policy"].as_str().unwrap_or("?"))
        .collect();

    let mut w = BufWriter::new(std::fs::File::create(out)?);
    let mut n = 0u64;
    for (i, item) in list.iter().enumerate() {
        for rep in 0..item.reps {
            for (pos, s) in rotation(i, rep as usize, cascades.len())
                .into_iter()
                .enumerate()
            {
                n += 1;
                log.borrow_mut().clear();
                let t0 = Instant::now();
                let (outcome, trace) = cascades[s].route(&Request::new(n, &item.text));
                let ms = t0.elapsed().as_secs_f64() * 1e3;
                let row = bench_row(
                    &space,
                    item,
                    names[s],
                    rep + 1,
                    pos,
                    &outcome,
                    &trace,
                    &log.borrow(),
                    ms,
                );
                writeln!(w, "{row}")?;
            }
        }
        w.flush()?; // a stopped run keeps every item it finished
    }

    let rss = rss_breakdown();
    let systems: Vec<Value> = values
        .iter()
        .map(|v| json!({"policy": v["policy"], "config_sha256": v["config_sha256"]}))
        .collect();
    let meta = json!({
        "systems": systems, "tiers_loaded": union, "items": list.len(), "requests": n,
        "load_s": load_s,
        "peak_rss_mib": rss.map(|r| kib_to_mib(r.peak_kib)),
        "rss_anon_mib_at_end": rss.map(|r| kib_to_mib(r.anon_kib)),
        "rss_file_mib_at_end": rss.map(|r| kib_to_mib(r.file_kib)),
    });
    std::fs::write(out.with_extension("meta.json"), format!("{meta:#}\n"))?;
    eprintln!("routed {} items, {n} requests", list.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::LabelList;
    use kestrel_core::Prediction;

    #[test]
    fn rotation_puts_every_system_in_every_position_over_n_items() {
        assert_eq!(rotation(0, 0, 3), vec![0, 1, 2]);
        assert_eq!(rotation(1, 0, 3), vec![1, 2, 0]);
        assert_eq!(rotation(0, 1, 3), vec![1, 2, 0]); // the next repetition shifts too
        assert_eq!(rotation(4, 3, 3), vec![1, 2, 0]);
        for r in 0..3 {
            for pos in 0..3 {
                let at: HashSet<_> = (0..3).map(|i| rotation(i, r, 3)[pos]).collect();
                assert_eq!(at.len(), 3, "rep {r}, position {pos}");
            }
        }
        assert_eq!(rotation(7, 2, 1), vec![0]);
    }

    fn cfg(policy: &str, prompt: &str) -> Value {
        json!({"policy": policy, "dataset": "clinc150", "est_cost_ms": [1, 0, 6, 989],
               "calibration": {"tier3_temperature": 0.67, "tier4_temperature": 1.26},
               "models": {"tier4": {"prompt": prompt}}, "accept": [1.0, 1.0, 0.6, 0.8]})
    }

    #[test]
    fn configs_must_share_models_prompt_and_calibration() {
        let (r0, r3) = (cfg("R0", "p.txt"), cfg("R3", "p.txt"));
        assert_eq!(check_compatible(&[r0.clone(), r3.clone()]), Ok(()));
        let err = check_compatible(&[r0.clone(), cfg("R3", "other.txt")]).unwrap_err();
        assert!(err.contains("models"), "{err}");
        let mut warm = r3.clone();
        warm["calibration"]["tier4_temperature"] = json!(1.0);
        assert!(
            check_compatible(&[r0.clone(), warm])
                .unwrap_err()
                .contains("calibration")
        );
        assert!(
            check_compatible(&[r0.clone(), r0.clone()])
                .unwrap_err()
                .contains("twice")
        );
        assert!(check_compatible(&[]).is_err());
    }

    #[test]
    fn items_default_to_one_timed_run() {
        let it = parse_item(r#"{"id": "val/1", "text": "wake me at 7"}"#).unwrap();
        assert_eq!((it.reps, it.warmup), (1, false));
        let it = parse_item(r#"{"id": 3, "text": "x", "reps": 5, "warmup": true}"#).unwrap();
        assert_eq!((it.id, it.reps, it.warmup), (json!(3), 5, true));
        assert!(parse_item(r#"{"id": 3, "text": "x", "reps": 0}"#).is_err());
        assert!(parse_item(r#"{"id": 3}"#).is_err());
    }

    struct Fixed;

    impl TierModel<usize> for Fixed {
        fn tier(&self) -> Tier {
            Tier::Encoder
        }
        fn est_cost_ms(&self) -> u32 {
            6
        }
        fn classify(&self, _: &Request) -> Result<Vec<(usize, f32)>, KestrelError> {
            Ok(vec![(1, 0.7)])
        }
    }

    #[test]
    fn a_shared_tier_answers_as_the_tier_it_borrows() {
        let s = Shared(&Fixed);
        assert_eq!((s.tier(), s.est_cost_ms()), (Tier::Encoder, 6));
        assert_eq!(s.classify(&Request::new(1, "x")).unwrap(), vec![(1, 0.7)]);
    }

    #[test]
    fn rows_carry_the_system_repetition_and_each_tiers_answer() {
        let space = LabelList::new(vec!["alarm".into(), "oos".into()]).unwrap();
        let item = parse_item(r#"{"id": "val/9", "text": "x", "warmup": true}"#).unwrap();
        let steps = [
            Step {
                tier: Tier::Encoder,
                result: Ok(Some((0, 0.4))),
                ms: 5.5,
            },
            Step {
                tier: Tier::Generative,
                result: Err("timed out".into()),
                ms: 900.0,
            },
        ];
        let p = Prediction {
            intent: 1,
            confidence: 0.9,
            tier: Tier::Generative,
        };
        let r = bench_row(
            &space,
            &item,
            "R3",
            2,
            1,
            &Outcome::Accepted(p),
            &Trace::default(),
            &steps,
            905.5,
        );
        assert_eq!(
            (r["id"].as_str(), r["intent"].as_str()),
            (Some("val/9"), Some("oos"))
        );
        assert_eq!(
            (r["system"].as_str(), r["rep"].as_u64(), r["pos"].as_u64()),
            (Some("R3"), Some(2), Some(1))
        );
        assert_eq!(r["warmup"], json!(true));
        assert_eq!(
            r["steps"],
            json!([{"tier": 3, "label": "alarm", "confidence": 0.4f32, "ms": 5.5},
                   {"tier": 4, "label": null, "confidence": null, "error": "timed out", "ms": 900.0}])
        );
    }
}
