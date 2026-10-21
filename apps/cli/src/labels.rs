//! The label space a cascade answers in (ADR-019). `route-items` uses the config's label list, so
//! one code path serves Kestrel-Intents-v0 and CLINC150, the primary E1 dataset (D8). `ask` uses
//! Kestrel's own `Intent` and refuses a config whose labels are not exactly the Kestrel intents.

use std::fmt::Debug;

use kestrel_core::{Intent, KestrelError, Request, Tier, TierModel};

/// The labels of a list file under datasets/contracts/: one per line; blank lines and `#` comments
/// are skipped (as experiments/tier4/b3_dev.py reads them).
pub fn parse_list(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// How a cascade's labels relate to the label names in heads, prompts and output rows.
pub trait LabelSpace {
    type Label: Copy + Debug;
    /// The label called `name` (a head class, a rule's intent label), if this space has one.
    fn label(&self, name: &str) -> Option<Self::Label>;
    /// Every label with its name, in list order: the answers Tier 4 may give.
    fn answers(&self) -> Vec<(Self::Label, &str)>;
    fn name(&self, label: Self::Label) -> &str;
}

/// Kestrel's nine intents (ADR-010).
pub struct Intents;

impl Intents {
    /// Kestrel's intents, if `list` names exactly them, in `Intent::ALL` order.
    pub fn check(list: &[String]) -> Result<Self, KestrelError> {
        if list
            .iter()
            .map(String::as_str)
            .eq(Intent::ALL.map(Intent::label))
        {
            Ok(Intents)
        } else {
            Err(KestrelError::Invalid(format!(
                "the config's {} labels (first {:?}) are not the 9 Kestrel intents",
                list.len(),
                list.first().map_or("", String::as_str)
            )))
        }
    }
}

impl LabelSpace for Intents {
    type Label = Intent;

    fn label(&self, name: &str) -> Option<Intent> {
        Intent::from_label(name)
    }

    fn answers(&self) -> Vec<(Intent, &str)> {
        Intent::ALL.iter().map(|&i| (i, i.label())).collect()
    }

    fn name(&self, label: Intent) -> &str {
        label.label()
    }
}

/// Positions in a label list, such as CLINC150's 150 intents followed by `oos`.
pub struct LabelList(Vec<String>);

impl LabelList {
    pub fn new(names: Vec<String>) -> Result<Self, KestrelError> {
        if names.is_empty() || (1..names.len()).any(|i| names[..i].contains(&names[i])) {
            return Err(KestrelError::Invalid(
                "a label list must be non-empty and without repeats".into(),
            ));
        }
        Ok(Self(names))
    }
}

impl LabelSpace for LabelList {
    type Label = usize;

    fn label(&self, name: &str) -> Option<usize> {
        self.0.iter().position(|n| n == name)
    }

    fn answers(&self) -> Vec<(usize, &str)> {
        self.0.iter().map(String::as_str).enumerate().collect()
    }

    fn name(&self, label: usize) -> &str {
        &self.0[label]
    }
}

/// Carries a tier that answers Kestrel intents (Tier 1, the rules) into another label space. An
/// answer naming a label the space lacks becomes "no opinion", as a Kestrel rule match on
/// CLINC150 is in experiments/router/policies.py.
pub struct MapLabels<'s, S, T> {
    inner: T,
    space: &'s S,
}

impl<'s, S, T> MapLabels<'s, S, T> {
    pub fn new(inner: T, space: &'s S) -> Self {
        Self { inner, space }
    }
}

impl<S: LabelSpace, T: TierModel> TierModel<S::Label> for MapLabels<'_, S, T> {
    fn tier(&self) -> Tier {
        self.inner.tier()
    }

    fn est_cost_ms(&self) -> u32 {
        self.inner.est_cost_ms()
    }

    fn classify(&self, req: &Request) -> Result<Vec<(S::Label, f32)>, KestrelError> {
        let mapped: Option<Vec<_>> = (self.inner.classify(req)?.into_iter())
            .map(|(i, p)| self.space.label(i.label()).map(|l| (l, p)))
            .collect();
        Ok(mapped.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(name: &str) -> Vec<String> {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../datasets/contracts/");
        parse_list(&std::fs::read_to_string(format!("{dir}{name}")).unwrap())
    }

    fn list(names: &[&str]) -> LabelList {
        LabelList::new(names.iter().map(|n| n.to_string()).collect()).unwrap()
    }

    #[test]
    fn label_files_skip_blank_lines_and_comments() {
        assert_eq!(parse_list("# header\na\n\nb\n"), ["a", "b"]);
        let clinc = contract("clinc150_labels.txt");
        assert_eq!((clinc.len(), clinc.last().unwrap().as_str()), (151, "oos"));
    }

    #[test]
    fn only_the_kestrel_intent_list_gives_the_intent_space() {
        assert!(Intents::check(&contract("intents.txt")).is_ok());
        assert!(Intents::check(&contract("clinc150_labels.txt")).is_err());
        let mut swapped = contract("intents.txt");
        swapped.swap(0, 1);
        assert!(Intents::check(&swapped).is_err());
    }

    #[test]
    fn a_label_list_indexes_its_names() {
        let l = list(&["search_notes", "create_task", "oos"]);
        assert_eq!(l.label("oos"), Some(2));
        assert_eq!(l.label("chitchat"), None);
        assert_eq!(l.name(1), "create_task");
        assert_eq!(
            l.answers(),
            [(0, "search_notes"), (1, "create_task"), (2, "oos")]
        );
        assert!(LabelList::new(vec![]).is_err());
        assert!(LabelList::new(vec!["a".into(), "a".into()]).is_err());
    }

    #[test]
    fn the_intent_space_names_every_intent() {
        let answers = Intents.answers();
        assert_eq!(answers.len(), 9);
        for (i, name) in answers {
            assert_eq!((Intents.label(name), Intents.name(i)), (Some(i), name));
        }
    }

    /// A tier with a fixed answer in Kestrel intents.
    struct Fixed(Vec<(Intent, f32)>);

    impl TierModel for Fixed {
        fn tier(&self) -> Tier {
            Tier::Rules
        }
        fn est_cost_ms(&self) -> u32 {
            1
        }
        fn classify(&self, _: &Request) -> Result<Vec<(Intent, f32)>, KestrelError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn rule_answers_move_into_the_label_space_or_become_no_opinion() {
        let l = list(&["search_notes", "create_task", "oos"]);
        let req = Request::new(1, "remind me to call asha");
        let known = MapLabels::new(Fixed(vec![(Intent::CreateTask, 0.97)]), &l);
        assert_eq!(known.classify(&req).unwrap(), [(1, 0.97)]);
        assert_eq!((known.tier(), known.est_cost_ms()), (Tier::Rules, 1));
        let unknown = MapLabels::new(Fixed(vec![(Intent::Chitchat, 0.97)]), &l);
        assert!(unknown.classify(&req).unwrap().is_empty());
    }
}
