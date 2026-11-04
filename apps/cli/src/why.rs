//! "Why this answer" (v4.1, F): how the last answer was made, in plain words. Which way it took (rules,
//! the routing tiers, the kind head and the writer), what it read, and what left this device for it. Told
//! from what happened, never asked of the model, which could only guess.

/// The whole of a request for this: "why did the train stop" is a question about the world.
const ASKS: [&str; 18] = [
    "why this answer",
    "why that answer",
    "why did you say that",
    "why did you say this",
    "how do you know",
    "how do you know that",
    "how did you know",
    "how did you know that",
    "ye jawab kyun",
    "ye jawab kyun diya",
    "yeh jawab kyun",
    "yeh jawab kyun diya",
    "aisa kyun bola",
    "aisa kyun kaha",
    "tumhe kaise pata",
    "tumko kaise pata",
    "aapko kaise pata",
    "ye kaise pata",
];

/// Does `text` ask why the last answer was what it was?
pub fn parse(text: &str) -> bool {
    let said = text
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .trim()
        .to_lowercase();
    ASKS.contains(&said.as_str())
}

/// What happened in the last turn.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Why {
    /// The way it took, in a sentence or a few.
    pub way: Vec<String>,
    /// When the turn began, for what left this device.
    pub at: i64,
}

impl Why {
    pub fn new(at: i64, way: &str) -> Self {
        Self {
            way: vec![way.to_string()],
            at,
        }
    }
}

/// How the routing tiers read a request, in plain words.
pub fn routed<L: Copy + Labelled>(outcome: &kestrel_router::Outcome<L>) -> String {
    match outcome {
        kestrel_router::Outcome::Accepted(p) => {
            let who = match p.tier {
                kestrel_core::Tier::Rules => "A rule",
                kestrel_core::Tier::Encoder => "A small classifier",
                _ => "The model",
            };
            format!(
                "{who} on this device read this as: {} ({:.0}% sure).",
                p.intent.in_words(),
                p.confidence * 100.0
            )
        }
        kestrel_router::Outcome::Abstained { .. } => {
            "Nothing on this device could tell what this asks for.".to_string()
        }
    }
}

/// What a routing label says, in words.
pub trait Labelled {
    fn in_words(&self) -> String;
}

impl Labelled for kestrel_core::Intent {
    fn in_words(&self) -> String {
        self.label().replace('_', " ")
    }
}

/// The lines for a request "why": the way, then what left this device (kind, host, what).
pub fn lines(why: Option<&Why>, left: &[(String, String, String)]) -> Vec<String> {
    let Some(why) = why else {
        return vec!["There is no answer to explain yet.".to_string()];
    };
    let mut out = why.way.clone();
    if left.is_empty() {
        out.push("Nothing left this device for it.".to_string());
    } else {
        out.push("What left this device for it:".to_string());
        out.extend(
            left.iter()
                .map(|(kind, host, what)| format!("- {kind} · {host} · {what}")),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_why_this_answer_is_told_from_asking_why_of_the_world() {
        for text in [
            "why this answer",
            "Why this answer?",
            "why did you say that",
            "why that answer",
            "how do you know",
            "how did you know that",
            "ye jawab kyun",
            "ye jawab kyun diya",
            "aisa kyun bola",
            "tumhe kaise pata",
        ] {
            assert!(parse(text), "{text}");
        }
        for text in [
            "why is the sky blue",
            "why did the train stop",
            "how do you make tea",
            "ye kyun hota hai",
            "hello",
            "write why this answer matters",
        ] {
            assert!(!parse(text), "{text}");
        }
    }

    #[test]
    fn with_nothing_to_explain_it_says_so() {
        assert_eq!(lines(None, &[]), ["There is no answer to explain yet."]);
    }

    #[test]
    fn it_tells_the_way_and_what_left_or_that_nothing_did() {
        let why = Why::new(5, "By rules: a task command, no model.");
        assert_eq!(
            lines(Some(&why), &[]),
            [
                "By rules: a task command, no model.",
                "Nothing left this device for it."
            ]
        );
        let left = vec![(
            "search".to_string(),
            "searx.example.org".to_string(),
            "mango season".to_string(),
        )];
        assert_eq!(
            lines(Some(&why), &left),
            [
                "By rules: a task command, no model.",
                "What left this device for it:",
                "- search · searx.example.org · mango season",
            ]
        );
    }
}
