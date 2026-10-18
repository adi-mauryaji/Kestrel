//! Integration (blueprint A.6): the real Tier-1 rules and a stand-in generative tier through the
//! real cascade.

use kestrel_core::{Intent, KestrelError, Request, Tier, TierModel};
use kestrel_nlu::rules::{RuleSet, RulesTier};
use kestrel_router::{Cascade, Outcome, RouterConfig};

/// Stands in for Tier 4: always the same answer.
struct Slm;

impl TierModel for Slm {
    fn tier(&self) -> Tier {
        Tier::Generative
    }
    fn est_cost_ms(&self) -> u32 {
        800
    }
    fn classify(&self, _: &Request) -> Result<Vec<(Intent, f32)>, KestrelError> {
        Ok(vec![(Intent::SummarizeNotes, 0.8)])
    }
}

fn cascade() -> Cascade {
    // 0.99 is a placeholder, NOT the rules' measured precision (Ch. 9.3).
    let rules = RulesTier::new(RuleSet::default_set(), 0.99, 1).unwrap();
    let cfg = RouterConfig {
        accept: [0.95, 0.90, 0.85, 0.60],
        latency_budget_ms: 2000,
    };
    Cascade::new(vec![Box::new(Slm), Box::new(rules)], cfg).unwrap()
}

fn route(text: &str) -> (Outcome, Vec<Tier>) {
    let (out, trace) = cascade().route(&Request::new(1, text));
    (out, trace.tiers_consulted)
}

#[test]
fn a_simple_request_never_touches_the_generative_tier() {
    let (out, consulted) = route("Remind me to call Asha tomorrow.");
    assert!(
        matches!(out, Outcome::Accepted(ref p) if p.intent == Intent::CreateTask && p.tier == Tier::Rules),
        "{out:?}"
    );
    assert_eq!(consulted, vec![Tier::Rules]);
}

#[test]
fn a_multi_intent_request_escalates() {
    let (out, consulted) = route("Summarize my last three meeting notes and create a task list");
    assert!(
        matches!(out, Outcome::Accepted(ref p) if p.tier == Tier::Generative),
        "{out:?}"
    );
    assert_eq!(consulted, vec![Tier::Rules, Tier::Generative]);
}

#[test]
fn a_negated_request_escalates() {
    let (out, consulted) = route("don't remind me to call asha");
    assert!(
        matches!(out, Outcome::Accepted(ref p) if p.tier == Tier::Generative),
        "{out:?}"
    );
    assert_eq!(consulted, vec![Tier::Rules, Tier::Generative]);
}
