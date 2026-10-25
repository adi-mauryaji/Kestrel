//! `kestrel ask`: route typed requests one at a time and show what every tier did. Each tier is
//! wrapped in a `Recorder`, so the cascade itself runs unchanged; the recorder notes the tier's
//! top answer and wall time, and `render` lays them out next to the tier's accept threshold.
//!
//!     kestrel ask [--no-memory] CONFIG.json          one request per line ("quit" or EOF ends)
//!     kestrel ask [--no-memory] CONFIG.json TEXT...  route TEXT once
//!
//! Memory (src/personal.rs, Week 10, ADR-028): before routing, a taught alias or a correction is
//! handled (a durable one asks "Save: ...? [y/N]"); other requests show their memory context and
//! are routed with the user's aliases resolved; after routing, an accepted set_preference is stored. The file is the one `kestrel memory` uses; if it cannot be opened,
//! one notice and no personalization (Ch. 8.3). One-shot mode cannot ask, so it saves nothing.
//!
//! With KESTREL_TELEMETRY=<file>, each request also appends a content-free record there
//! (src/telemetry.rs: tier path and times, decision, RSS; never the text or a label).
//!
//! With `--notes DIR` (or KESTREL_NOTES) the request is also carried out (src/act.rs): a request
//! that joins several (the routing signal) is split and each part routed (ADR-025), then planned by
//! the templates and run on the notes folder, tasks and preferences going into the memory file; a
//! delete or a batch of tasks asks y/N first. `--brief` prints only what was found or done.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;
use std::time::Instant;

use kestrel_core::{Intent, KestrelError, Request, Tier, TierModel};
use kestrel_router::{AbstainReason, Outcome, Trace};

/// What one tier answered for one request: its top label (or no opinion), or the error.
#[derive(Debug, Clone, PartialEq)]
pub struct Step<L = Intent> {
    pub tier: Tier,
    pub result: Result<Option<(L, f32)>, String>,
    pub ms: f64,
}

/// Shared by every recorder of one cascade; cleared before each request.
pub type Log<L = Intent> = Rc<RefCell<Vec<Step<L>>>>;

/// A tier that notes each answer in `log` and passes it on untouched. Any label type, so
/// `bench-items` records per-tier times in a benchmark's label space too.
pub struct Recorder<'a, L = Intent> {
    inner: Box<dyn TierModel<L> + 'a>,
    log: Log<L>,
}

impl<'a, L> Recorder<'a, L> {
    pub fn new(inner: Box<dyn TierModel<L> + 'a>, log: Log<L>) -> Self {
        Self { inner, log }
    }
}

impl<L: Copy> TierModel<L> for Recorder<'_, L> {
    fn tier(&self) -> Tier {
        self.inner.tier()
    }

    fn est_cost_ms(&self) -> u32 {
        self.inner.est_cost_ms()
    }

    fn classify(&self, req: &Request) -> Result<Vec<(L, f32)>, KestrelError> {
        let t0 = Instant::now();
        let out = self.inner.classify(req);
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        let result = match &out {
            // The router takes the first of equal probabilities; so does this.
            Ok(probs) => Ok(probs
                .iter()
                .fold(None, |top: Option<(L, f32)>, &(i, p)| match top {
                    Some((_, q)) if q >= p => top,
                    _ => Some((i, p)),
                })),
            Err(e) => Err(e.to_string()),
        };
        self.log.borrow_mut().push(Step {
            tier: self.tier(),
            result,
            ms,
        });
        out
    }
}

/// Loads the models once, then routes `once` or every line typed until "quit" or EOF.
#[cfg(feature = "llama")]
pub fn run(
    config_path: &std::path::Path,
    once: Option<&str>,
    memory: Option<&std::path::Path>,
    notes: Option<&std::path::Path>,
    brief: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{BufRead, Write};

    // The demo shows Kestrel intents, so the config must answer in them (not CLINC150's labels);
    // checked before the models load.
    let (_, cfg) = crate::config::load(&std::fs::read_to_string(config_path)?)?;
    let list = std::path::Path::new("datasets/contracts").join(&cfg.models.tier4.labels);
    let intents =
        crate::labels::Intents::check(&crate::labels::parse_list(&std::fs::read_to_string(list)?))?;
    // The notes folder is checked before the models load, so a wrong path fails at once.
    let act = match (notes, memory) {
        (None, _) => None,
        (Some(_), None) => {
            return Err(
                "--notes needs the memory file (tasks live there): drop --no-memory".into(),
            );
        }
        (Some(dir), Some(_)) if !dir.is_dir() => {
            return Err(format!(
                "no notes folder at {}: create it (mkdir -p {}) and put your notes in it (.md or .txt), \
                 or name a folder that exists",
                dir.display(),
                dir.display()
            )
            .into());
        }
        (Some(dir), Some(mem)) => {
            let a = crate::act::Act::open(dir, mem)?;
            match a.documents().len() {
                0 => eprintln!(
                    "{} has no notes yet: searches will find nothing",
                    dir.display()
                ),
                k => eprintln!("acting on {} ({k} files)", dir.display()),
            }
            eprintln!("tasks and preferences in {}", mem.display());
            Some(a)
        }
    };
    eprintln!("loading models (encoder, SLM) and priming the Tier-4 prompt...");
    let t0 = Instant::now();
    let models = crate::route_items::Models::load(config_path)?;
    let log = Log::default();
    let cascade = models.cascade(&intents, |t| Box::new(Recorder::new(t, log.clone())))?;
    let accept = models.router.accept;
    eprintln!(
        "ready in {:.1} s. tiers {:?}, accept thresholds T1 {:.3} T3 {:.3} T4 {:.3}, budget {} ms",
        t0.elapsed().as_secs_f64(),
        models.cfg.tiers,
        accept[0],
        accept[2],
        accept[3],
        models.router.latency_budget_ms
    );
    let telemetry_path = std::env::var_os("KESTREL_TELEMETRY");
    let sink = crate::telemetry::Sink::open(telemetry_path.as_deref().map(std::path::Path::new));
    if sink.is_on() {
        eprintln!(
            "content-free telemetry appended to {:?}",
            telemetry_path.unwrap_or_default()
        );
    }

    let personal = crate::personal::Personal::open(memory);
    let mut assistant = crate::assistant::Assistant::new(cascade, log, personal, act, sink, brief);
    let mut ask = |text: &str, confirm: &mut dyn FnMut(&str) -> bool| {
        for l in assistant.handle(text, confirm) {
            println!("{l}");
        }
    };
    if let Some(text) = once {
        ask(text, &mut |q| {
            println!("{q} (one-shot mode cannot ask)");
            false
        });
        return Ok(());
    }
    eprintln!("type a request (English or Hinglish); \"quit\" or Ctrl-D ends");
    // Piped requests are not echoed by a terminal, so print them after the prompt.
    let echo = !std::io::IsTerminal::is_terminal(&std::io::stdin());
    let mut stdin = std::io::stdin().lock();
    loop {
        print!("> ");
        std::io::stdout().flush()?;
        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 || matches!(line.trim(), "quit" | "exit") {
            return Ok(());
        }
        if line.trim().is_empty() {
            continue;
        }
        if echo {
            println!("{}", line.trim());
        }
        let mut confirm = |q: &str| {
            print!("{q} [y/N] ");
            let _ = std::io::stdout().flush();
            let mut answer = String::new();
            if stdin.read_line(&mut answer).unwrap_or(0) == 0 {
                return false;
            }
            if echo {
                println!("{}", answer.trim());
            }
            matches!(
                answer.trim().to_lowercase().as_str(),
                "y" | "yes" | "haan" | "ha"
            )
        };
        ask(line.trim(), &mut confirm);
        println!();
    }
}

fn name(t: Tier) -> &'static str {
    match t {
        Tier::Rules => "rules",
        Tier::Micro => "micro",
        Tier::Encoder => "encoder",
        Tier::Generative => "SLM",
    }
}

fn reason(r: AbstainReason) -> &'static str {
    match r {
        AbstainReason::LowConfidence => "no tier was confident enough",
        AbstainReason::BudgetExhausted => "latency budget used up",
        AbstainReason::NoTierAvailable => "no tier had an answer",
        AbstainReason::Cancelled => "cancelled",
    }
}

/// The report for one routed request: the normalized text if it differs, the multi-intent skip,
/// one line per tier consulted (answer, confidence vs. accept threshold, verdict, time), and the
/// decision with the total wall time.
pub fn render(
    req: &Request,
    steps: &[Step],
    outcome: &Outcome,
    trace: &Trace,
    accept: [f32; 4],
    ms: f64,
) -> String {
    let mut s = String::new();
    if req.text != req.raw {
        let _ = writeln!(s, "  normalized: {}", req.text);
    }
    if trace.multi_intent {
        let _ = writeln!(
            s,
            "  multi-intent signal: may ask for several things, so tiers below 4 are skipped"
        );
    }
    let accepted_at = match outcome {
        Outcome::Accepted(p) => Some(p.tier),
        Outcome::Abstained { .. } => None,
    };
    for st in steps {
        let tier = format!("T{} {}", st.tier as u8, name(st.tier));
        let answer = match &st.result {
            Ok(Some((intent, p))) => {
                let tau = accept[st.tier.index()];
                let (cmp, verdict) = if accepted_at == Some(st.tier) {
                    (">=", "accept")
                } else {
                    ("< ", "escalate")
                };
                format!("{:<16} {p:.3} {cmp} {tau:.3}  {verdict}", intent.label())
            }
            Ok(None) => format!("{:<31}escalate", "no opinion"),
            Err(e) => format!("{:<31}escalate", format!("error: {e}")),
        };
        let _ = writeln!(s, "  {tier:<11} {answer:<42} {:>8.1} ms", st.ms);
    }
    let _ = match outcome {
        Outcome::Accepted(p) => writeln!(
            s,
            "  => {}  (Tier {}, confidence {:.3}, {ms:.1} ms)",
            p.intent.label(),
            p.tier as u8,
            p.confidence
        ),
        Outcome::Abstained { best, reason: why } => {
            let best = best.as_ref().map_or(String::new(), |b| {
                format!(
                    "; best guess {} {:.3} from Tier {}",
                    b.intent.label(),
                    b.confidence,
                    b.tier as u8
                )
            });
            writeln!(s, "  => abstain: {}{best}  ({ms:.1} ms)", reason(*why))
        }
    };
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use kestrel_core::Prediction;
    use kestrel_router::{Cascade, RouterConfig};

    /// A tier with a fixed reply.
    struct Fixed(Tier, Result<Vec<(Intent, f32)>, ()>);

    impl TierModel for Fixed {
        fn tier(&self) -> Tier {
            self.0
        }
        fn est_cost_ms(&self) -> u32 {
            1
        }
        fn classify(&self, _: &Request) -> Result<Vec<(Intent, f32)>, KestrelError> {
            self.1
                .clone()
                .map_err(|()| KestrelError::Invalid("boom".into()))
        }
    }

    const ACCEPT: [f32; 4] = [1.0, 1.0, 0.9, 0.0];

    fn cascade<'a>(tiers: Vec<Fixed>, log: &Log) -> Cascade<'a> {
        let tiers = tiers
            .into_iter()
            .map(|t| Box::new(Recorder::new(Box::new(t), log.clone())) as Box<dyn TierModel>)
            .collect();
        let cfg = RouterConfig {
            accept: ACCEPT,
            latency_budget_ms: 100,
            multi_intent_to_generative: true,
        };
        Cascade::new(tiers, cfg).unwrap()
    }

    #[test]
    fn recorders_note_each_consulted_tier_and_leave_routing_unchanged() {
        let log = Log::default();
        let c = cascade(
            vec![
                Fixed(Tier::Rules, Ok(vec![])),
                Fixed(
                    Tier::Encoder,
                    Ok(vec![(Intent::CreateTask, 0.6), (Intent::DeleteTask, 0.3)]),
                ),
                Fixed(Tier::Generative, Ok(vec![(Intent::CreateTask, 0.95)])),
            ],
            &log,
        );
        let (out, _) = c.route(&Request::new(1, "add milk to my list"));
        assert!(matches!(out, Outcome::Accepted(ref p) if p.tier == Tier::Generative));
        let steps = log.borrow();
        let got: Vec<_> = steps.iter().map(|s| (s.tier, s.result.clone())).collect();
        assert_eq!(
            got,
            vec![
                (Tier::Rules, Ok(None)),
                (Tier::Encoder, Ok(Some((Intent::CreateTask, 0.6)))),
                (Tier::Generative, Ok(Some((Intent::CreateTask, 0.95)))),
            ]
        );
    }

    #[test]
    fn recorders_keep_the_first_of_equal_probabilities_and_note_errors() {
        let log = Log::default();
        let c = cascade(
            vec![
                Fixed(Tier::Rules, Err(())),
                Fixed(
                    Tier::Encoder,
                    Ok(vec![(Intent::SearchNotes, 0.5), (Intent::Chitchat, 0.5)]),
                ),
            ],
            &log,
        );
        let (out, _) = c.route(&Request::new(1, "hmm"));
        assert!(matches!(out, Outcome::Abstained { .. }));
        let steps = log.borrow();
        assert_eq!(steps[0].result, Err("invalid input: boom".into()));
        assert_eq!(steps[1].result, Ok(Some((Intent::SearchNotes, 0.5))));
    }

    fn step(tier: Tier, result: Result<Option<(Intent, f32)>, String>) -> Step {
        Step {
            tier,
            result,
            ms: 2.0,
        }
    }

    #[test]
    fn render_shows_each_tier_against_its_threshold_and_the_decision() {
        let steps = [
            step(Tier::Rules, Ok(None)),
            step(Tier::Encoder, Ok(Some((Intent::CreateTask, 0.6)))),
            step(Tier::Generative, Ok(Some((Intent::CreateTask, 0.95)))),
        ];
        let p = Prediction {
            intent: Intent::CreateTask,
            confidence: 0.95,
            tier: Tier::Generative,
        };
        let r = render(
            &Request::new(1, "Add milk  to my list"),
            &steps,
            &Outcome::Accepted(p),
            &Trace::default(),
            ACCEPT,
            7.0,
        );
        let lines: Vec<_> = r.lines().collect();
        assert_eq!(lines[0], "  normalized: add milk to my list");
        assert!(lines[1].contains("T1 rules") && lines[1].contains("no opinion"));
        assert!(lines[2].contains("create_task      0.600 <  0.900  escalate"));
        assert!(lines[3].contains("T4 SLM") && lines[3].contains("0.950 >= 0.000  accept"));
        assert_eq!(
            lines[4],
            "  => create_task  (Tier 4, confidence 0.950, 7.0 ms)"
        );
    }

    #[test]
    fn render_explains_skips_errors_and_abstentions() {
        let steps = [step(
            Tier::Generative,
            Err("tier Generative timed out".into()),
        )];
        let trace = Trace {
            multi_intent: true,
            ..Trace::default()
        };
        let best = Prediction {
            intent: Intent::DeleteTask,
            confidence: 0.4,
            tier: Tier::Encoder,
        };
        let out = Outcome::Abstained {
            best: Some(best),
            reason: AbstainReason::LowConfidence,
        };
        let r = render(
            &Request::new(1, "a and b"),
            &steps,
            &out,
            &trace,
            ACCEPT,
            3.0,
        );
        assert!(r.contains("multi-intent signal"));
        assert!(r.contains("error: tier Generative timed out"));
        assert!(r.ends_with(
            "=> abstain: no tier was confident enough; best guess delete_task 0.400 from Tier 3  (3.0 ms)\n"
        ));
        assert!(!r.contains("normalized"));
    }
}
