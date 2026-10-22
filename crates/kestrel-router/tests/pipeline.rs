//! Integration (blueprint A.6): the real Tier-1 rules and a stand-in generative tier through the
//! real cascade, including the abstention and failure paths (Week 8 "T").

use kestrel_core::{Intent, KestrelError, Request, Tier, TierModel};
use kestrel_nlu::rules::{RuleSet, RulesTier};
use kestrel_router::{AbstainReason, Cascade, Outcome, RouterConfig};

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

fn cascade(signal: bool) -> Cascade<'static> {
    // 0.99 is a placeholder, NOT the rules' measured precision (Ch. 9.3).
    let rules = RulesTier::new(RuleSet::default_set(), 0.99, 1).unwrap();
    let cfg = RouterConfig {
        accept: [0.95, 0.90, 0.85, 0.60],
        latency_budget_ms: 2000,
        multi_intent_to_generative: signal,
    };
    Cascade::new(vec![Box::new(Slm), Box::new(rules)], cfg).unwrap()
}

fn route(text: &str) -> (Outcome, Vec<Tier>) {
    let (out, trace) = cascade(false).route(&Request::new(1, text));
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

#[test]
fn with_the_signal_a_multi_intent_request_skips_the_rules() {
    let r = Request::new(
        3,
        "Summarize my last three meeting notes and create a task list",
    );
    let (out, trace) = cascade(true).route(&r);
    assert!(
        matches!(out, Outcome::Accepted(ref p) if p.tier == Tier::Generative),
        "{out:?}"
    );
    assert_eq!(trace.tiers_consulted, vec![Tier::Generative]);
    assert!(trace.multi_intent);
}

/// Probabilities, or a function making the error (`KestrelError` is not `Clone`).
type Reply = Result<Vec<(Intent, f32)>, fn() -> KestrelError>;

/// A stand-in Tier 4 with a chosen reply or failure.
struct Slm4 {
    cost: u32,
    reply: Reply,
}

impl TierModel for Slm4 {
    fn tier(&self) -> Tier {
        Tier::Generative
    }
    fn est_cost_ms(&self) -> u32 {
        self.cost
    }
    fn classify(&self, _: &Request) -> Result<Vec<(Intent, f32)>, KestrelError> {
        self.reply.clone().map_err(|e| e())
    }
}

/// Real rules (which may not accept: tau1 = 1.0 > their confidence) in front of `slm`.
fn with(
    slm: Slm4,
    tau1: f32,
    budget: u32,
    signal: bool,
    text: &str,
) -> (Outcome, kestrel_router::Trace) {
    let rules = RulesTier::new(RuleSet::default_set(), 0.99, 1).unwrap();
    let cfg = RouterConfig {
        accept: [tau1, 0.90, 0.85, 0.60],
        latency_budget_ms: budget,
        multi_intent_to_generative: signal,
    };
    let tiers: Vec<Box<dyn TierModel>> = vec![Box::new(rules), Box::new(slm)];
    Cascade::new(tiers, cfg)
        .unwrap()
        .route(&Request::new(1, text))
}

fn slm(reply: Reply) -> Slm4 {
    Slm4 { cost: 800, reply }
}

#[test]
fn an_unsure_slm_abstains_with_the_most_confident_answer_seen() {
    let unsure = slm(Ok(vec![(Intent::SummarizeNotes, 0.4)]));
    let (out, trace) = with(unsure, 1.0, 2000, false, "Remind me to call Asha tomorrow.");
    let Outcome::Abstained {
        best: Some(b),
        reason,
    } = out
    else {
        panic!("{out:?}")
    };
    assert_eq!(reason, AbstainReason::LowConfidence);
    assert_eq!((b.intent, b.tier), (Intent::CreateTask, Tier::Rules)); // 0.99 beats 0.4
    assert_eq!(trace.tiers_consulted, vec![Tier::Rules, Tier::Generative]);
}

#[test]
fn a_timed_out_slm_after_a_rules_miss_has_no_tier_available() {
    let timeout = slm(Err(|| KestrelError::Timeout {
        tier: Tier::Generative,
    }));
    let (out, trace) = with(timeout, 0.95, 2000, false, "what is the capital of peru");
    assert_eq!(
        out,
        Outcome::Abstained {
            best: None,
            reason: AbstainReason::NoTierAvailable
        }
    );
    assert_eq!(trace.tiers_failed, vec![Tier::Generative]);
}

#[test]
fn a_budget_too_small_for_the_slm_abstains_with_the_rules_answer() {
    let fine = slm(Ok(vec![(Intent::CreateTask, 0.99)]));
    let (out, trace) = with(fine, 1.0, 500, false, "Remind me to call Asha tomorrow.");
    let Outcome::Abstained {
        best: Some(b),
        reason,
    } = out
    else {
        panic!("{out:?}")
    };
    assert_eq!(reason, AbstainReason::BudgetExhausted);
    assert_eq!(b.tier, Tier::Rules);
    assert_eq!(trace.tiers_consulted, vec![Tier::Rules]); // Tier 4 was never paid for
}

#[test]
fn a_cancelled_slm_stops_routing() {
    let cancelled = slm(Err(|| KestrelError::Cancelled));
    let (out, _) = with(
        cancelled,
        1.0,
        2000,
        false,
        "Remind me to call Asha tomorrow.",
    );
    assert!(
        matches!(
            out,
            Outcome::Abstained {
                reason: AbstainReason::Cancelled,
                ..
            }
        ),
        "{out:?}"
    );
}

#[test]
fn a_flagged_request_whose_slm_fails_abstains_instead_of_trusting_the_rules() {
    let broken = slm(Err(|| KestrelError::ModelUnavailable(Tier::Generative)));
    let text = "remind me to call asha and summarize my notes";
    let (out, trace) = with(broken, 0.95, 2000, true, text);
    assert_eq!(
        out,
        Outcome::Abstained {
            best: None,
            reason: AbstainReason::NoTierAvailable
        }
    );
    assert!(trace.multi_intent);
    assert_eq!(trace.tiers_consulted, vec![Tier::Generative]); // the rules were not asked
}
