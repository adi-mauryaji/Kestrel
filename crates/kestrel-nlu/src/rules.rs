//! Tier 1: deterministic rules (blueprint Ch. 9, A.2).
//!
//! Differences from the A.2 reference, each covered by a test:
//! - matching is on whole tokens, prefixes included, so "add task" doesn't fire on "add taskforce";
//! - tokens are split on punctuation, so "notes," still matches "notes";
//! - a negation word ("not", "don't", Hinglish "mat"/"nahi") turns a hit into `Negated`, an escalation;
//! - Hinglish conjunctions ("aur", "phir") count for multi-intent detection.
//!
//! Rules are English only until the Kestrel-Intents-v0 dev split exists. Hinglish requests
//! get `NoMatch` here and escalate to the multilingual encoder. Write rules on the dev split only (Ch. 9.4).

use kestrel_core::features::{has_conjunction, tokens};
use kestrel_core::{Intent, KestrelError, Request, Tier, TierModel};

/// Words that flip or cancel a request. Conservative: on any of them, Tier 1 escalates.
const NEGATIONS: &[&str] = &[
    "not", "don't", "dont", "doesn't", "never", "cannot", "can't", "mat", "nahi", "nahin",
];

pub enum Matcher {
    /// The request's first tokens equal this phrase's tokens.
    StartsWith(&'static str),
    /// Every listed token appears somewhere in the request, in any order.
    ContainsAll(&'static [&'static str]),
}

pub struct Rule {
    pub name: &'static str,
    /// Higher wins.
    pub priority: u8,
    pub matcher: Matcher,
    pub intent: Intent,
}

#[derive(Debug, PartialEq)]
pub enum RuleOutcome {
    Match {
        intent: Intent,
        rule: &'static str,
    },
    /// Several intents fired and the rules can't pick one: escalate.
    Ambiguous(Vec<Intent>),
    /// A rule fired but the request contains a negation: escalate.
    Negated,
    NoMatch,
}

pub struct RuleSet {
    rules: Vec<Rule>,
}

impl Rule {
    fn matches(&self, toks: &[&str]) -> bool {
        match &self.matcher {
            Matcher::StartsWith(phrase) => {
                let want: Vec<&str> = phrase.split(' ').collect();
                toks.starts_with(&want)
            }
            Matcher::ContainsAll(words) => words.iter().all(|w| toks.contains(w)),
        }
    }
}

impl RuleSet {
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }

    pub fn default_set() -> Self {
        use Intent::*;
        use Matcher::*;
        let r = |name, priority, matcher, intent| Rule {
            name,
            priority,
            matcher,
            intent,
        };
        Self::new(vec![
            r("remind", 10, StartsWith("remind me to"), CreateTask),
            r("add_task", 10, StartsWith("add task"), CreateTask),
            r(
                "create_task",
                8,
                ContainsAll(&["create", "task"]),
                CreateTask,
            ),
            r("find_notes", 10, StartsWith("find notes"), SearchNotes),
            r(
                "search_notes",
                8,
                ContainsAll(&["search", "notes"]),
                SearchNotes,
            ),
            r(
                "summarize_notes",
                8,
                ContainsAll(&["summarize", "notes"]),
                SummarizeNotes,
            ),
            r("prefer", 9, StartsWith("i prefer"), SetPreference),
            r(
                "remember_that",
                9,
                StartsWith("remember that"),
                SetPreference,
            ),
            r(
                "recall",
                9,
                StartsWith("what do you remember"),
                RetrieveMemory,
            ),
        ])
    }

    /// Classify already-normalized text. Policy, in order:
    /// no rule fired → `NoMatch`; negation present → `Negated`;
    /// several intents joined by a conjunction → `Ambiguous`; otherwise the highest priority
    /// wins, and a tie between different intents at the top → `Ambiguous`.
    pub fn classify(&self, text: &str) -> RuleOutcome {
        let toks = tokens(text);
        let hits: Vec<&Rule> = self.rules.iter().filter(|r| r.matches(&toks)).collect();
        let Some(top) = hits.iter().map(|r| r.priority).max() else {
            return RuleOutcome::NoMatch;
        };
        if toks.iter().any(|t| NEGATIONS.contains(t)) {
            return RuleOutcome::Negated;
        }

        let distinct = distinct_intents(hits.iter().copied());
        if distinct.len() > 1 && has_conjunction(text) {
            return RuleOutcome::Ambiguous(distinct);
        }

        let winners: Vec<&Rule> = hits.into_iter().filter(|r| r.priority == top).collect();
        let top_intents = distinct_intents(winners.iter().copied());
        match top_intents.as_slice() {
            [intent] => RuleOutcome::Match {
                intent: *intent,
                rule: winners[0].name,
            },
            _ => RuleOutcome::Ambiguous(top_intents),
        }
    }
}

/// Intents of the given rules, deduplicated, in rule-declaration order.
fn distinct_intents<'a>(rules: impl Iterator<Item = &'a Rule>) -> Vec<Intent> {
    let mut out = Vec::new();
    for r in rules {
        if !out.contains(&r.intent) {
            out.push(r.intent);
        }
    }
    out
}

/// Adapts the rule set to the cascade. `precision` is the rules' MEASURED precision on the
/// validation split and is used as their confidence (Ch. 9.3). `est_cost_ms` comes from the bench.
pub struct RulesTier {
    rules: RuleSet,
    precision: f32,
    est_cost_ms: u32,
}

impl RulesTier {
    pub fn new(rules: RuleSet, precision: f32, est_cost_ms: u32) -> Result<Self, KestrelError> {
        // `contains` is false for NaN, so NaN is rejected too.
        if !(0.0..=1.0).contains(&precision) {
            return Err(KestrelError::Invalid(format!(
                "rule precision must be in [0, 1], got {precision}"
            )));
        }
        Ok(Self {
            rules,
            precision,
            est_cost_ms,
        })
    }
}

impl TierModel for RulesTier {
    fn tier(&self) -> Tier {
        Tier::Rules
    }

    fn est_cost_ms(&self) -> u32 {
        self.est_cost_ms
    }

    fn classify(&self, req: &Request) -> Result<Vec<(Intent, f32)>, KestrelError> {
        Ok(match self.rules.classify(&req.text) {
            RuleOutcome::Match { intent, .. } => vec![(intent, self.precision)],
            // Ambiguous, Negated, NoMatch: no opinion, the router escalates.
            _ => Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Intent::*;
    use kestrel_core::normalize::normalize;

    fn c(s: &str) -> RuleOutcome {
        RuleSet::default_set().classify(&normalize(s))
    }

    fn hit(intent: Intent, rule: &'static str) -> RuleOutcome {
        RuleOutcome::Match { intent, rule }
    }

    /// Placeholder confidence for tests only, NOT a measured precision (Ch. 9.3).
    const TEST_PRECISION: f32 = 0.99;

    #[test]
    fn every_rule_fires_on_its_phrase() {
        let cases = [
            (
                "Remind me to call Asha tomorrow.",
                hit(CreateTask, "remind"),
            ),
            ("add task buy milk", hit(CreateTask, "add_task")),
            (
                "please create a task for the report",
                hit(CreateTask, "create_task"),
            ),
            ("find notes about rust", hit(SearchNotes, "find_notes")),
            ("search my notes for asha", hit(SearchNotes, "search_notes")),
            (
                "summarize my last three meeting notes",
                hit(SummarizeNotes, "summarize_notes"),
            ),
            ("I prefer concise answers", hit(SetPreference, "prefer")),
            (
                "remember that my sister is asha",
                hit(SetPreference, "remember_that"),
            ),
            (
                "what do you remember about asha",
                hit(RetrieveMemory, "recall"),
            ),
        ];
        for (text, want) in cases {
            assert_eq!(c(text), want, "{text:?}");
        }
    }

    #[test]
    fn near_misses_do_not_fire() {
        for text in [
            "add taskforce members to the list", // prefix must be whole tokens
            "do research on notebooks",          // "search" in "research", "notes" in "notebooks"
            "remind me later",                   // not "remind me to"
            "i preferred tea yesterday",         // "preferred" is not "prefer"
            "remember thatcher's policies",      // "thatcher's" is not "that"
            "what do you think i remember",      // phrase order matters for prefixes
            "create a tasty dinner",             // "tasty" is not "task"
            "find noteworthy papers",            // "noteworthy" is not "notes"
            "summarize this article",            // no "notes": escalate to a model
            "tell me a joke",
        ] {
            assert_eq!(c(text), RuleOutcome::NoMatch, "{text:?}");
        }
    }

    #[test]
    fn punctuation_does_not_hide_tokens() {
        assert_eq!(
            c("search my notes, please"),
            hit(SearchNotes, "search_notes")
        );
    }

    #[test]
    fn negation_escalates_instead_of_acting() {
        assert_eq!(
            c("please don't create a task for this"),
            RuleOutcome::Negated
        );
        assert_eq!(c("do not create a task"), RuleOutcome::Negated);
        assert_eq!(c("task create mat karna"), RuleOutcome::Negated);
    }

    #[test]
    fn multi_intent_with_conjunction_is_ambiguous() {
        let both = RuleOutcome::Ambiguous(vec![CreateTask, SummarizeNotes]);
        assert_eq!(
            c("Summarize my last three meeting notes and create a task list"),
            both
        );
        // "remind" (p10) outranks "summarize_notes" (p8), so only conjunction detection,
        // in English and in Hinglish, makes these Ambiguous instead of a CreateTask match.
        for text in [
            "remind me to call asha and summarize my notes",
            "remind me to call asha aur summarize my notes",
            "remind me to call asha phir summarize my notes",
        ] {
            assert_eq!(c(text), both, "{text:?}");
        }
    }

    #[test]
    fn higher_priority_intent_wins_without_conjunction() {
        // "remind" (p10, CreateTask) and "search_notes" (p8, SearchNotes) both fire.
        assert_eq!(c("remind me to search my notes"), hit(CreateTask, "remind"));
    }

    #[test]
    fn higher_priority_rule_named_for_same_intent_overlap() {
        // "add_task" (p10) and "create_task" (p8) both fire for CreateTask.
        assert_eq!(
            c("add task create task report"),
            hit(CreateTask, "add_task")
        );
    }

    #[test]
    fn top_priority_tie_between_intents_is_ambiguous() {
        // "create_task" and "search_notes" are both p8 and no conjunction joins them.
        assert_eq!(
            c("create task search notes"),
            RuleOutcome::Ambiguous(vec![CreateTask, SearchNotes])
        );
    }

    #[test]
    fn tier_reports_match_with_precision_as_confidence() {
        let tier = RulesTier::new(RuleSet::default_set(), TEST_PRECISION, 1).unwrap();
        let out = tier
            .classify(&Request::new(1, "Remind me to call Asha"))
            .unwrap();
        assert_eq!(out, vec![(CreateTask, TEST_PRECISION)]);
        assert_eq!(tier.tier(), Tier::Rules);
    }

    #[test]
    fn tier_has_no_opinion_unless_a_single_rule_matches() {
        let tier = RulesTier::new(RuleSet::default_set(), TEST_PRECISION, 1).unwrap();
        for text in [
            "tell me a joke",
            "do not create a task",
            "create task search notes",
        ] {
            assert!(
                tier.classify(&Request::new(1, text)).unwrap().is_empty(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn tier_rejects_precision_outside_zero_to_one() {
        for bad in [-0.1, 1.5, f32::NAN] {
            assert!(
                RulesTier::new(RuleSet::default_set(), bad, 1).is_err(),
                "{bad}"
            );
        }
    }
}
