//! v3: notes found by meaning (ADR-039). The keyword search (kestrel-tools' Notes::search, frozen
//! for the research) runs first; when it finds fewer notes than asked, src/act.rs adds the notes
//! this index finds by meaning. Meaning: the Tier-3 e5 encoder, "query: " + the query (a query with
//! a common Hindi word is first put into English by the writer, since e5 does not know Romanized
//! Hindi) against "passage: " + each chunk of a note (its title and one paragraph); a note's score
//! is its best chunk's cosine; the closest note at or above θ is added (more added wrong notes on
//! dev). θ, k, the count added and the translation are the dev run's (experiments/v3/notes_meaning.py → experiments/configs/notes_meaning_v0.json). Chunk
//! vectors are kept in a cache file under a hash of each note's text: only changed notes are
//! embedded again.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use kestrel_core::KestrelError;
use kestrel_core::embedder::Embedder;
use kestrel_core::normalize::normalize;
use kestrel_tools::notes::Notes;
use kestrel_tools::tools::Note;
use serde::Deserialize;
use serde_json::{Value, json};

/// Where the service finds θ, k and whether to translate (written by notes_meaning.py).
pub const CONFIG: &str = "experiments/configs/notes_meaning_v0.json";

#[derive(Debug, Deserialize)]
pub struct MeaningConfig {
    pub encoder_file: String,
    pub theta: f32,
    pub k: usize,
    /// Notes added by meaning at most (the closest first).
    pub added: usize,
    pub translate: bool,
}

impl MeaningConfig {
    pub fn load(path: &Path) -> Result<Self, KestrelError> {
        let bad = |e: String| KestrelError::Invalid(format!("{}: {e}", path.display()));
        let text = std::fs::read_to_string(path).map_err(|e| bad(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| bad(e.to_string()))
    }
}

/// Notes close in meaning to a query, best first.
pub trait Meaning {
    fn find(&mut self, query: &str) -> Vec<Note>;
    /// At most this many results in all when notes by meaning are added (the dev run's k).
    fn k(&self) -> usize;
    /// The `n` closest notes, best first, when the closest is at or above θ; none otherwise (the
    /// notes Q&A's gate, experiments/v3/notes_qa.py: the next ones may be below θ).
    fn closest(&mut self, query: &str, n: usize) -> Vec<Note>;
}

/// The note's title (its first line that is not "tags:") with each paragraph after it.
pub fn chunks(text: &str) -> Vec<String> {
    let lines: Vec<&str> = (text.lines())
        .filter(|l| !l.to_lowercase().starts_with("tags:"))
        .collect();
    let Some(at) = lines.iter().position(|l| !l.trim().is_empty()) else {
        return Vec::new();
    };
    let title = lines[at].trim();
    let paras: Vec<String> = (lines[at + 1..].join("\n").split("\n\n"))
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .collect();
    if paras.is_empty() {
        return vec![title.to_string()];
    }
    paras.iter().map(|p| format!("{title}. {p}")).collect()
}

/// e5 does not know Romanized Hindi: a query with any common Hindi word goes through English.
pub fn needs_translation(query: &str) -> bool {
    (query.split_whitespace()).any(|w| crate::kinds::hindi_word(&w.to_lowercase()))
}

/// FNV-1a: a stable hash of a note's text across runs (std's hashers promise no stability).
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Puts a query into English (the writer), or None when it cannot.
pub type Translate<'a> = Box<dyn Fn(&str) -> Option<String> + 'a>;

pub struct MeaningIndex<'a> {
    embedder: Box<dyn Embedder + 'a>,
    notes: Notes,
    theta: f32,
    k: usize,
    added: usize,
    translate: Option<Translate<'a>>,
    cache: Option<PathBuf>,
    /// What the vectors were made with; a cache made with anything else is not used.
    tag: String,
    /// Note name -> (hash of its text, one vector per chunk).
    vectors: BTreeMap<String, (u64, Vec<Vec<f32>>)>,
}

impl<'a> MeaningIndex<'a> {
    pub fn new(
        embedder: Box<dyn Embedder + 'a>,
        notes: Notes,
        theta: f32,
        k: usize,
        added: usize,
        tag: &str,
    ) -> Self {
        Self {
            embedder,
            notes,
            theta,
            k,
            added,
            translate: None,
            cache: None,
            tag: tag.into(),
            vectors: BTreeMap::new(),
        }
    }

    pub fn with_translation(mut self, f: Translate<'a>) -> Self {
        self.translate = Some(f);
        self
    }

    /// Vectors kept in `file`: read now when it was made with the same tag, written after changes.
    pub fn with_cache(mut self, file: PathBuf) -> Self {
        let saved: Option<Value> =
            (std::fs::read_to_string(&file).ok()).and_then(|t| serde_json::from_str(&t).ok());
        if let Some(v) = saved.filter(|v| v["tag"] == self.tag.as_str())
            && let Some(notes) = v["notes"].as_object()
        {
            for (name, n) in notes {
                let chunks: Option<Vec<Vec<f32>>> =
                    serde_json::from_value(n["chunks"].clone()).ok();
                if let (Some(h), Some(c)) = (n["hash"].as_u64(), chunks) {
                    self.vectors.insert(name.clone(), (h, c));
                }
            }
        }
        self.cache = Some(file);
        self
    }

    /// Written aside and renamed, so a reader never sees half a file; a failed write only costs
    /// the next start the embedding.
    fn save(&self) {
        let Some(file) = &self.cache else { return };
        let notes: serde_json::Map<String, Value> = (self.vectors.iter())
            .map(|(name, (h, c))| (name.clone(), json!({"hash": h, "chunks": c})))
            .collect();
        let tmp = file.with_extension(format!("tmp{}", std::process::id()));
        let body = json!({"tag": self.tag, "notes": notes}).to_string();
        if std::fs::write(&tmp, body).is_ok() {
            let _ = std::fs::rename(&tmp, file);
        }
        let _ = std::fs::remove_file(&tmp);
    }

    fn try_find(&mut self, query: &str) -> Result<Vec<Note>, KestrelError> {
        let theta = self.theta;
        let scored = self.scored(query)?;
        Ok((scored.into_iter())
            .take(self.added)
            .take_while(|(s, _)| *s >= theta)
            .map(|(_, n)| n)
            .collect())
    }

    /// Every note with its score for `query`, best first (ties by name).
    fn scored(&mut self, query: &str) -> Result<Vec<(f32, Note)>, KestrelError> {
        // Every note, as the keyword search sees them: an empty query has no term to miss.
        let all = self.notes.search("", u32::MAX);
        let names: HashSet<&str> = all.iter().map(|n| n.name.as_str()).collect();
        let before = self.vectors.len();
        self.vectors.retain(|n, _| names.contains(n.as_str()));
        let mut changed = self.vectors.len() != before;
        for note in &all {
            let h = fnv(&note.text);
            if self.vectors.get(&note.name).map(|v| v.0) == Some(h) {
                continue;
            }
            let vs = (chunks(&note.text).iter())
                .map(|c| self.embedder.embed(&format!("passage: {}", normalize(c))))
                .collect::<Result<Vec<_>, _>>()?;
            self.vectors.insert(note.name.clone(), (h, vs));
            changed = true;
        }
        if changed {
            self.save();
        }
        let asked = match &self.translate {
            Some(t) if needs_translation(query) => (t(query))
                .map(|a| a.trim().trim_end_matches('.').to_string())
                .unwrap_or_else(|| query.to_string()),
            _ => query.to_string(),
        };
        let q = self
            .embedder
            .embed(&format!("query: {}", normalize(&asked)))?;
        let mut scored: Vec<(f32, Note)> = (all.into_iter())
            .filter_map(|n| {
                let (_, vs) = self.vectors.get(&n.name)?;
                let best = vs
                    .iter()
                    .map(|v| dot(v, &q))
                    .fold(f32::NEG_INFINITY, f32::max);
                Some((best, n))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        Ok(scored)
    }
}

impl Meaning for MeaningIndex<'_> {
    /// A failure (the encoder) finds nothing: the keyword hits stand alone.
    fn find(&mut self, query: &str) -> Vec<Note> {
        self.try_find(query).unwrap_or_default()
    }

    fn k(&self) -> usize {
        self.k
    }

    fn closest(&mut self, query: &str, n: usize) -> Vec<Note> {
        let theta = self.theta;
        match self.scored(query) {
            Ok(scored) if scored.first().is_some_and(|(s, _)| *s >= theta) => {
                scored.into_iter().take(n).map(|(_, n)| n).collect()
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn a_note_is_cut_into_its_title_with_each_paragraph_as_in_python() {
        // experiments/v3/test_notes_meaning.py's cases.
        let text = "tags: money\nMonthly budget\n\nRent 12000.\n\nCut subscriptions.\n";
        assert_eq!(
            chunks(text),
            [
                "Monthly budget. Rent 12000.",
                "Monthly budget. Cut subscriptions."
            ]
        );
        assert_eq!(chunks("Home router\n"), ["Home router"]);
        assert_eq!(
            chunks("Title\n\nline one\nline   two\n"),
            ["Title. line one line two"]
        );
        assert!(chunks("\n\n").is_empty());
    }

    #[test]
    fn a_query_with_any_hindi_word_is_translated_first() {
        assert!(needs_translation("har mahine paise bachana"));
        assert!(needs_translation("maa ke liye tohfa"));
        assert!(!needs_translation("present for my mother"));
        assert!(!needs_translation("how to cook lentils"));
    }

    /// Vectors by topic words: money, plants, sleep, the router, and a small rest. Counts its calls.
    struct Topics(Rc<Cell<usize>>);

    impl Embedder for Topics {
        fn dim(&self) -> usize {
            5
        }
        fn embed(&self, text: &str) -> Result<Vec<f32>, KestrelError> {
            self.0.set(self.0.get() + 1);
            let has = |ws: &[&str]| ws.iter().filter(|w| text.contains(*w)).count() as f32;
            let v = [
                has(&["money", "budget", "rent"]),
                has(&["plant", "water", "tulsi"]),
                has(&["sleep", "night"]),
                has(&["router", "password"]),
                0.1,
            ];
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            Ok(v.iter().map(|x| x / n).collect())
        }
    }

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kestrel_meaning_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("docs")).unwrap();
        let w = |n: &str, t: &str| std::fs::write(d.join(n), t).unwrap();
        w(
            "2026-09-02-budget.md",
            "tags: money\nMonthly budget\n\nRent 12000.\n\nSave money first.\n",
        );
        w(
            "2026-10-06-plants.md",
            "Balcony plants\n\nWater the tulsi every morning.\n",
        );
        w("docs/wifi.md", "Home router\n\nChange the password.\n");
        d.canonicalize().unwrap()
    }

    fn index(dir: &Path, calls: &Rc<Cell<usize>>) -> MeaningIndex<'static> {
        let sandbox = kestrel_tools::sandbox::Sandbox::new(&[dir.to_path_buf()]).unwrap();
        MeaningIndex::new(
            Box::new(Topics(calls.clone())),
            Notes::new(sandbox),
            0.8,
            3,
            3,
            "topics",
        )
    }

    #[test]
    fn notes_at_or_above_theta_come_best_first_with_their_text() {
        let dir = folder("find");
        let calls = Rc::new(Cell::new(0));
        let mut ix = index(&dir, &calls);
        let found = ix.find("saving money");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "2026-09-02-budget.md");
        assert!(found[0].text.contains("Rent 12000."), "{:?}", found[0].text);
        assert!(ix.find("guitar lessons").is_empty(), "nothing close enough");
    }

    #[test]
    fn the_closest_notes_come_together_when_the_first_reaches_theta() {
        let dir = folder("closest");
        let calls = Rc::new(Cell::new(0));
        let mut ix = index(&dir, &calls);
        let names = |ns: Vec<Note>| ns.into_iter().map(|n| n.name).collect::<Vec<_>>();
        // The second note is far below θ, and still comes with the first.
        let two = names(ix.closest("how much rent", 2));
        assert_eq!(two.len(), 2);
        assert_eq!(two[0], "2026-09-02-budget.md");
        assert_eq!(
            names(ix.closest("how much rent", 1)),
            ["2026-09-02-budget.md"]
        );
        assert!(
            ix.closest("guitar lessons", 2).is_empty(),
            "the closest is below θ"
        );
    }

    #[test]
    fn at_most_the_added_count_of_closest_notes_is_found() {
        let dir = folder("added");
        std::fs::write(
            dir.join("2026-10-07-rent.md"),
            "Rent\n\nRent money is due.\n",
        )
        .unwrap();
        let calls = Rc::new(Cell::new(0));
        let sandbox = kestrel_tools::sandbox::Sandbox::new(std::slice::from_ref(&dir)).unwrap();
        let mut ix = MeaningIndex::new(
            Box::new(Topics(calls)),
            Notes::new(sandbox),
            0.8,
            3,
            1,
            "topics",
        );
        let found = ix.find("money");
        assert_eq!(found.len(), 1, "two notes are close, the closer one comes");
    }

    #[test]
    fn only_a_query_with_hindi_words_is_translated() {
        let dir = folder("translate");
        let calls = Rc::new(Cell::new(0));
        let asked = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let a = asked.clone();
        let mut ix = index(&dir, &calls).with_translation(Box::new(move |q| {
            a.borrow_mut().push(q.to_string());
            Some("Watering the plants.".into())
        }));
        let found = ix.find("paudhon ko paani");
        assert_eq!(found[0].name, "2026-10-06-plants.md");
        ix.find("plants and water");
        assert_eq!(*asked.borrow(), ["paudhon ko paani"]);
    }

    #[test]
    fn only_changed_notes_are_embedded_again_and_the_cache_survives_a_restart() {
        let dir = folder("cache");
        let cache = dir.join("cache.json");
        let calls = Rc::new(Cell::new(0));
        let mut ix = index(&dir, &calls).with_cache(cache.clone());
        ix.find("money");
        // 4 chunks (2 + 1 + 1) and the query.
        assert_eq!(calls.get(), 5);
        ix.find("money");
        assert_eq!(calls.get(), 6, "only the query again");
        std::fs::write(
            dir.join("docs/wifi.md"),
            "Home router\n\nNew password: rent money.\n",
        )
        .unwrap();
        std::fs::remove_file(dir.join("2026-10-06-plants.md")).unwrap();
        let found = ix.find("money");
        assert_eq!(calls.get(), 8, "the changed note's one chunk and the query");
        assert!(found.iter().all(|n| n.name != "2026-10-06-plants.md"));
        let calls2 = Rc::new(Cell::new(0));
        let mut again = index(&dir, &calls2).with_cache(cache.clone());
        again.find("money");
        assert_eq!(
            calls2.get(),
            1,
            "a new index reads the vectors from the cache"
        );
        let calls3 = Rc::new(Cell::new(0));
        let sandbox = kestrel_tools::sandbox::Sandbox::new(std::slice::from_ref(&dir)).unwrap();
        let mut other = MeaningIndex::new(
            Box::new(Topics(calls3.clone())),
            Notes::new(sandbox),
            0.8,
            3,
            3,
            "other",
        )
        .with_cache(cache);
        other.find("money");
        assert_eq!(
            calls3.get(),
            4,
            "a cache made with another encoder is not used"
        );
    }

    /// The dev run's English queries (no translation) through this index with the real encoder:
    /// the same best note at the same score (the log keeps 4 decimals).
    #[cfg(feature = "llama")]
    #[test]
    #[ignore = "needs models/ and --features llama"]
    fn scores_match_the_python_dev_run() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let encoder = kestrel_inference::llama::LlamaEncoder::load(
            &root.join("models/multilingual-e5-small-q8_0.gguf"),
            6,
        )
        .unwrap();
        let notes = root
            .join("datasets/kestrel_notesq_v0/notes")
            .canonicalize()
            .unwrap();
        let sandbox = kestrel_tools::sandbox::Sandbox::new(&[notes]).unwrap();
        let mut ix = MeaningIndex::new(
            Box::new(encoder.session().unwrap()),
            Notes::new(sandbox),
            0.83,
            3,
            1,
            "e5",
        );
        let log: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("experiments/logs/v3/notes/meaning_dev.json"))
                .unwrap(),
        )
        .unwrap();
        let mut checked = 0;
        for q in log["queries"].as_array().unwrap() {
            let text = q["text"].as_str().unwrap();
            if needs_translation(text) {
                continue;
            }
            let scored = ix.scored(text).unwrap();
            let (score, note) = &scored[0];
            assert_eq!(note.name, q["best"][0].as_str().unwrap(), "{text}");
            let want = q["best"][1].as_f64().unwrap() as f32;
            assert!((score - want).abs() < 2e-4, "{text}: {score} vs {want}");
            checked += 1;
        }
        assert!(checked >= 10, "{checked}");
    }
}
