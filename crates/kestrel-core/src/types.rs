//! Core vocabulary shared by every tier (blueprint Ch. 7.3, A.1).

use serde::{Deserialize, Serialize};

use crate::normalize::normalize;

/// What the user wants: one intent per allowlisted tool, plus chitchat and unsupported (ADR-010).
/// The serialized names are the labels used in datasets, `head.json` and configs, so renaming
/// a variant is a breaking change. `datasets/contracts/intents.txt` lists them for Python.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    SearchNotes,
    ReadDocument,
    CreateTask,
    DeleteTask,
    SummarizeNotes,
    SetPreference,
    RetrieveMemory,
    Chitchat,
    Unsupported,
}

impl Intent {
    /// Every intent in declaration order: the class order for classifier heads.
    pub const ALL: [Intent; 9] = [
        Intent::SearchNotes,
        Intent::ReadDocument,
        Intent::CreateTask,
        Intent::DeleteTask,
        Intent::SummarizeNotes,
        Intent::SetPreference,
        Intent::RetrieveMemory,
        Intent::Chitchat,
        Intent::Unsupported,
    ];

    /// The label string: the serialized name (tested), used where a tier needs the text itself,
    /// such as Tier 4's constrained label decoding.
    pub fn label(self) -> &'static str {
        match self {
            Intent::SearchNotes => "search_notes",
            Intent::ReadDocument => "read_document",
            Intent::CreateTask => "create_task",
            Intent::DeleteTask => "delete_task",
            Intent::SummarizeNotes => "summarize_notes",
            Intent::SetPreference => "set_preference",
            Intent::RetrieveMemory => "retrieve_memory",
            Intent::Chitchat => "chitchat",
            Intent::Unsupported => "unsupported",
        }
    }
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
            (Intent::ReadDocument, "\"read_document\""),
            (Intent::CreateTask, "\"create_task\""),
            (Intent::DeleteTask, "\"delete_task\""),
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
    fn label_is_the_serialized_name() {
        for intent in Intent::ALL {
            let json = serde_json::to_value(intent).unwrap();
            assert_eq!(json.as_str(), Some(intent.label()), "{intent:?}");
        }
    }

    /// `ALL` must hold every variant once, in declaration order. The match has no
    /// wildcard, so a new variant doesn't compile until it is given a position here.
    #[test]
    fn all_lists_every_intent_in_order() {
        use Intent::*;
        let position = |i: Intent| match i {
            SearchNotes => 0,
            ReadDocument => 1,
            CreateTask => 2,
            DeleteTask => 3,
            SummarizeNotes => 4,
            SetPreference => 5,
            RetrieveMemory => 6,
            Chitchat => 7,
            Unsupported => 8,
        };
        assert_eq!(Intent::ALL.map(position), [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }

    /// The label list Python reads (ADR-010) must match the enum exactly.
    #[test]
    fn all_matches_the_dataset_contract() {
        let contract = include_str!("../../../datasets/contracts/intents.txt");
        let file_labels: Vec<&str> = contract
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        let enum_labels: Vec<String> = Intent::ALL
            .iter()
            .map(|i| {
                serde_json::to_string(i)
                    .unwrap()
                    .trim_matches('"')
                    .to_owned()
            })
            .collect();
        assert_eq!(enum_labels, file_labels);
    }

    #[test]
    fn unknown_intent_label_is_rejected() {
        assert!(serde_json::from_str::<Intent>("\"set_alarm\"").is_err());
    }
}
