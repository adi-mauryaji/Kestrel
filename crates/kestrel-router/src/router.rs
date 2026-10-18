//! The cascade (blueprint Ch. 13.5, A.5): consult tiers cheapest first, accept the first
//! prediction whose calibrated confidence reaches that tier's threshold, abstain otherwise.
//!
//! Differences from the A.5 reference, each covered by a test:
//! - a probability outside [0, 1] (NaN included) is a broken tier, recorded as failed; A.5
//!   panicked on NaN (`partial_cmp(..).unwrap()`), against the "errors are values" contract;
//! - `Cancelled` stops routing; A.5 escalated, so a cancelled request ran the next, dearer tier;
//! - `Cascade::new` rejects thresholds outside [0, 1] and two models for one tier;
//! - the budget sum cannot overflow (checked, not wrapping or saturating);
//! - ties between intents go to the first one the tier listed, so routing is deterministic.

use kestrel_core::features::has_conjunction;
use kestrel_core::{Intent, KestrelError, Prediction, Request, Tier, TierModel};
use serde::Deserialize;

/// Generated from the dev tuning by experiments/router/make_config.py (thresholds from val only);
/// fields the router does not use (provenance, hash) are ignored when reading.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RouterConfig {
    /// Minimum calibrated confidence to accept at each tier (index = `Tier::index()`).
    /// Tuned on the validation split only, never on test (Ch. 13.4).
    pub accept: [f32; 4],
    /// Per-request budget in ms. A tier whose measured cost would push the running total past it
    /// is not consulted.
    pub latency_budget_ms: u32,
    /// ADR-017: a request with a conjunction token (`features::has_conjunction`) may ask for
    /// several things, so it goes straight to the generative tier.
    pub multi_intent_to_generative: bool,
}

impl RouterConfig {
    /// Reads a generated config (JSON) and checks it.
    pub fn from_json(text: &str) -> Result<Self, KestrelError> {
        let cfg: Self = serde_json::from_str(text)
            .map_err(|e| KestrelError::Invalid(format!("router config: {e}")))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Every threshold must lie in [0, 1] (`contains` is false for NaN, so NaN fails too).
    pub fn validate(&self) -> Result<(), KestrelError> {
        match self.accept.iter().find(|t| !(0.0..=1.0).contains(*t)) {
            Some(t) => Err(KestrelError::Invalid(format!(
                "accept thresholds must be in [0, 1], got {t}"
            ))),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbstainReason {
    /// Tiers answered, none confidently enough.
    LowConfidence,
    /// The next tier would exceed the budget; an earlier tier had answered.
    BudgetExhausted,
    /// No tier answered: all failed, had no opinion, or did not fit the budget.
    NoTierAvailable,
    /// The request was cancelled while a tier ran.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Accepted(Prediction),
    /// `best` is the most confident prediction seen (calibrated, so comparable across tiers).
    Abstained {
        best: Option<Prediction>,
        reason: AbstainReason,
    },
}

/// What routing one request did: the tier-reach data of Ch. 36.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trace {
    /// Every tier whose cost was paid, in order, failed ones included.
    pub tiers_consulted: Vec<Tier>,
    pub tiers_failed: Vec<Tier>,
    /// Sum of the consulted tiers' measured costs.
    pub est_spent_ms: u32,
    /// The multi-intent signal fired, so tiers below the generative one were skipped.
    pub multi_intent: bool,
}

pub struct Cascade {
    /// Sorted by tier, cheapest first; at most one model per tier.
    tiers: Vec<Box<dyn TierModel>>,
    cfg: RouterConfig,
}

impl Cascade {
    pub fn new(
        mut tiers: Vec<Box<dyn TierModel>>,
        cfg: RouterConfig,
    ) -> Result<Self, KestrelError> {
        cfg.validate()?;
        tiers.sort_by_key(|m| m.tier());
        if let Some(w) = tiers.windows(2).find(|w| w[0].tier() == w[1].tier()) {
            return Err(KestrelError::Invalid(format!(
                "two models for tier {:?}",
                w[0].tier()
            )));
        }
        Ok(Self { tiers, cfg })
    }

    pub fn config(&self) -> &RouterConfig {
        &self.cfg
    }

    pub fn route(&self, req: &Request) -> (Outcome, Trace) {
        let mut trace = Trace::default();
        let mut best: Option<Prediction> = None;
        let abstain = |best: Option<Prediction>, reason| Outcome::Abstained { best, reason };
        // A request that may ask for several things must not be accepted as one intent below the
        // generative tier (ADR-010), so it skips those tiers, and abstains if that tier fails.
        trace.multi_intent = self.cfg.multi_intent_to_generative && has_conjunction(&req.text);

        for m in &self.tiers {
            if trace.multi_intent && m.tier() < Tier::Generative {
                continue;
            }
            let spent = trace.est_spent_ms.checked_add(m.est_cost_ms());
            let Some(spent) = spent.filter(|s| *s <= self.cfg.latency_budget_ms) else {
                let reason = if best.is_some() {
                    AbstainReason::BudgetExhausted
                } else {
                    AbstainReason::NoTierAvailable
                };
                return (abstain(best, reason), trace);
            };
            trace.tiers_consulted.push(m.tier());
            trace.est_spent_ms = spent;

            let top = match m.classify(req).and_then(top_prediction) {
                Ok(top) => top,
                Err(KestrelError::Cancelled) => {
                    return (abstain(best, AbstainReason::Cancelled), trace);
                }
                Err(_) => {
                    // Graceful degradation (Ch. 16): the next tier may still answer.
                    trace.tiers_failed.push(m.tier());
                    continue;
                }
            };
            let Some((intent, confidence)) = top else {
                continue; // no opinion
            };
            let pred = Prediction {
                intent,
                confidence,
                tier: m.tier(),
            };
            if confidence >= self.cfg.accept[m.tier().index()] {
                return (Outcome::Accepted(pred), trace);
            }
            if best.as_ref().is_none_or(|b| confidence > b.confidence) {
                best = Some(pred);
            }
        }
        let reason = if best.is_some() {
            AbstainReason::LowConfidence
        } else {
            AbstainReason::NoTierAvailable
        };
        (abstain(best, reason), trace)
    }
}

/// A tier's most probable intent (the first listed on ties), or `None` for "no opinion".
/// A probability outside [0, 1] breaks the `TierModel` contract, so it is an error.
fn top_prediction(probs: Vec<(Intent, f32)>) -> Result<Option<(Intent, f32)>, KestrelError> {
    let mut top: Option<(Intent, f32)> = None;
    for (intent, p) in probs {
        if !(0.0..=1.0).contains(&p) {
            return Err(KestrelError::Invalid(format!(
                "probability {p} for {intent:?} is outside [0, 1]"
            )));
        }
        if top.is_none_or(|(_, q)| p > q) {
            top = Some((intent, p));
        }
    }
    Ok(top)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Intent::*;
    use Tier::*;

    /// Probabilities, or a function making the error (`KestrelError` is not `Clone`).
    type Reply = Result<Vec<(Intent, f32)>, fn() -> KestrelError>;

    /// A tier with a fixed reply.
    struct Mock {
        tier: Tier,
        cost: u32,
        reply: Reply,
    }

    impl TierModel for Mock {
        fn tier(&self) -> Tier {
            self.tier
        }
        fn est_cost_ms(&self) -> u32 {
            self.cost
        }
        fn classify(&self, _: &Request) -> Result<Vec<(Intent, f32)>, KestrelError> {
            match &self.reply {
                Ok(v) => Ok(v.clone()),
                Err(e) => Err(e()),
            }
        }
    }

    fn m(tier: Tier, cost: u32, probs: Vec<(Intent, f32)>) -> Box<dyn TierModel> {
        Box::new(Mock {
            tier,
            cost,
            reply: Ok(probs),
        })
    }

    fn failing(tier: Tier, cost: u32, e: fn() -> KestrelError) -> Box<dyn TierModel> {
        Box::new(Mock {
            tier,
            cost,
            reply: Err(e),
        })
    }

    /// Illustrative thresholds for the tests, not tuned ones.
    fn cfg(budget: u32) -> RouterConfig {
        RouterConfig {
            accept: [0.95, 0.90, 0.85, 0.60],
            latency_budget_ms: budget,
            multi_intent_to_generative: false,
        }
    }

    fn route_text(tiers: Vec<Box<dyn TierModel>>, signal: bool, text: &str) -> (Outcome, Trace) {
        let mut c = cfg(2000);
        c.multi_intent_to_generative = signal;
        Cascade::new(tiers, c)
            .unwrap()
            .route(&Request::new(1, text))
    }

    fn three_tiers() -> Vec<Box<dyn TierModel>> {
        vec![
            m(Rules, 1, vec![(SearchNotes, 0.99)]),
            m(Encoder, 10, vec![(SearchNotes, 0.99)]),
            m(Generative, 800, vec![(SummarizeNotes, 0.7)]),
        ]
    }

    fn route(tiers: Vec<Box<dyn TierModel>>, budget: u32) -> (Outcome, Trace) {
        Cascade::new(tiers, cfg(budget))
            .unwrap()
            .route(&Request::new(1, "anything"))
    }

    fn pred(intent: Intent, confidence: f32, tier: Tier) -> Prediction {
        Prediction {
            intent,
            confidence,
            tier,
        }
    }

    #[test]
    fn stops_at_the_first_confident_tier() {
        let (out, tr) = route(
            vec![
                m(Rules, 1, vec![]),
                m(Encoder, 10, vec![(CreateTask, 0.97)]),
                m(Generative, 800, vec![(Chitchat, 0.99)]),
            ],
            2000,
        );
        assert_eq!(out, Outcome::Accepted(pred(CreateTask, 0.97, Encoder)));
        assert_eq!(tr.tiers_consulted, vec![Rules, Encoder]); // Tier 4 never ran
        assert_eq!(tr.est_spent_ms, 11);
    }

    #[test]
    fn accepts_at_exactly_the_threshold() {
        let (out, _) = route(vec![m(Encoder, 10, vec![(SearchNotes, 0.85)])], 2000);
        assert_eq!(out, Outcome::Accepted(pred(SearchNotes, 0.85, Encoder)));
    }

    #[test]
    fn escalates_when_lower_tiers_are_unsure() {
        let (out, tr) = route(
            vec![
                m(Rules, 1, vec![(SearchNotes, 0.50)]),
                m(Encoder, 30, vec![(SearchNotes, 0.70)]),
                m(Generative, 800, vec![(SummarizeNotes, 0.80)]),
            ],
            2000,
        );
        assert_eq!(
            out,
            Outcome::Accepted(pred(SummarizeNotes, 0.80, Generative))
        );
        assert_eq!(tr.tiers_consulted, vec![Rules, Encoder, Generative]);
    }

    #[test]
    fn consults_tiers_cheapest_first_whatever_the_input_order() {
        let (out, tr) = route(
            vec![
                m(Generative, 800, vec![(Chitchat, 0.99)]),
                m(Encoder, 10, vec![(CreateTask, 0.97)]),
            ],
            2000,
        );
        assert_eq!(out, Outcome::Accepted(pred(CreateTask, 0.97, Encoder)));
        assert_eq!(tr.tiers_consulted, vec![Encoder]);
    }

    #[test]
    fn abstains_with_the_most_confident_prediction_when_no_tier_is_sure() {
        let (out, tr) = route(
            vec![
                m(Encoder, 30, vec![(SearchNotes, 0.70), (ReadDocument, 0.30)]),
                m(Generative, 800, vec![(SummarizeNotes, 0.55)]),
            ],
            2000,
        );
        assert_eq!(
            out,
            Outcome::Abstained {
                best: Some(pred(SearchNotes, 0.70, Encoder)),
                reason: AbstainReason::LowConfidence,
            }
        );
        assert_eq!(tr.est_spent_ms, 830);
    }

    #[test]
    fn abstains_when_the_budget_would_be_exceeded() {
        let (out, tr) = route(
            vec![
                m(Rules, 10, vec![(SearchNotes, 0.50)]),
                m(Encoder, 30, vec![(SearchNotes, 0.70)]),
                m(Generative, 800, vec![(SummarizeNotes, 0.99)]),
            ],
            100,
        );
        assert_eq!(
            out,
            Outcome::Abstained {
                best: Some(pred(SearchNotes, 0.70, Encoder)),
                reason: AbstainReason::BudgetExhausted,
            }
        );
        assert_eq!(tr.tiers_consulted, vec![Rules, Encoder]);
        assert_eq!(tr.est_spent_ms, 40);
    }

    #[test]
    fn a_budget_too_small_for_any_tier_has_no_tier_available() {
        let (out, tr) = route(vec![m(Encoder, 30, vec![(SearchNotes, 0.99)])], 20);
        assert_eq!(
            out,
            Outcome::Abstained {
                best: None,
                reason: AbstainReason::NoTierAvailable,
            }
        );
        assert!(tr.tiers_consulted.is_empty());
    }

    #[test]
    fn the_budget_sum_cannot_overflow() {
        let half = u32::MAX / 2 + 1; // two of these overflow u32
        let (out, tr) = route(
            vec![
                m(Encoder, half, vec![(SearchNotes, 0.5)]),
                m(Generative, half, vec![(SearchNotes, 0.99)]),
            ],
            u32::MAX,
        );
        assert_eq!(
            out,
            Outcome::Abstained {
                best: Some(pred(SearchNotes, 0.5, Encoder)),
                reason: AbstainReason::BudgetExhausted,
            }
        );
        assert_eq!(tr.tiers_consulted, vec![Encoder]);
    }

    #[test]
    fn a_failed_tier_degrades_to_the_next() {
        let (out, tr) = route(
            vec![
                m(Rules, 1, vec![(SearchNotes, 0.2)]),
                failing(Encoder, 30, || KestrelError::ModelUnavailable(Encoder)),
                m(Generative, 800, vec![(SearchNotes, 0.9)]),
            ],
            2000,
        );
        assert_eq!(out, Outcome::Accepted(pred(SearchNotes, 0.9, Generative)));
        assert_eq!(tr.tiers_consulted, vec![Rules, Encoder, Generative]); // its cost was paid
        assert_eq!(tr.tiers_failed, vec![Encoder]);
    }

    #[test]
    fn a_timed_out_tier_also_degrades() {
        let (out, tr) = route(
            vec![
                failing(Encoder, 30, || KestrelError::Timeout { tier: Encoder }),
                m(Generative, 800, vec![(Chitchat, 0.9)]),
            ],
            2000,
        );
        assert_eq!(out, Outcome::Accepted(pred(Chitchat, 0.9, Generative)));
        assert_eq!(tr.tiers_failed, vec![Encoder]);
    }

    #[test]
    fn cancellation_stops_routing_instead_of_escalating() {
        let (out, tr) = route(
            vec![
                m(Rules, 1, vec![(SearchNotes, 0.3)]),
                failing(Encoder, 30, || KestrelError::Cancelled),
                m(Generative, 800, vec![(SearchNotes, 0.99)]),
            ],
            2000,
        );
        assert_eq!(
            out,
            Outcome::Abstained {
                best: Some(pred(SearchNotes, 0.3, Rules)),
                reason: AbstainReason::Cancelled,
            }
        );
        assert_eq!(tr.tiers_consulted, vec![Rules, Encoder]); // Tier 4 never started
        assert!(tr.tiers_failed.is_empty());
    }

    #[test]
    fn an_invalid_probability_is_a_failed_tier_not_a_panic() {
        for bad in [f32::NAN, 1.5, -0.1, f32::INFINITY] {
            let (out, tr) = route(
                vec![
                    m(Encoder, 30, vec![(SearchNotes, 0.4), (Chitchat, bad)]),
                    m(Generative, 800, vec![(Chitchat, 0.9)]),
                ],
                2000,
            );
            assert_eq!(
                out,
                Outcome::Accepted(pred(Chitchat, 0.9, Generative)),
                "{bad}"
            );
            assert_eq!(tr.tiers_failed, vec![Encoder], "{bad}");
        }
    }

    #[test]
    fn no_opinion_anywhere_abstains_without_a_prediction() {
        let (out, tr) = route(
            vec![
                m(Rules, 1, vec![]),
                failing(Encoder, 30, || KestrelError::Invalid("x".into())),
            ],
            2000,
        );
        assert_eq!(
            out,
            Outcome::Abstained {
                best: None,
                reason: AbstainReason::NoTierAvailable,
            }
        );
        assert_eq!(tr.tiers_consulted, vec![Rules, Encoder]);
    }

    #[test]
    fn a_tie_goes_to_the_first_listed_intent() {
        let (out, _) = route(
            vec![m(
                Encoder,
                10,
                vec![(ReadDocument, 0.9), (SearchNotes, 0.9)],
            )],
            2000,
        );
        assert_eq!(out, Outcome::Accepted(pred(ReadDocument, 0.9, Encoder)));
    }

    #[test]
    fn rejects_thresholds_outside_the_unit_interval() {
        for bad in [f32::NAN, -0.01, 1.01] {
            let mut c = cfg(2000);
            c.accept[Encoder.index()] = bad;
            assert!(Cascade::new(vec![], c).is_err(), "{bad}");
        }
        let mut edges = cfg(2000);
        edges.accept = [0.0, 1.0, 0.0, 1.0];
        assert!(Cascade::new(vec![], edges).is_ok());
    }

    #[test]
    fn reads_a_generated_config_and_ignores_its_provenance_fields() {
        let json = r#"{"format": 1, "accept": [0.0, 1.0, 0.96, 0.0], "latency_budget_ms": 10000,
                       "multi_intent_to_generative": true, "config_sha256": "ab", "chosen_on": {}}"#;
        let c = RouterConfig::from_json(json).unwrap();
        assert_eq!(c.accept, [0.0, 1.0, 0.96, 0.0]);
        assert_eq!(c.latency_budget_ms, 10000);
        assert!(c.multi_intent_to_generative);
    }

    #[test]
    fn a_config_with_a_bad_threshold_or_a_missing_field_is_an_error() {
        let bad = r#"{"accept": [0.0, 1.0, 1.5, 0.0], "latency_budget_ms": 1, "multi_intent_to_generative": false}"#;
        assert!(RouterConfig::from_json(bad).is_err());
        let missing = r#"{"accept": [0.0, 1.0, 0.5, 0.0], "latency_budget_ms": 1}"#;
        assert!(RouterConfig::from_json(missing).is_err());
    }

    #[test]
    fn rejects_two_models_for_one_tier() {
        let tiers = vec![m(Encoder, 10, vec![]), m(Encoder, 20, vec![])];
        assert!(Cascade::new(tiers, cfg(2000)).is_err());
    }

    #[test]
    fn a_conjunction_sends_the_request_straight_to_the_generative_tier() {
        let (out, tr) = route_text(three_tiers(), true, "Find my notes and summarize them");
        assert_eq!(
            out,
            Outcome::Accepted(pred(SummarizeNotes, 0.7, Generative))
        );
        assert_eq!(tr.tiers_consulted, vec![Generative]);
        assert_eq!(tr.est_spent_ms, 800);
        assert!(tr.multi_intent);
    }

    #[test]
    fn the_signal_changes_nothing_when_off_or_without_a_conjunction() {
        for (signal, text) in [
            (false, "find my notes and summarize them"),
            (true, "find my notes about android"),
        ] {
            let (out, tr) = route_text(three_tiers(), signal, text);
            assert_eq!(
                out,
                Outcome::Accepted(pred(SearchNotes, 0.99, Rules)),
                "{text}"
            );
            assert!(!tr.multi_intent, "{text}");
        }
    }

    #[test]
    fn a_flagged_request_abstains_rather_than_fall_back_below_the_generative_tier() {
        let none = Outcome::Abstained {
            best: None,
            reason: AbstainReason::NoTierAvailable,
        };
        let broken = vec![
            m(Encoder, 10, vec![(SearchNotes, 0.99)]),
            failing(Generative, 800, || {
                KestrelError::ModelUnavailable(Generative)
            }),
        ];
        let (out, tr) = route_text(broken, true, "find my notes and summarize them");
        assert_eq!(out, none);
        assert_eq!(tr.tiers_failed, vec![Generative]);
        let no_slm = vec![m(Encoder, 10, vec![(SearchNotes, 0.99)])];
        let (out, tr) = route_text(no_slm, true, "find my notes and summarize them");
        assert_eq!(out, none);
        assert!(tr.tiers_consulted.is_empty());
    }
}
