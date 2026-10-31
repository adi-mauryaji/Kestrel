//! `kestrel calibrate` (v4 A2, ADR-049): what the kind fallback needs to know about a writer model,
//! measured on the dev splits of Kinds-v2 (cal and val; never test). Gemma's confidence is
//! temperature-scaled on cal (the temperature minimizing the binary cross-entropy of its
//! confidence against being right), then (τ, τ_g) are chosen on val by the rule fixed before
//! Kinds-v1's run (ADR-042). This is experiments/v3/kind_fallback.py's policy in Rust, so a
//! device without the Python experiments can measure a model it has.
//!
//! The run then measures the same writer's NotesQA and CodeTasks dev pass rates through the
//! service's own worker (src/pass_rates.rs); all of it is kept per model in device_profile.json.

use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};

use kestrel_inference::labels::Step;

use crate::kinds::Kind;

/// The search range for the confidence temperature (experiments/tier4/b3_dev.py T_BOUNDS).
const T_BOUNDS: (f64, f64) = (0.05, 20.0);
/// The grids the dev run judged (experiments/v3/kind_fallback.py TAUS, TAU_G).
pub const TAUS: [f32; 5] = [0.5, 0.6, 0.7, 0.8, 0.9];
pub const TAU_G: [f32; 5] = [0.5, 0.6, 0.7, 0.8, 0.9];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    Cal,
    Val,
}

/// One dev request with what the head and Gemma said about it.
pub struct Row {
    pub split: Split,
    pub gold: Kind,
    /// The head's probability per kind.
    pub head: Vec<(Kind, f32)>,
    /// Gemma's kind and its constrained decoding steps; None when it failed.
    pub gemma: Option<(Kind, Vec<Step>)>,
}

/// P(chosen) under the constrained distribution at temperature `t`, the product over the steps.
pub fn step_confidence(steps: &[Step], t: f64) -> f64 {
    steps
        .iter()
        .map(|s| {
            let max = s.logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let w = |z: f32| ((f64::from(z) - f64::from(max)) / t).exp();
            w(s.logits[s.chosen]) / s.logits.iter().map(|&z| w(z)).sum::<f64>()
        })
        .product()
}

/// The T minimizing the mean binary cross-entropy of `step_confidence(_, T)` against being right.
pub fn fit_temperature(items: &[(Vec<Step>, bool)]) -> Result<f64, String> {
    let right = items.iter().filter(|(_, ok)| *ok).count();
    if right == 0 || right == items.len() {
        return Err("calibration needs both right and wrong items".into());
    }
    let bce = |log_t: f64| {
        let t = log_t.exp();
        -items
            .iter()
            .map(|(steps, ok)| {
                let c = step_confidence(steps, t).clamp(1e-12, 1.0 - 1e-12);
                if *ok { c.ln() } else { (1.0 - c).ln() }
            })
            .sum::<f64>()
            / items.len() as f64
    };
    // Golden-section search over log T.
    let (lo, hi) = (T_BOUNDS.0.ln(), T_BOUNDS.1.ln());
    let phi = (5f64.sqrt() - 1.0) / 2.0;
    let (mut a, mut b) = (lo, hi);
    let (mut x1, mut x2) = (b - phi * (b - a), a + phi * (b - a));
    let (mut f1, mut f2) = (bce(x1), bce(x2));
    while b - a > 1e-6 {
        if f1 < f2 {
            b = x2;
            (x2, f2) = (x1, f1);
            x1 = b - phi * (b - a);
            f1 = bce(x1);
        } else {
            a = x1;
            (x1, f1) = (x2, f2);
            x2 = a + phi * (b - a);
            f2 = bce(x2);
        }
    }
    let best = (a + b) / 2.0;
    if (best - lo).min(hi - best) < 1e-3 {
        return Err(format!(
            "the temperature optimum is at the search bound {T_BOUNDS:?}: {:.4}",
            best.exp()
        ));
    }
    Ok(best.exp())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Decision {
    Take(Kind),
    Ask(Kind),
}

/// One request's decision. `gemma`: its kind and calibrated confidence, or None for the head alone.
pub fn decide(head: &[(Kind, f32)], tau: f32, gemma: Option<(Kind, f64)>, tau_g: f32) -> Decision {
    let (kind, p) = head
        .iter()
        .copied()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .expect("the head gives a probability per kind");
    if p >= tau || (kind == Kind::App && gemma.is_none()) {
        return Decision::Take(kind);
    }
    if let Some((g, c)) = gemma
        && c >= f64::from(tau_g)
    {
        return Decision::Take(g);
    }
    if kind == Kind::App {
        Decision::Take(Kind::App)
    } else {
        Decision::Ask(kind)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Score {
    pub n: usize,
    pub taken: usize,
    pub right: usize,
    pub skill_not_asked_for: usize,
    pub skill_sent_old_way: usize,
    pub asked: usize,
    pub asked_right_kind: usize,
}

impl Score {
    /// A skill the request did not ask for, plus a skill request sent the old way.
    pub fn wrong(&self) -> usize {
        self.skill_not_asked_for + self.skill_sent_old_way
    }
}

pub fn score(decisions: &[Decision], gold: &[Kind]) -> Score {
    let mut s = Score {
        n: gold.len(),
        ..Score::default()
    };
    for (d, &g) in decisions.iter().zip(gold) {
        match *d {
            Decision::Take(k) => {
                s.taken += 1;
                s.right += usize::from(k == g);
                s.skill_not_asked_for += usize::from(k != g && k != Kind::App);
                s.skill_sent_old_way += usize::from(k == Kind::App && g != Kind::App);
            }
            Decision::Ask(k) => {
                s.asked += 1;
                s.asked_right_kind += usize::from(k == g);
            }
        }
    }
    s
}

/// (τ, τ_g) by the rule fixed before Kinds-v1's run: fewest wrong decisions, then fewest asks,
/// then the lower τ (fewer Gemma calls), then the higher τ_g.
pub fn choose(grid: &[((f32, f32), Score)]) -> (f32, f32) {
    grid.iter()
        .min_by(|(a, sa), (b, sb)| {
            (sa.wrong(), sa.asked)
                .cmp(&(sb.wrong(), sb.asked))
                .then(a.0.total_cmp(&b.0))
                .then(b.1.total_cmp(&a.1))
        })
        .map(|(k, _)| *k)
        .expect("the grid is not empty")
}

/// What the fallback is run with for one writer model.
#[derive(Debug, Clone, PartialEq)]
pub struct Calibration {
    pub temperature: f32,
    pub tau: f32,
    pub tau_g: f32,
    /// The chosen pair's score on val.
    pub val: Score,
    pub cal_n: usize,
    pub cal_gemma_accuracy: f64,
}

pub fn calibrate(rows: &[Row]) -> Result<Calibration, String> {
    let cal: Vec<(&Row, &(Kind, Vec<Step>))> = rows
        .iter()
        .filter(|r| r.split == Split::Cal)
        .filter_map(|r| r.gemma.as_ref().map(|g| (r, g)))
        .collect();
    let items: Vec<(Vec<Step>, bool)> = cal
        .iter()
        .map(|(r, (k, steps))| (steps.clone(), *k == r.gold))
        .collect();
    let t = fit_temperature(&items)?;
    let val: Vec<&Row> = rows.iter().filter(|r| r.split == Split::Val).collect();
    if val.is_empty() {
        return Err("calibration needs val items".into());
    }
    // A Gemma that failed on val counts as `app` with no confidence, as in the dev run.
    let gemma: Vec<(Kind, f64)> = val
        .iter()
        .map(|r| match &r.gemma {
            Some((k, steps)) => (*k, step_confidence(steps, t)),
            None => (Kind::App, 0.0),
        })
        .collect();
    let gold: Vec<Kind> = val.iter().map(|r| r.gold).collect();
    let run = |tau: f32, tau_g: f32| {
        let decisions: Vec<Decision> = val
            .iter()
            .zip(&gemma)
            .map(|(r, g)| decide(&r.head, tau, Some(*g), tau_g))
            .collect();
        score(&decisions, &gold)
    };
    let grid: Vec<((f32, f32), Score)> = TAUS
        .iter()
        .flat_map(|&a| TAU_G.iter().map(move |&g| (a, g)))
        .map(|(a, g)| ((a, g), run(a, g)))
        .collect();
    let (tau, tau_g) = choose(&grid);
    let accuracy = items.iter().filter(|(_, ok)| *ok).count() as f64 / items.len() as f64;
    Ok(Calibration {
        temperature: t as f32,
        tau,
        tau_g,
        val: run(tau, tau_g),
        cal_n: items.len(),
        cal_gemma_accuracy: accuracy,
    })
}

/// One dev request as the dataset holds it.
#[derive(Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub text: String,
    pub gold: Kind,
}

/// The dev splits and nothing else: the test split is not nameable from here.
pub fn dev_splits(root: &Path) -> [(Split, PathBuf); 2] {
    let dir = root.join("datasets/kestrel_kinds_v2/splits");
    [
        (Split::Cal, dir.join("cal.jsonl")),
        (Split::Val, dir.join("val.jsonl")),
    ]
}

pub fn read_items(path: &Path) -> Result<Vec<Item>, Box<dyn Error>> {
    let bad = |l: usize, why: &str| format!("{}:{}: {why}", path.display(), l + 1);
    std::fs::read_to_string(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(n, l)| {
            let v: serde_json::Value =
                serde_json::from_str(l).map_err(|e| bad(n, &e.to_string()))?;
            let field = |k: &str| v[k].as_str().ok_or_else(|| bad(n, &format!("no {k}")));
            Ok(Item {
                id: field("id")?.to_string(),
                text: field("text")?.to_string(),
                gold: Kind::from_label(field("label")?).ok_or_else(|| bad(n, "not a kind"))?,
            })
        })
        .collect()
}

/// The profile's record of a calibration (`now`: Unix time).
pub fn calibrated(
    c: &Calibration,
    model: &str,
    file: (u64, u64),
    binary: (u64, u64),
    now: u64,
) -> kestrel_capability::Calibrated {
    kestrel_capability::Calibrated {
        model: model.into(),
        file,
        binary,
        temperature: c.temperature,
        tau: c.tau,
        tau_g: c.tau_g,
        val_n: c.val.n,
        val_wrong: c.val.wrong(),
        val_asked: c.val.asked,
        measured_at: now,
        notes_qa: None,
        code_tasks: None,
    }
}

/// `kestrel calibrate [CONFIG]`: the model slot's writer's kind fallback measured on Kinds-v2 cal
/// and val, kept in the device profile. About ten minutes of full CPU with the 2B writer.
pub fn run(args: &[String], out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let data = kestrel_platform::data_dir(kestrel_platform::Os::current(), &kestrel_platform::env);
    run_in(Path::new("."), data.as_deref(), args, out)
}

#[cfg(not(feature = "llama"))]
fn run_in(
    _: &Path,
    _: Option<&Path>,
    _: &[String],
    _: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    Err(
        "calibrate needs the llama build: cargo build --release -p kestrel-cli --features llama"
            .into(),
    )
}

#[cfg(feature = "llama")]
fn run_in(
    root: &Path,
    data: Option<&Path>,
    args: &[String],
    out: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    use kestrel_core::{Request, TierModel};

    let config = match args {
        [] => crate::capabilities::DEFAULT_CONFIG,
        [c] if !c.starts_with("--") => c,
        _ => return Err("usage: kestrel calibrate [CONFIG]".into()),
    };
    let data = data.ok_or("no data folder to keep the calibration in")?;
    // The model slot's writer (`kestrel model`), which the service switches to once this passes.
    let ctx = crate::capabilities::context(root, Some(data), Some(config))?;
    let id = (ctx.device.writer.as_deref())
        .ok_or("no writer model here: `kestrel model` shows what fits")?;
    let model = (ctx.catalog.models.iter())
        .find(|m| m.id == id)
        .ok_or("the writer is not in the catalog")?;
    let path = crate::slot::model_path(root, Some(data), &model.file);
    let file = crate::capabilities::file_id(&path)
        .ok_or_else(|| format!("{} is missing: download it first", path.display()))?;
    writeln!(
        out,
        "calibrating {id} (about ten minutes with the 2B model)..."
    )?;
    // The kind fallback in this process; its copy of the model is freed before the worker loads one.
    let c = {
        let models = crate::route_items::Models::load_for_writer(
            &root.join(config),
            &[3, 4],
            Some(&model.file),
        )?;
        let head = models.kinds_tier()?;
        // Only the steps are used: the temperature is what is being measured.
        let fallback = models.kinds_fallback_at(Some(1.0))?;
        let mut rows = Vec::new();
        for (split, file_path) in dev_splits(root) {
            let items = read_items(&file_path)?;
            writeln!(out, "{}: {} requests...", file_path.display(), items.len())?;
            for (n, item) in items.iter().enumerate() {
                let req = Request::new(n as u64, &item.text);
                let probs = head.classify(&req)?;
                // A fallback that fails on one request counts as failed there, as in the dev run.
                let gemma = fallback
                    .classify_choice(&req)
                    .ok()
                    .map(|(k, c)| (k, c.steps));
                rows.push(Row {
                    split,
                    gold: item.gold,
                    head: probs,
                    gemma,
                });
            }
        }
        calibrate(&rows)?
    };
    writeln!(out, "{}", report(&model.id, &c))?;
    let (notes_qa, code_tasks) = pass_rates(root, config, &model.file, out)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mut profile = crate::capabilities::load_profile(Some(data));
    let mut entry = calibrated(&c, &model.id, file, crate::capabilities::this_binary(), now);
    (entry.notes_qa, entry.code_tasks) = (Some(notes_qa), code_tasks);
    profile.calibrate(entry);
    std::fs::create_dir_all(data)?;
    std::fs::write(
        data.join("device_profile.json"),
        serde_json::to_string_pretty(&profile)? + "\n",
    )?;
    writeln!(
        out,
        "kept in {}; the service writes with {id} from its next start (`kestrel stop`).",
        data.join("device_profile.json").display()
    )?;
    Ok(())
}

/// NotesQA and CodeTasks dev through the service's own worker (`kestrel tier4-worker`) with
/// `writer`, and the service's notes by meaning; code is judged only where the sandbox runs.
#[cfg(feature = "llama")]
fn pass_rates(
    root: &Path,
    config: &str,
    writer: &str,
    out: &mut dyn Write,
) -> Result<
    (
        kestrel_capability::NotesQa,
        Option<kestrel_capability::CodeTasks>,
    ),
    Box<dyn Error>,
> {
    use crate::kinds::{Kind, Writer};
    use crate::pass_rates as pr;
    use crate::remote::{RemoteTier, SharedRemote};

    let args = crate::service::WorkerArgs {
        config: root.join(config),
        writer: Some(writer.into()),
        kinds_temperature: None,
    };
    let remote = std::rc::Rc::new(RemoteTier::new(
        std::env::current_exe()?,
        args.to_args(),
        0,
        std::time::Duration::from_secs(3600),
    ));
    let worker = SharedRemote(remote.clone());
    let encoder = crate::route_items::Models::load_for(&root.join(config), &[3])?;
    let mut index = encoder.meaning_index(&pr::notes_dir(root), None)?;
    // As the service does: a Hinglish question is first put into English by the writer.
    let mc = crate::meaning::MeaningConfig::load(Path::new(crate::meaning::CONFIG))?;
    if mc.translate {
        let translator = SharedRemote(remote.clone());
        index = index.with_translation(Box::new(move |q| {
            let ask = format!("translate to english: {q}");
            translator.write(Kind::Translate, &ask, &mut |_| {}).ok()
        }));
    }
    let questions = pr::read_questions(&pr::notes_dev(root))?;
    writeln!(out, "notes questions: {}...", questions.len())?;
    let mut graded = Vec::new();
    for q in &questions {
        let (seen, grade) = pr::run_question(&mut index, &worker, q);
        if grade != pr::Grade::Right {
            writeln!(out, "  {grade:?}: {} ({seen:?})", q.id)?;
        }
        graded.push((q, grade));
    }
    let notes = pr::notes_tally(&graded);
    let tasks = pr::read_code_tasks(&pr::code_dev(root))?;
    writeln!(out, "code requests: {}...", tasks.len())?;
    let mut results = Vec::new();
    for t in &tasks {
        match pr::run_code_task(&worker, t) {
            Ok(r) => {
                if !r.1 {
                    writeln!(out, "  fails: {}", t.id)?;
                }
                results.push(r)
            }
            Err(e) => {
                writeln!(out, "code not measured: {}", e.0)?;
                writeln!(out, "{}", pr::report(&notes, None))?;
                return Ok((notes, None));
            }
        }
    }
    let code = pr::code_tally(&results);
    writeln!(out, "{}", pr::report(&notes, Some(&code)))?;
    Ok((notes, Some(code)))
}

/// What the run found, in words.
pub fn report(model: &str, c: &Calibration) -> String {
    format!(
        "{model}: Gemma right on {:.0}% of {} cal requests; confidence temperature {:.2}.\n\
         chosen on val ({} requests): tau {}, tau_g {}: {} wrong decisions, {} questions asked.",
        c.cal_gemma_accuracy * 100.0,
        c.cal_n,
        c.temperature,
        c.val.n,
        c.tau,
        c.tau_g,
        c.val.wrong(),
        c.val.asked
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One constrained step over `logits` that picked `chosen`.
    fn step(logits: &[f32], chosen: usize) -> Step {
        Step {
            allowed: (0..logits.len() as i32).collect(),
            logits: logits.to_vec(),
            chosen,
        }
    }

    /// `top` with `p`; the rest shared by the other kinds, so `top` is the top while p > 1/7.
    fn head(top: Kind, p: f32) -> Vec<(Kind, f32)> {
        crate::kinds::KINDS
            .iter()
            .map(|&k| (k, if k == top { p } else { (1.0 - p) / 6.0 }))
            .collect()
    }

    #[test]
    fn a_step_s_confidence_is_the_chosen_logit_s_softmax_at_t() {
        let s = [step(&[2.0, 1.0], 0)];
        let p = 2f64.exp() / (2f64.exp() + 1f64.exp());
        assert!((step_confidence(&s, 1.0) - p).abs() < 1e-6);
        assert!(step_confidence(&s, 5.0) < step_confidence(&s, 1.0));
        // Two steps multiply.
        let two = [step(&[2.0, 1.0], 0), step(&[0.0, 0.0], 1)];
        assert!((step_confidence(&two, 1.0) - p * 0.5).abs() < 1e-6);
    }

    #[test]
    fn an_overconfident_model_gets_a_temperature_above_one() {
        // A gap of 6 logits says 99.7%, but the model is right 3 times in 5.
        let items: Vec<(Vec<Step>, bool)> = (0..50)
            .map(|i| (vec![step(&[6.0, 0.0], 0)], i % 5 < 3))
            .collect();
        let t = fit_temperature(&items).unwrap();
        assert!(t > 1.5, "{t}");
        let bce = |t: f64| {
            -items
                .iter()
                .map(|(s, ok)| {
                    let c = step_confidence(s, t).clamp(1e-12, 1.0 - 1e-12);
                    if *ok { c.ln() } else { (1.0 - c).ln() }
                })
                .sum::<f64>()
        };
        assert!(bce(t) < bce(1.0));
        // The optimum for 60% right is the temperature that makes the confidence 0.6.
        assert!((step_confidence(&items[0].0, t) - 0.6).abs() < 0.01);
    }

    #[test]
    fn a_temperature_needs_both_right_and_wrong_answers_and_an_optimum_inside_the_bounds() {
        let all_right: Vec<_> = (0..10)
            .map(|_| (vec![step(&[3.0, 0.0], 0)], true))
            .collect();
        assert!(fit_temperature(&all_right).unwrap_err().contains("both"));
        assert!(fit_temperature(&[]).is_err());
        // Sure and right only half the time: the optimum runs to the upper bound.
        let flat: Vec<_> = (0..20)
            .map(|i| (vec![step(&[9.0, 0.0], 0)], i % 2 == 0))
            .collect();
        assert!(fit_temperature(&flat).unwrap_err().contains("search bound"));
    }

    #[test]
    fn decide_follows_the_dev_run_s_policy() {
        use Decision::{Ask, Take};
        // Sure head: taken as it is.
        assert_eq!(
            decide(&head(Kind::Email, 0.9), 0.8, None, 0.7),
            Take(Kind::Email)
        );
        // Unsure skill, no Gemma: asked. Unsure app: the old way, taken.
        assert_eq!(
            decide(&head(Kind::Email, 0.5), 0.8, None, 0.7),
            Ask(Kind::Email)
        );
        assert_eq!(
            decide(&head(Kind::App, 0.5), 0.8, None, 0.7),
            Take(Kind::App)
        );
        // Gemma at or above τ_g decides; below it the head's answer stands.
        let g = |c| Some((Kind::Chat, c));
        assert_eq!(
            decide(&head(Kind::Email, 0.5), 0.8, g(0.7), 0.7),
            Take(Kind::Chat)
        );
        assert_eq!(
            decide(&head(Kind::Email, 0.5), 0.8, g(0.69), 0.7),
            Ask(Kind::Email)
        );
        assert_eq!(
            decide(&head(Kind::App, 0.5), 0.8, g(0.2), 0.7),
            Take(Kind::App)
        );
        // A sure head is not second-guessed.
        assert_eq!(
            decide(&head(Kind::Email, 0.8), 0.8, g(1.0), 0.7),
            Take(Kind::Email)
        );
    }

    #[test]
    fn a_score_counts_the_wrong_decisions_the_way_the_dev_run_did() {
        use Decision::{Ask, Take};
        let gold = [Kind::Email, Kind::App, Kind::Chat, Kind::Email, Kind::Code];
        let decisions = [
            Take(Kind::Email), // right
            Take(Kind::Chat),  // a skill the request did not ask for
            Take(Kind::App),   // a skill request sent the old way
            Ask(Kind::Email),  // asked, right kind
            Ask(Kind::Chat),   // asked, wrong kind
        ];
        let s = score(&decisions, &gold);
        assert_eq!(
            s,
            Score {
                n: 5,
                taken: 3,
                right: 1,
                skill_not_asked_for: 1,
                skill_sent_old_way: 1,
                asked: 2,
                asked_right_kind: 1
            }
        );
        assert_eq!(s.wrong(), 2);
    }

    #[test]
    fn the_pair_is_the_fewest_wrong_then_fewest_asked_then_lower_tau_then_higher_tau_g() {
        let sc = |wrong: usize, asked: usize| Score {
            skill_not_asked_for: wrong,
            asked,
            ..Score::default()
        };
        let grid = vec![
            ((0.5, 0.5), sc(2, 0)),
            ((0.6, 0.7), sc(1, 3)),
            ((0.7, 0.6), sc(1, 2)),
            ((0.8, 0.5), sc(1, 2)),
            ((0.7, 0.9), sc(1, 2)),
        ];
        // wrong 1, asked 2: (0.7, 0.6), (0.8, 0.5), (0.7, 0.9) -> lower τ 0.7 -> higher τ_g 0.9.
        assert_eq!(choose(&grid), (0.7, 0.9));
    }

    fn row(split: Split, gold: Kind, h: Vec<(Kind, f32)>, g: Option<(Kind, Vec<Step>)>) -> Row {
        Row {
            split,
            gold,
            head: h,
            gemma: g,
        }
    }

    /// Gemma picks `kind` (index 1 of two labels) with a logit gap, i.e. some confidence.
    fn picks(kind: Kind, gap: f32) -> Option<(Kind, Vec<Step>)> {
        Some((kind, vec![step(&[gap, 0.0], 0)]))
    }

    #[test]
    fn calibrating_fits_t_on_cal_and_picks_the_pair_on_val() {
        let mut rows = Vec::new();
        // cal: Gemma is right 3 in 4 at one gap.
        for i in 0..40 {
            let ok = i % 4 != 0;
            let said = if ok { Kind::Email } else { Kind::Chat };
            rows.push(row(
                Split::Cal,
                Kind::Email,
                head(Kind::Email, 0.5),
                picks(said, 4.0),
            ));
        }
        // val: the head is unsure and wrong about an email; Gemma is right (so a lower τ_g is
        // better than asking), and a sure app stays app.
        for _ in 0..10 {
            rows.push(row(
                Split::Val,
                Kind::Email,
                head(Kind::Chat, 0.4),
                picks(Kind::Email, 4.0),
            ));
            rows.push(row(
                Split::Val,
                Kind::App,
                head(Kind::App, 0.95),
                picks(Kind::App, 4.0),
            ));
        }
        let c = calibrate(&rows).unwrap();
        // 75% right at gap 4: T makes the confidence ~0.75 (> 0.7, < 0.8).
        assert!(c.temperature > 1.0);
        let conf = step_confidence(&[step(&[4.0, 0.0], 0)], f64::from(c.temperature));
        assert!((conf - 0.75).abs() < 0.02, "{conf}");
        assert_eq!(c.cal_n, 40);
        assert!((c.cal_gemma_accuracy - 0.75).abs() < 1e-9);
        // Gemma's confidence 0.75 clears τ_g up to 0.7, not 0.8: the pair with the fewest wrong
        // and asked decisions is τ 0.5 (lower τ first) with the highest τ_g that still takes it.
        assert_eq!((c.tau, c.tau_g), (0.5, 0.7));
        assert_eq!(c.val.wrong(), 0);
        assert_eq!(c.val.asked, 0);
        assert_eq!(c.val.n, 20);
    }

    #[test]
    fn a_gemma_that_failed_counts_as_app_with_no_confidence_on_val_and_is_left_out_of_cal() {
        let mut rows: Vec<Row> = (0..8)
            .map(|i| {
                let said = if i == 0 { Kind::Chat } else { Kind::Email };
                row(
                    Split::Cal,
                    Kind::Email,
                    head(Kind::Email, 0.5),
                    picks(said, 4.0),
                )
            })
            .collect();
        rows.push(row(Split::Cal, Kind::Email, head(Kind::Email, 0.5), None));
        rows.push(row(Split::Val, Kind::Email, head(Kind::Email, 0.95), None));
        let c = calibrate(&rows).unwrap();
        assert_eq!(c.cal_n, 8);
        assert_eq!(c.val.n, 1);
        // Sure head: taken whatever Gemma did.
        assert_eq!(c.val.right, 1);
        // An unsure head and a failed Gemma: `app`, sent the old way, never asked.
        rows.push(row(Split::Val, Kind::Email, head(Kind::Email, 0.4), None));
        let c = calibrate(&rows).unwrap();
        assert_eq!((c.val.n, c.val.skill_sent_old_way), (2, 0));
        assert!(calibrate(&rows[9..]).is_err(), "nothing on cal to fit");
    }

    /// The Python dev run's logged outputs for Kinds-v2 as rows (Gemma's steps are in the log).
    fn logged_rows() -> Vec<Row> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(
            root.join("experiments/logs/v3/kinds/fallback_outputs_v2.jsonl"),
        )
        .unwrap();
        let kind = |l: &str| Kind::from_label(l).unwrap();
        text.lines()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).unwrap();
                let steps = v["gemma"]["steps"].as_array().map(|a| {
                    a.iter()
                        .map(|s| Step {
                            allowed: serde_json::from_value(s["allowed"].clone()).unwrap(),
                            logits: serde_json::from_value(s["logits"].clone()).unwrap(),
                            chosen: s["chosen"].as_u64().unwrap() as usize,
                        })
                        .collect::<Vec<_>>()
                });
                Row {
                    split: if v["split"] == "cal" {
                        Split::Cal
                    } else {
                        Split::Val
                    },
                    gold: kind(v["gold"].as_str().unwrap()),
                    head: v["head"]
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(k, p)| (kind(k), p.as_f64().unwrap() as f32))
                        .collect(),
                    gemma: steps.map(|s| (kind(v["gemma"]["label"].as_str().unwrap()), s)),
                }
            })
            .collect()
    }

    #[test]
    fn the_v3_dev_run_s_logged_outputs_give_its_temperature_and_pair() {
        let c = calibrate(&logged_rows()).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dev: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("experiments/logs/v3/kinds/fallback_dev_v2.json"))
                .unwrap(),
        )
        .unwrap();
        let t = dev["temperature_gemma"].as_f64().unwrap();
        assert!(
            (f64::from(c.temperature) - t).abs() < 1e-3,
            "{} vs {t}",
            c.temperature
        );
        assert_eq!((c.tau, c.tau_g), (0.8, 0.7), "ADR-042's chosen pair");
        let want = &dev["val_grid"]["0.8/0.7"];
        assert_eq!(c.val.n as u64, want["n"].as_u64().unwrap());
        assert_eq!(c.val.right as u64, want["right"].as_u64().unwrap());
        assert_eq!(
            c.val.wrong() as u64,
            (want["skill_not_asked_for"].as_u64().unwrap()
                + want["skill_sent_old_way"].as_u64().unwrap())
        );
        assert_eq!(c.val.asked as u64, want["asked"].as_u64().unwrap());
    }

    #[test]
    fn the_dev_splits_are_kinds_v2_s_cal_and_val_and_read_as_items() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let [(Split::Cal, cal), (Split::Val, val)] = dev_splits(&root) else {
            panic!("cal then val");
        };
        assert!(cal.ends_with("kestrel_kinds_v2/splits/cal.jsonl"));
        assert!(val.ends_with("kestrel_kinds_v2/splits/val.jsonl"));
        let (cal, val) = (read_items(&cal).unwrap(), read_items(&val).unwrap());
        assert_eq!((cal.len(), val.len()), (123, 152));
        assert_eq!(cal[0].id, "app-09/en/0");
        assert_eq!(cal[0].gold, Kind::App);
        let dir = std::env::temp_dir().join(format!("kestrel_cal_items_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("bad.jsonl");
        std::fs::write(
            &bad,
            "{\"id\": \"a\", \"text\": \"x\", \"label\": \"nope\"}\n",
        )
        .unwrap();
        assert!(
            read_items(&bad)
                .unwrap_err()
                .to_string()
                .contains("bad.jsonl:1: not a kind")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_calibration_is_kept_with_its_val_counts_and_reported_in_words() {
        let c = calibrate(&logged_rows()).unwrap();
        let k = calibrated(&c, "gemma-4-E2B-it-Q4_0", (1, 2), (3, 4), 99);
        assert_eq!((k.tau, k.tau_g, k.val_n), (0.8, 0.7, 152));
        assert_eq!((k.val_wrong, k.val_asked), (c.val.wrong(), c.val.asked));
        assert_eq!((k.file, k.binary, k.measured_at), ((1, 2), (3, 4), 99));
        let r = report("gemma-4-E2B-it-Q4_0", &c);
        assert!(
            r.contains("tau 0.8, tau_g 0.7") && r.contains("152 requests"),
            "{r}"
        );
    }
}
