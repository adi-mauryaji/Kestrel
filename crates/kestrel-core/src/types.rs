//! Core vocabulary shared by every tier (blueprint Ch. 7.3, A.1).

use serde::{Deserialize, Serialize};

use crate::normalize::normalize;

/// What the user wants. The serialized names are the labels used in datasets,
/// `head.json` and configs, so renaming a variant is a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    SearchNotes,
    CreateTask,
    SummarizeNotes,
    SetPreference,
    RetrieveMemory,
    Chitchat,
    Unsupported,
}

/// Cascade tiers, cheapest first. The discriminant is the tier number (1..=4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Tier {
    Rules = 1,
    Micro = 2,
    Encoder = 3,
    Generative = 4,
}

impl Tier {
    /// 0-based position in per-tier arrays such as `router.accept` in kestrel.toml.
    pub fn index(self) -> usize {
        self as usize - 1
    }
}

/// One user request. `raw` is kept for display and audit; tiers read `text`.
#[derive(Debug, Clone)]
pub struct Request {
    pub id: u64,
    pub raw: String,
    pub text: String,
}

impl Request {
    pub fn new(id: u64, raw: &str) -> Self {
        Self {
            id,
            raw: raw.to_string(),
            text: normalize(raw),
        }
    }
}

/// A tier's answer. `confidence` is a calibrated probability (Ch. 13).
#[derive(Debug, Clone, PartialEq)]
pub struct Prediction {
    pub intent: Intent,
    pub confidence: f32,
    pub tier: Tier,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_index_matches_config_array_position() {
        assert_eq!(Tier::Rules.index(), 0);
        assert_eq!(Tier::Micro.index(), 1);
        assert_eq!(Tier::Encoder.index(), 2);
        assert_eq!(Tier::Generative.index(), 3);
    }

    #[test]
    fn tiers_order_cheapest_first() {
        assert!(Tier::Rules < Tier::Micro);
        assert!(Tier::Micro < Tier::Encoder);
        assert!(Tier::Encoder < Tier::Generative);
    }

    #[test]
    fn request_keeps_raw_and_normalizes_text() {
        let r = Request::new(7, "  Remind ME to call Asha!");
        assert_eq!(r.id, 7);
        assert_eq!(r.raw, "  Remind ME to call Asha!");
        assert_eq!(r.text, "remind me to call asha");
    }

    #[test]
    fn intent_labels_are_snake_case() {
        let cases = [
            (Intent::SearchNotes, "\"search_notes\""),
            (Intent::CreateTask, "\"create_task\""),
            (Intent::SummarizeNotes, "\"summarize_notes\""),
            (Intent::SetPreference, "\"set_preference\""),
            (Intent::RetrieveMemory, "\"retrieve_memory\""),
            (Intent::Chitchat, "\"chitchat\""),
            (Intent::Unsupported, "\"unsupported\""),
        ];
        for (intent, label) in cases {
            assert_eq!(serde_json::to_string(&intent).unwrap(), label);
            assert_eq!(serde_json::from_str::<Intent>(label).unwrap(), intent);
        }
    }

    #[test]
    fn unknown_intent_label_is_rejected() {
        assert!(serde_json::from_str::<Intent>("\"set_alarm\"").is_err());
    }
}
