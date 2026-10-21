//! The generated router config (experiments/router/make_config.py) as the runtime reads it: the
//! router's part (`RouterConfig`) plus what building the tiers needs.

use kestrel_core::{KestrelError, Tier};
use kestrel_router::{AbstainReason, Outcome, RouterConfig, Trace};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::labels::LabelSpace;

#[derive(Debug, Deserialize)]
pub struct RuntimeConfig {
    /// Tier numbers the cascade is built from (a tier that never accepts is left out).
    pub tiers: Vec<u8>,
    /// Measured dev cost per tier, ms (index = `Tier::index()`).
    pub est_cost_ms: [u32; 4],
    pub calibration: Calibration,
    pub models: Models,
}

#[derive(Debug, Deserialize)]
pub struct Calibration {
    /// The rules' precision on cal; none when the cascade has no Tier 1 (CLINC150).
    pub tier1_confidence: Option<f32>,
    pub tier3_temperature: f32,
    pub tier4_temperature: f32,
}

#[derive(Debug, Deserialize)]
pub struct Models {
    pub tier3: Tier3,
    pub tier4: Tier4,
}

#[derive(Debug, Deserialize)]
pub struct Tier3 {
    /// Head directory name under experiments/artifacts/tier3/.
    pub head: String,
    /// GGUF file under models/.
    pub encoder_file: String,
}

#[derive(Debug, Deserialize)]
pub struct Tier4 {
    pub model_file: String,
    /// Prompt contract under datasets/contracts/.
    pub prompt: String,
    /// Label list under datasets/contracts/: Tier 4's answers, in order (ADR-019).
    pub labels: String,
    pub n_ctx: u32,
    pub shots: Vec<Shot>,
}

#[derive(Debug, Deserialize)]
pub struct Shot {
    pub text: String,
    pub label: String,
}

/// Both views of one config file. Tiers must be among 1, 3 and 4 (Tier 2 is not built), each once;
/// Tier 1 needs its calibrated confidence.
pub fn load(text: &str) -> Result<(RouterConfig, RuntimeConfig), KestrelError> {
    let router = RouterConfig::from_json(text)?;
    let rt: RuntimeConfig = serde_json::from_str(text)
        .map_err(|e| KestrelError::Invalid(format!("runtime config: {e}")))?;
    let ok = |t: &u8| matches!(t, 1 | 3 | 4);
    if rt.tiers.is_empty()
        || !rt.tiers.iter().all(ok)
        || (1..rt.tiers.len()).any(|i| rt.tiers[..i].contains(&rt.tiers[i]))
    {
        return Err(KestrelError::Invalid(format!(
            "tiers must be distinct values among 1, 3, 4; got {:?}",
            rt.tiers
        )));
    }
    if rt.tiers.contains(&1) && rt.calibration.tier1_confidence.is_none() {
        return Err(KestrelError::Invalid(
            "tier 1 is in the cascade but calibration.tier1_confidence is missing".into(),
        ));
    }
    Ok((router, rt))
}

fn reason(r: AbstainReason) -> &'static str {
    match r {
        AbstainReason::LowConfidence => "low_confidence",
        AbstainReason::BudgetExhausted => "budget_exhausted",
        AbstainReason::NoTierAvailable => "no_tier_available",
        AbstainReason::Cancelled => "cancelled",
    }
}

fn tiers(ts: &[Tier]) -> Vec<u8> {
    ts.iter().map(|&t| t as u8).collect()
}

/// One output line: the decision, the trace and the wall time. `tier` is where routing ended:
/// the accepting tier, or the last one consulted (0 if none), as in experiments/router/policies.py.
/// Labels are written by name, so rows look the same in every label space.
pub fn row<S: LabelSpace>(
    space: &S,
    id: &Value,
    out: &Outcome<S::Label>,
    trace: &Trace,
    ms: f64,
) -> Value {
    let last = trace.tiers_consulted.last().map_or(0, |&t| t as u8);
    let mut r = json!({
        "id": id, "consulted": tiers(&trace.tiers_consulted), "failed": tiers(&trace.tiers_failed),
        "multi_intent": trace.multi_intent, "est_spent_ms": trace.est_spent_ms, "ms": ms,
    });
    match out {
        Outcome::Accepted(p) => {
            r["intent"] = json!(space.name(p.intent));
            r["confidence"] = json!(p.confidence);
            r["tier"] = json!(p.tier as u8);
            r["reason"] = Value::Null;
        }
        Outcome::Abstained { best, reason: why } => {
            r["intent"] = Value::Null;
            r["confidence"] = Value::Null;
            r["tier"] = json!(last);
            r["reason"] = json!(reason(*why));
            r["best"] = best.as_ref().map_or(Value::Null, |b| {
                json!({"intent": space.name(b.intent), "confidence": b.confidence, "tier": b.tier as u8})
            });
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::{Intents, LabelList};
    use kestrel_core::{Intent, Prediction};

    fn config(tiers: &str) -> String {
        format!(
            r#"{{"accept": [0.0, 1.0, 0.96, 0.0], "latency_budget_ms": 10000,
               "multi_intent_to_generative": true, "tiers": {tiers}, "est_cost_ms": [1, 0, 6, 813],
               "calibration": {{"tier1_confidence": 1.0, "tier3_temperature": 0.69, "tier4_temperature": 0.85}},
               "models": {{"tier3": {{"head": "h", "encoder_file": "e.gguf", "encoder": "e"}},
                          "tier4": {{"model_file": "g.gguf", "prompt": "p.txt", "labels": "l.txt", "n_ctx": 2048,
                                     "shots": [{{"text": "add a task", "label": "create_task"}}]}}}},
               "config_sha256": "x"}}"#
        )
    }

    #[test]
    fn loads_both_views_of_a_generated_config() {
        let (router, rt) = load(&config("[1, 3, 4]")).unwrap();
        assert!(router.multi_intent_to_generative);
        assert_eq!(rt.tiers, vec![1, 3, 4]);
        assert_eq!(rt.est_cost_ms[Tier::Generative.index()], 813);
        assert_eq!(rt.models.tier4.shots[0].label, "create_task");
        assert_eq!(rt.models.tier4.labels, "l.txt");
    }

    #[test]
    fn tier_1_needs_its_confidence_and_only_tier_1_does() {
        let cfg = config("[1, 3, 4]")
            .replace(r#""tier1_confidence": 1.0"#, r#""tier1_confidence": null"#);
        assert!(load(&cfg).is_err());
        let (_, rt) = load(&cfg.replace("[1, 3, 4]", "[3, 4]")).unwrap();
        assert_eq!(rt.calibration.tier1_confidence, None);
    }

    #[test]
    fn rejects_unknown_repeated_or_missing_tiers() {
        for bad in ["[1, 2, 4]", "[3, 3]", "[]", "[5]"] {
            assert!(load(&config(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_committed_dev_config_loads() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../experiments/configs/router_intents_v0.json"
        );
        let (_, rt) = load(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(rt.models.tier4.shots.len(), 36); // k = 2 x 9 labels x 2 languages
        assert_eq!(rt.models.tier4.labels, "intents.txt");
    }

    #[test]
    fn the_committed_clinc150_config_loads() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../experiments/configs/router_clinc150.json"
        );
        let (_, rt) = load(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(rt.tiers, vec![3, 4]); // the Kestrel rules cannot answer CLINC150 labels
        assert_eq!(rt.models.tier4.shots.len(), 150); // k = 1 x 150 intents
        assert_eq!(rt.models.tier4.labels, "clinc150_labels.txt");
    }

    #[test]
    fn rows_record_where_routing_ended() {
        let trace = Trace {
            tiers_consulted: vec![Tier::Rules, Tier::Encoder],
            tiers_failed: vec![],
            est_spent_ms: 7,
            multi_intent: false,
        };
        let p = Prediction {
            intent: Intent::CreateTask,
            confidence: 0.5,
            tier: Tier::Encoder,
        };
        let r = row(
            &Intents,
            &json!("a"),
            &Outcome::Accepted(p.clone()),
            &trace,
            6.5,
        );
        assert_eq!(r["intent"], "create_task");
        assert_eq!(r["tier"], 3);
        assert_eq!(r["consulted"], json!([1, 3]));
        assert!(r["reason"].is_null());
        let abstained = Outcome::Abstained {
            best: Some(p),
            reason: AbstainReason::LowConfidence,
        };
        let r = row(&Intents, &json!("b"), &abstained, &trace, 6.5);
        assert!(r["intent"].is_null());
        assert_eq!(r["tier"], 3); // the last tier consulted
        assert_eq!(r["reason"], "low_confidence");
        assert_eq!(r["best"]["intent"], "create_task");
    }

    #[test]
    fn rows_name_labels_from_the_label_space() {
        let l = LabelList::new(vec!["alarm".into(), "oos".into()]).unwrap();
        let p = Prediction {
            intent: 1,
            confidence: 0.9,
            tier: Tier::Generative,
        };
        let r = row(&l, &json!(7), &Outcome::Accepted(p), &Trace::default(), 1.0);
        assert_eq!(
            (r["intent"].as_str(), r["tier"].as_u64()),
            (Some("oos"), Some(4))
        );
    }
}
