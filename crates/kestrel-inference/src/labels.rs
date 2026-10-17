//! Constrained label decoding: Tier-4 confidence for classification (blueprint Ch. 11.1, 13:
//! "constrain decoding to label strings and take the probability mass over the label tokens,
//! then temperature-scale it"). Model-free, so the logic is tested without llama.cpp.
//!
//! Each label is a token sequence ending in an end-of-answer token, so no label is a prefix of
//! another. Decoding is greedy over the tokens that continue some label. The confidence of the
//! chosen label is the product, over its steps, of the softmax restricted to the allowed tokens.
//! Temperature does not change a greedy path, so `LabelChoice` keeps every step's allowed
//! logits and the confidence can be recomputed at any T (fitted on the calibration split).

use kestrel_core::KestrelError;

#[derive(Debug, Clone)]
pub struct LabelTrie {
    labels: Vec<Vec<i32>>,
}

impl LabelTrie {
    pub fn new(labels: Vec<Vec<i32>>) -> Result<Self, KestrelError> {
        if labels.is_empty() || labels.iter().any(Vec::is_empty) {
            return Err(invalid("labels must be non-empty token sequences"));
        }
        for (i, a) in labels.iter().enumerate() {
            for (j, b) in labels.iter().enumerate() {
                // Covers duplicates too: a label "starts with" its copy.
                if i != j && b.starts_with(a) {
                    return Err(invalid(format!(
                        "label {i} is a prefix of label {j}; end every label with an end token"
                    )));
                }
            }
        }
        Ok(Self { labels })
    }

    pub fn len(&self) -> usize {
        self.labels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    /// Tokens in the longest label (context budgeting).
    pub fn max_len(&self) -> usize {
        self.labels.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// Tokens that extend `prefix` toward at least one label: sorted, without duplicates.
    pub fn allowed(&self, prefix: &[i32]) -> Vec<i32> {
        let mut next: Vec<i32> = self
            .labels
            .iter()
            .filter(|l| l.len() > prefix.len() && l.starts_with(prefix))
            .map(|l| l[prefix.len()])
            .collect();
        next.sort_unstable();
        next.dedup();
        next
    }

    /// The index of the label equal to `prefix`, if `prefix` is a whole label.
    pub fn complete(&self, prefix: &[i32]) -> Option<usize> {
        self.labels.iter().position(|l| l == prefix)
    }
}

/// One constrained step: the allowed tokens, their logits, and the greedy pick (index into both).
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub allowed: Vec<i32>,
    pub logits: Vec<f32>,
    pub chosen: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LabelChoice {
    pub label: usize,
    pub steps: Vec<Step>,
}

impl LabelChoice {
    /// P(label) under the label-constrained distribution at temperature `t` (> 0).
    pub fn confidence(&self, t: f32) -> f32 {
        let t = f64::from(t);
        self.steps
            .iter()
            .map(|s| {
                let max = s.logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let w = |z: f32| ((f64::from(z) - f64::from(max)) / t).exp();
                w(s.logits[s.chosen]) / s.logits.iter().map(|&z| w(z)).sum::<f64>()
            })
            .product::<f64>() as f32
    }
}

/// Greedy constrained decoding. `next_logits(prefix)` returns the full-vocabulary logits after
/// the prompt followed by the label tokens chosen so far (`prefix`).
pub fn decode_label(
    trie: &LabelTrie,
    mut next_logits: impl FnMut(&[i32]) -> Result<Vec<f32>, KestrelError>,
) -> Result<LabelChoice, KestrelError> {
    let (mut prefix, mut steps) = (Vec::new(), Vec::new());
    loop {
        if let Some(label) = trie.complete(&prefix) {
            return Ok(LabelChoice { label, steps });
        }
        // Never empty: by construction `prefix` is a proper prefix of some label.
        let allowed = trie.allowed(&prefix);
        let z = next_logits(&prefix)?;
        let logits = allowed
            .iter()
            .map(|&tok| {
                usize::try_from(tok)
                    .ok()
                    .and_then(|i| z.get(i).copied())
                    .ok_or_else(|| invalid(format!("token {tok} outside the {} logits", z.len())))
            })
            .collect::<Result<Vec<f32>, _>>()?;
        // Greedy; the first maximum wins ties, so the result is deterministic.
        let chosen =
            (0..logits.len()).fold(0, |best, i| if logits[i] > logits[best] { i } else { best });
        prefix.push(allowed[chosen]);
        steps.push(Step {
            allowed,
            logits,
            chosen,
        });
    }
}

fn invalid(msg: impl Into<String>) -> KestrelError {
    KestrelError::Invalid(msg.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const END: i32 = 99;

    /// Labels: [1, 2, END], [1, 3, END], [4, END]  (two share the first token).
    fn trie() -> LabelTrie {
        LabelTrie::new(vec![vec![1, 2, END], vec![1, 3, END], vec![4, END]]).unwrap()
    }

    /// (token, logit) pairs; every other token of the vocabulary gets logit 0.
    type Logits<'a> = &'a [(i32, f32)];
    /// Logits to return after a given label prefix.
    type Entry<'a> = (&'a [i32], Logits<'a>);

    /// A fake model: logits over a 100-token vocabulary from a per-prefix table.
    fn model(table: &[Entry<'_>]) -> impl FnMut(&[i32]) -> Result<Vec<f32>, KestrelError> {
        type Owned = (Vec<i32>, Vec<(i32, f32)>);
        let table: Vec<Owned> = table
            .iter()
            .map(|(p, l)| (p.to_vec(), l.to_vec()))
            .collect();
        move |prefix: &[i32]| {
            let mut z = vec![0.0f32; 100];
            if let Some((_, l)) = table.iter().find(|(p, _)| p == prefix) {
                for &(tok, v) in l {
                    z[tok as usize] = v;
                }
            }
            Ok(z)
        }
    }

    #[test]
    fn allowed_tokens_follow_the_labels() {
        let t = trie();
        assert_eq!(t.allowed(&[]), vec![1, 4]);
        assert_eq!(t.allowed(&[1]), vec![2, 3]);
        assert_eq!(t.allowed(&[1, 2]), vec![END]);
        assert!(t.allowed(&[7]).is_empty());
        assert_eq!(t.complete(&[1, 3, END]), Some(1));
        assert_eq!(t.complete(&[1, 3]), None);
    }

    #[test]
    fn rejects_label_sets_that_cannot_be_decoded_unambiguously() {
        assert!(LabelTrie::new(vec![]).is_err());
        assert!(LabelTrie::new(vec![vec![1, END], vec![]]).is_err());
        assert!(LabelTrie::new(vec![vec![1, END], vec![1, END]]).is_err()); // duplicate
        assert!(LabelTrie::new(vec![vec![1], vec![1, 2]]).is_err()); // prefix of another
    }

    #[test]
    fn decodes_greedily_among_allowed_tokens_only() {
        // Token 50 has the highest logit but is not part of any label.
        let mut m = model(&[
            (&[], &[(50, 9.0), (1, 2.0), (4, 1.0)]),
            (&[1], &[(50, 9.0), (2, 0.0), (3, 1.0)]),
            (&[1, 3], &[(END, 0.0)]),
        ]);
        let c = decode_label(&trie(), &mut m).unwrap();
        assert_eq!(c.label, 1); // [1, 3, END]
        assert_eq!(c.steps.len(), 3);
        assert_eq!(c.steps[0].allowed, vec![1, 4]);
        assert_eq!(c.steps[2].allowed, vec![END]);
    }

    #[test]
    fn confidence_is_the_product_of_restricted_softmaxes() {
        let mut m = model(&[(&[], &[(1, 2.0), (4, 1.0)]), (&[1], &[(2, 0.0), (3, 1.0)])]);
        let c = decode_label(&trie(), &mut m).unwrap();
        let p1 = 2f32.exp() / (2f32.exp() + 1f32.exp());
        let p2 = 1f32.exp() / (1.0 + 1f32.exp());
        assert!((c.confidence(1.0) - p1 * p2).abs() < 1e-6); // last step has one choice: p = 1
        // T > 1 softens toward 1/2 * 1/2; T < 1 sharpens.
        assert!(c.confidence(3.0) < c.confidence(1.0));
        assert!(c.confidence(0.5) > c.confidence(1.0));
        assert!((c.confidence(1e6) - 0.25).abs() < 1e-3);
    }

    #[test]
    fn model_errors_propagate() {
        let r = decode_label(&trie(), |_| Err(KestrelError::Cancelled));
        assert!(matches!(r, Err(KestrelError::Cancelled)));
    }

    #[test]
    fn short_logit_vectors_are_an_error_not_a_panic() {
        let r = decode_label(&trie(), |_| Ok(vec![0.0; 3])); // token 4 is out of range
        assert!(matches!(r, Err(KestrelError::Invalid(_))));
    }
}
