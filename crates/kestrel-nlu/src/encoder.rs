//! Tier 3: frozen sentence encoder + trained linear (softmax-regression) head (blueprint Ch. 11.2,
//! A.9, Ch. 33.2). The head is trained and calibrated in Python (experiments/tier3/head.py) and
//! read here from `head.json` + `head.bin` (little-endian f32, row-major [K x D]) +
//! `head_bias.bin` ([K] f32). Rust does the matrix-vector product and the temperature softmax.
//!
//! Changes from A.9: the bias length is checked (A.9 indexed `b[k]` unchecked, a panic on a short
//! bias); the temperature must be finite and > 0; the cost is the measured p95 passed in, not a
//! hard-coded 30 ms; head classes are label strings, so a CLINC150 head loads too, and
//! `EncoderTier` maps each one into the cascade's label type (Kestrel intents, or a benchmark's
//! label indices, ADR-019), refusing a class it cannot map; the toy hashing embedder is test-only.

use std::fs;
use std::path::Path;

use kestrel_core::prob::softmax;
use kestrel_core::{Intent, KestrelError, Request, Tier, TierModel};
use serde::Deserialize;

/// Defined in core so the llama.cpp embedder (kestrel-inference) needn't depend on this crate.
pub use kestrel_core::Embedder;

const FORMAT: u32 = 1;

/// The fields of head.json that Rust needs; provenance fields are ignored here.
#[derive(Deserialize)]
struct HeadFile {
    format: u32,
    classes: Vec<String>,
    dim: usize,
    temperature: f32,
    weights: String,
    bias: String,
    #[serde(default)]
    input_prefix: String,
}

#[derive(Debug, Clone)]
pub struct LinearHead {
    classes: Vec<String>,
    w: Vec<f32>, // K * dim, row-major
    b: Vec<f32>, // K
    dim: usize,
    temperature: f32,
    input_prefix: String,
}

impl LinearHead {
    pub fn new(
        classes: Vec<String>,
        w: Vec<f32>,
        b: Vec<f32>,
        dim: usize,
        temperature: f32,
        input_prefix: &str,
    ) -> Result<Self, KestrelError> {
        let k = classes.len();
        if k == 0 || dim == 0 {
            return Err(invalid("head needs at least one class and dimension"));
        }
        if w.len() != k * dim || b.len() != k {
            return Err(invalid(format!(
                "head shapes disagree: {k} classes x {dim} dims needs {} weights and {k} biases, got {} and {}",
                k * dim,
                w.len(),
                b.len()
            )));
        }
        // `!(t > 0.0)` is also true for NaN.
        if !(temperature > 0.0 && temperature.is_finite()) {
            return Err(invalid(format!(
                "temperature must be finite and > 0, got {temperature}"
            )));
        }
        if !w.iter().chain(&b).all(|v| v.is_finite()) {
            return Err(invalid("head weights or bias are not finite"));
        }
        Ok(Self {
            classes,
            w,
            b,
            dim,
            temperature,
            input_prefix: input_prefix.to_string(),
        })
    }

    /// Reads a head exported by experiments/tier3/head.py from `dir`.
    pub fn load(dir: &Path) -> Result<Self, KestrelError> {
        let json = fs::read_to_string(dir.join("head.json")).map_err(|e| io("head.json", e))?;
        let h: HeadFile =
            serde_json::from_str(&json).map_err(|e| invalid(format!("head.json: {e}")))?;
        if h.format != FORMAT {
            return Err(invalid(format!(
                "head format {} (expected {FORMAT})",
                h.format
            )));
        }
        let w = read_f32s(dir, &h.weights)?;
        let b = read_f32s(dir, &h.bias)?;
        Self::new(h.classes, w, b, h.dim, h.temperature, &h.input_prefix)
    }

    pub fn classes(&self) -> &[String] {
        &self.classes
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn temperature(&self) -> f32 {
        self.temperature
    }

    pub fn input_prefix(&self) -> &str {
        &self.input_prefix
    }

    /// Raw scores W x + b, one per class.
    pub fn logits(&self, x: &[f32]) -> Result<Vec<f32>, KestrelError> {
        if x.len() != self.dim {
            return Err(invalid(format!(
                "embedding has {} dims, head expects {}",
                x.len(),
                self.dim
            )));
        }
        Ok(self
            .w
            .chunks_exact(self.dim)
            .zip(&self.b)
            .map(|(row, b)| b + row.iter().zip(x).map(|(w, x)| w * x).sum::<f32>())
            .collect())
    }

    /// Calibrated class probabilities: softmax(logits / T).
    pub fn probs(&self, x: &[f32]) -> Result<Vec<f32>, KestrelError> {
        Ok(softmax(&self.logits(x)?, self.temperature))
    }
}

pub struct EncoderTier<E: Embedder, L = Intent> {
    embedder: E,
    head: LinearHead,
    /// Head class index -> label.
    labels: Vec<L>,
    est_cost_ms: u32,
}

impl<E: Embedder, L> EncoderTier<E, L> {
    /// `est_cost_ms`: measured p95 of embed + head on the target machine (TierModel contract).
    /// `label_of` maps a head class name to the cascade's label (`Intent::from_label` for Kestrel).
    pub fn new(
        embedder: E,
        head: LinearHead,
        est_cost_ms: u32,
        label_of: impl Fn(&str) -> Option<L>,
    ) -> Result<Self, KestrelError> {
        if embedder.dim() != head.dim() {
            return Err(invalid(format!(
                "embedder gives {} dims, head expects {}",
                embedder.dim(),
                head.dim()
            )));
        }
        let labels = head
            .classes()
            .iter()
            .map(|c| label_of(c).ok_or_else(|| invalid(format!("head class {c:?} has no label"))))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            embedder,
            head,
            labels,
            est_cost_ms,
        })
    }
}

impl<E: Embedder, L: Copy> TierModel<L> for EncoderTier<E, L> {
    fn tier(&self) -> Tier {
        Tier::Encoder
    }

    fn est_cost_ms(&self) -> u32 {
        self.est_cost_ms
    }

    fn classify(&self, req: &Request) -> Result<Vec<(L, f32)>, KestrelError> {
        let input = format!("{}{}", self.head.input_prefix(), req.text);
        let probs = self.head.probs(&self.embedder.embed(&input)?)?;
        Ok(self.labels.iter().copied().zip(probs).collect())
    }
}

fn invalid(msg: impl Into<String>) -> KestrelError {
    KestrelError::Invalid(msg.into())
}

fn io(what: &str, e: std::io::Error) -> KestrelError {
    KestrelError::Storage(format!("{what}: {e}"))
}

/// Little-endian f32 values from `name`, which must be a plain file inside `dir`.
fn read_f32s(dir: &Path, name: &str) -> Result<Vec<f32>, KestrelError> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(invalid(format!(
            "head file {name:?} must be a file name inside the head directory"
        )));
    }
    let bytes = fs::read(dir.join(name)).map_err(|e| io(name, e))?;
    let (words, rest) = bytes.as_chunks::<4>();
    if !rest.is_empty() {
        return Err(invalid(format!(
            "{name}: {} bytes is not a whole number of f32",
            bytes.len()
        )));
    }
    Ok(words.iter().map(|w| f32::from_le_bytes(*w)).collect())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use super::*;

    fn parity_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../datasets/contracts/head_parity")
    }

    /// A fresh empty directory under the system temp dir (no tempfile crate needed).
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel-nlu-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn copy_fixture(name: &str) -> PathBuf {
        let d = scratch(name);
        for f in ["head.json", "head.bin", "head_bias.bin"] {
            fs::copy(parity_dir().join(f), d.join(f)).unwrap();
        }
        d
    }

    fn labels(n: usize) -> Vec<String> {
        Intent::ALL[..n]
            .iter()
            .map(|i| {
                serde_json::to_value(i)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    /// Loads the head in `dir` and checks its probabilities against Python's on the 100 inputs
    /// in `dir/cases.json` (written by experiments/tier3/head.py or train.py).
    fn check_parity(dir: &Path) {
        let head = LinearHead::load(dir).unwrap();
        let cases: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("cases.json")).unwrap()).unwrap();
        let tol = cases["tolerance"].as_f64().unwrap();
        let inputs = cases["inputs"].as_array().unwrap();
        let probs = cases["probs"].as_array().unwrap();
        assert_eq!(inputs.len(), 100);
        assert_eq!(probs.len(), 100);
        let mut worst = 0.0f64;
        for (x, want) in inputs.iter().zip(probs) {
            let x: Vec<f32> = x
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect();
            let got = head.probs(&x).unwrap();
            for (g, w) in got.iter().zip(want.as_array().unwrap()) {
                worst = worst.max((*g as f64 - w.as_f64().unwrap()).abs());
            }
        }
        assert!(worst <= tol, "max |rust - python| = {worst:e} > {tol:e}");
    }

    #[test]
    fn matches_python_probabilities_on_the_parity_fixture() {
        check_parity(&parity_dir());
    }

    /// The same check on a real trained head (not in CI: heads are not committed), e.g.
    /// `KESTREL_HEAD_DIR=experiments/artifacts/tier3/intents_v0-multilingual-e5-small-q8_0
    ///  cargo test -p kestrel-nlu -- --ignored`
    #[test]
    #[ignore = "needs KESTREL_HEAD_DIR pointing at a head from experiments/tier3/train.py"]
    fn matches_python_probabilities_on_a_trained_head() {
        let dir = PathBuf::from(std::env::var("KESTREL_HEAD_DIR").expect("set KESTREL_HEAD_DIR"));
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        check_parity(&if dir.is_absolute() {
            dir
        } else {
            workspace.join(dir)
        });
    }

    #[test]
    fn load_reads_the_contract_fields() {
        let head = LinearHead::load(&parity_dir()).unwrap();
        assert_eq!(head.classes(), labels(9).as_slice());
        assert_eq!(head.dim(), 16);
        assert!((head.temperature() - 1.7).abs() < 1e-6);
        assert_eq!(head.input_prefix(), "");
    }

    #[test]
    fn load_rejects_a_truncated_weight_file() {
        let d = copy_fixture("truncated");
        let bytes = fs::read(d.join("head.bin")).unwrap();
        fs::write(d.join("head.bin"), &bytes[..bytes.len() - 4]).unwrap();
        assert!(LinearHead::load(&d).is_err());
    }

    #[test]
    fn load_rejects_an_unknown_format_version() {
        let d = copy_fixture("format");
        let json = fs::read_to_string(d.join("head.json")).unwrap();
        fs::write(
            d.join("head.json"),
            json.replace("\"format\": 1", "\"format\": 2"),
        )
        .unwrap();
        assert!(LinearHead::load(&d).is_err());
    }

    #[test]
    fn load_refuses_weight_paths_outside_the_head_directory() {
        let d = copy_fixture("escape");
        let json = fs::read_to_string(d.join("head.json")).unwrap();
        fs::write(
            d.join("head.json"),
            json.replace("\"head.bin\"", "\"../head.bin\""),
        )
        .unwrap();
        assert!(LinearHead::load(&d).is_err());
    }

    #[test]
    fn new_rejects_heads_that_would_misbehave() {
        let ok = || (labels(2), vec![0.0f32; 6], vec![0.0f32; 2]);
        let (c, w, b) = ok();
        assert!(LinearHead::new(c, w, b, 3, 1.0, "").is_ok());
        for t in [0.0f32, -1.0, f32::NAN, f32::INFINITY] {
            let (c, w, b) = ok();
            assert!(LinearHead::new(c, w, b, 3, t, "").is_err(), "T = {t}");
        }
        let (c, _, b) = ok();
        assert!(LinearHead::new(c, vec![0.0; 5], b, 3, 1.0, "").is_err()); // W too short
        let (c, w, _) = ok();
        assert!(LinearHead::new(c, w, vec![0.0; 1], 3, 1.0, "").is_err()); // A.9 panicked here
        let (c, mut w, b) = ok();
        w[0] = f32::NAN;
        assert!(LinearHead::new(c, w, b, 3, 1.0, "").is_err());
        assert!(LinearHead::new(vec![], vec![], vec![], 3, 1.0, "").is_err());
    }

    #[test]
    fn logits_are_w_x_plus_b() {
        let head = LinearHead::new(
            labels(2),
            vec![1.0, 2.0, 3.0, 4.0],
            vec![0.5, -0.5],
            2,
            1.0,
            "",
        )
        .unwrap();
        assert_eq!(head.logits(&[1.0, 1.0]).unwrap(), vec![3.5, 6.5]);
    }

    #[test]
    fn dimension_mismatch_is_an_error_not_a_panic() {
        let head = LinearHead::load(&parity_dir()).unwrap();
        assert!(head.probs(&[0.0; 15]).is_err());
        assert!(head.probs(&[0.0; 17]).is_err());
    }

    /// Toy bag-of-words hashing embedder (A.9's HashEmbedder): tests only, not semantic.
    struct HashEmbedder {
        dim: usize,
        seen: RefCell<Vec<String>>,
    }

    impl HashEmbedder {
        fn new(dim: usize) -> Self {
            Self {
                dim,
                seen: RefCell::new(Vec::new()),
            }
        }

        fn slot(&self, token: &str) -> usize {
            let h = token
                .bytes()
                .fold(5381u64, |a, b| a.wrapping_mul(33) ^ b as u64);
            (h % self.dim as u64) as usize
        }
    }

    impl Embedder for HashEmbedder {
        fn dim(&self) -> usize {
            self.dim
        }

        fn embed(&self, text: &str) -> Result<Vec<f32>, KestrelError> {
            self.seen.borrow_mut().push(text.to_string());
            let mut v = vec![0.0f32; self.dim];
            for tok in text.split_whitespace() {
                v[self.slot(tok)] += 1.0;
            }
            Ok(v)
        }
    }

    /// Class 0 = create_task keyed on "remind", class 1 = search_notes keyed on "search".
    fn embedder_and_head(temperature: f32, prefix: &str) -> (HashEmbedder, LinearHead) {
        let e = HashEmbedder::new(64);
        let mut w = vec![0.0f32; 2 * 64];
        w[e.slot("remind")] = 4.0;
        w[64 + e.slot("search")] = 4.0;
        let classes = vec!["create_task".to_string(), "search_notes".to_string()];
        let head = LinearHead::new(classes, w, vec![0.0, 0.0], 64, temperature, prefix).unwrap();
        (e, head)
    }

    fn tier(temperature: f32, prefix: &str) -> EncoderTier<HashEmbedder> {
        let (e, head) = embedder_and_head(temperature, prefix);
        EncoderTier::new(e, head, 12, Intent::from_label).unwrap()
    }

    #[test]
    fn maps_head_classes_into_any_label_space() {
        // A benchmark's labels are positions in its label list; `oos` has no head class.
        let list = ["search_notes", "create_task", "oos"];
        let (e, head) = embedder_and_head(1.0, "");
        let t = EncoderTier::new(e, head, 12, |c| list.iter().position(|l| *l == c)).unwrap();
        let out = t.classify(&Request::new(1, "remind me")).unwrap();
        assert_eq!(out.iter().map(|(l, _)| *l).collect::<Vec<_>>(), [1, 0]);
        assert!(out[0].1 > 0.9);
    }

    #[test]
    fn picks_the_expected_intent_from_normalized_text() {
        // A.9's version embedded raw text, so "Remind" missed the "remind" slot.
        let out = tier(1.0, "")
            .classify(&Request::new(1, "Remind me to call Asha"))
            .unwrap();
        assert_eq!(out[0].0, Intent::CreateTask);
        assert!(out[0].1 > 0.9);
        assert!((out.iter().map(|(_, p)| p).sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn temperature_softens_confidence() {
        let req = Request::new(1, "remind me");
        let sharp = tier(1.0, "").classify(&req).unwrap()[0].1;
        let soft = tier(3.0, "").classify(&req).unwrap()[0].1;
        assert!(soft < sharp);
    }

    #[test]
    fn embeds_prefix_plus_normalized_text_like_python() {
        let t = tier(1.0, "query: ");
        t.classify(&Request::new(1, "  Remind ME!")).unwrap();
        assert_eq!(t.embedder.seen.borrow().as_slice(), ["query: remind me"]);
    }

    #[test]
    fn reports_the_measured_cost_and_its_tier() {
        let t = tier(1.0, "");
        assert_eq!(t.est_cost_ms(), 12);
        assert_eq!(t.tier(), Tier::Encoder);
    }

    #[test]
    fn rejects_heads_whose_classes_are_not_kestrel_intents() {
        let classes = vec!["create_task".to_string(), "translate".to_string()];
        let head = LinearHead::new(classes, vec![0.0; 128], vec![0.0; 2], 64, 1.0, "").unwrap();
        assert!(EncoderTier::new(HashEmbedder::new(64), head, 12, Intent::from_label).is_err());
    }

    #[test]
    fn rejects_an_embedder_with_another_dimension() {
        let head = LinearHead::new(labels(2), vec![0.0; 128], vec![0.0; 2], 64, 1.0, "").unwrap();
        assert!(EncoderTier::new(HashEmbedder::new(32), head, 12, Intent::from_label).is_err());
    }
}
