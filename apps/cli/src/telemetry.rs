//! Local, content-free telemetry (FR-10, Ch. 15): per routed request, the tier path with each
//! tier's time, where routing ended and why, the total time and the peak RSS. Never the request
//! text and never a label, so the log cannot tell what was asked (Ch. 42 canary audit). Writing
//! is best effort: a file that cannot be opened or written is reported once and never blocks a
//! request. Opt in with KESTREL_TELEMETRY=<file> (`kestrel ask`); nothing leaves the machine.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use kestrel_bench::rss::kib_to_mib;
use kestrel_router::{Outcome, Trace};
use serde_json::{Value, json};

use crate::ask::Step;
use crate::config;

/// One request's record; `unix_s` and `peak_rss_kib` are passed in so tests are deterministic.
pub fn record<L>(
    steps: &[Step<L>],
    outcome: &Outcome<L>,
    trace: &Trace,
    ms: f64,
    unix_s: u64,
    peak_rss_kib: Option<u64>,
) -> Value {
    let path: Vec<Value> = steps
        .iter()
        .map(|s| {
            json!({"tier": s.tier as u8, "ms": s.ms, "answered": matches!(s.result, Ok(Some(_))),
                   "failed": s.result.is_err()})
        })
        .collect();
    let last = trace.tiers_consulted.last().map_or(0, |&t| t as u8);
    let (end_tier, accepted, confidence, reason) = match outcome {
        Outcome::Accepted(p) => (p.tier as u8, true, Some(p.confidence), None),
        Outcome::Abstained { reason: why, .. } => (last, false, None, Some(config::reason(*why))),
    };
    json!({
        "t": unix_s, "path": path, "end_tier": end_tier, "accepted": accepted,
        "confidence": confidence, "reason": reason, "multi_intent": trace.multi_intent, "ms": ms,
        "peak_rss_mib": peak_rss_kib.map(kib_to_mib),
    })
}

/// An append-only JSONL file, or nothing.
pub struct Sink {
    out: Option<BufWriter<File>>,
}

impl Sink {
    pub fn open(path: Option<&Path>) -> Self {
        let out = path.and_then(|p| {
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
            {
                Ok(f) => Some(BufWriter::new(f)),
                Err(e) => {
                    eprintln!("telemetry off: cannot open {}: {e}", p.display());
                    None
                }
            }
        });
        Self { out }
    }

    pub fn is_on(&self) -> bool {
        self.out.is_some()
    }

    /// Appends `rec` and flushes; on failure warns once and turns itself off.
    pub fn write(&mut self, rec: &Value) {
        if let Some(w) = &mut self.out
            && let Err(e) = writeln!(w, "{rec}").and_then(|()| w.flush())
        {
            eprintln!("telemetry off: write failed: {e}");
            self.out = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ask::{Log, Recorder};
    use kestrel_core::{Intent, KestrelError, Request, Tier, TierModel};
    use kestrel_router::{Cascade, RouterConfig};

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

    fn routed(text: &str) -> Value {
        let log = Log::default();
        let tiers: Vec<Box<dyn TierModel>> = vec![
            Box::new(Recorder::new(
                Box::new(Fixed(Tier::Rules, Ok(vec![]))),
                log.clone(),
            )),
            Box::new(Recorder::new(
                Box::new(Fixed(Tier::Encoder, Err(()))),
                log.clone(),
            )),
            Box::new(Recorder::new(
                Box::new(Fixed(Tier::Generative, Ok(vec![(Intent::CreateTask, 0.9)]))),
                log.clone(),
            )),
        ];
        let cfg = RouterConfig {
            accept: [1.0, 1.0, 0.9, 0.5],
            latency_budget_ms: 100,
            multi_intent_to_generative: false,
        };
        let (out, trace) = Cascade::new(tiers, cfg)
            .unwrap()
            .route(&Request::new(1, text));
        record(&log.borrow(), &out, &trace, 12.5, 1_700_000_000, Some(2048))
    }

    #[test]
    fn a_record_has_the_path_and_decision_but_no_content() {
        let r = routed("CANARY-7f3a remind me to call asha");
        let s = r.to_string();
        assert!(!s.contains("CANARY") && !s.contains("asha"), "{s}");
        assert!(!s.contains("create_task"), "labels are content too: {s}");
        assert_eq!(
            r["path"],
            json!([{"tier": 1, "ms": r["path"][0]["ms"], "answered": false, "failed": false},
                   {"tier": 3, "ms": r["path"][1]["ms"], "answered": false, "failed": true},
                   {"tier": 4, "ms": r["path"][2]["ms"], "answered": true, "failed": false}])
        );
        assert_eq!(
            (r["end_tier"].as_u64(), r["accepted"].as_bool()),
            (Some(4), Some(true))
        );
        assert!(r["reason"].is_null());
        assert_eq!(
            (r["t"].as_u64(), r["ms"].as_f64()),
            (Some(1_700_000_000), Some(12.5))
        );
        assert_eq!(r["peak_rss_mib"].as_f64(), Some(2.0));
    }

    #[test]
    fn a_sink_appends_one_line_per_record() {
        let path =
            std::env::temp_dir().join(format!("kestrel-telemetry-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let mut sink = Sink::open(Some(&path));
            assert!(sink.is_on());
            sink.write(&json!({"a": 1}));
            sink.write(&json!({"a": 2}));
        }
        {
            let mut again = Sink::open(Some(&path)); // appends, does not truncate
            again.write(&json!({"a": 3}));
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn no_path_or_an_unwritable_one_is_off_and_never_fails_a_request() {
        let mut off = Sink::open(None);
        assert!(!off.is_on());
        off.write(&json!({"a": 1}));
        let mut bad = Sink::open(Some(Path::new("/nonexistent-kestrel-dir/t.jsonl")));
        assert!(!bad.is_on());
        bad.write(&json!({"a": 1}));
    }
}
