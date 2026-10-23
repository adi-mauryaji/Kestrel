//! Week 10 "Done" (Ch. 52): the Chapter 8 scenario end to end, without models.
//!
//! "Summarize my last three meeting notes and create a task list." goes through the real
//! normalizer, the memory context, the real Tier-1 rules and cascade (a stand-in answers for
//! Tier 4, which the whole compound request reaches), ADR-025's split with each part routed and
//! planned by its template, plan validation, the runner with the deterministic text ops, and the
//! SQLite memory backend. Then the Chapter 20 correction turn shapes the next request's context.
//! Tier 4 is a stand-in; the model-backed run of the same path is `kestrel plan-eval --hybrid`.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use kestrel_core::resources::{CancelToken, Deadline};
use kestrel_core::{Intent, KestrelError, Request, Tier, TierModel};
use kestrel_learning::correction::{self, Action};
use kestrel_memory::{Draft, Filter, Kind, Memory, Source};
use kestrel_nlu::rules::{RuleSet, RulesTier};
use kestrel_router::{Cascade, Outcome, RouterConfig};
use kestrel_tools::backend::MemoryBackend;
use kestrel_tools::compound::{plan_parts, split};
use kestrel_tools::date::Date;
use kestrel_tools::plan::{self, Data, NodeStatus};
use kestrel_tools::runner::{MarkedItems, Runner};
use kestrel_tools::sandbox::Sandbox;
use kestrel_tools::templates::{Context, Proposal};
use kestrel_tools::tools::{Confirm, Policy};

const NOW: i64 = 1_792_713_600; // 2026-10-23T00:00:00Z
const DAY: i64 = 86_400;
const REQUEST: &str = "Summarize my last three meeting notes and create a task list.";

/// Stands in for Tier 4: always the same answer.
struct Slm;

impl TierModel for Slm {
    fn tier(&self) -> Tier {
        Tier::Generative
    }
    fn est_cost_ms(&self) -> u32 {
        800
    }
    fn classify(&self, _: &Request) -> Result<Vec<(Intent, f32)>, KestrelError> {
        Ok(vec![(Intent::SummarizeNotes, 0.8)])
    }
}

fn cascade() -> Cascade<'static> {
    // 0.99 is a placeholder, NOT the rules' measured precision (Ch. 9.3).
    let rules = RulesTier::new(RuleSet::default_set(), 0.99, 1).unwrap();
    let cfg = RouterConfig {
        accept: [0.95, 0.90, 0.85, 0.60],
        latency_budget_ms: 2000,
        multi_intent_to_generative: false,
    };
    Cascade::new(vec![Box::new(Slm), Box::new(rules)], cfg).unwrap()
}

/// Says yes and remembers every question.
#[derive(Default)]
struct Asked(RefCell<Vec<String>>);

impl Confirm for Asked {
    fn ask(&self, q: &str) -> bool {
        self.0.borrow_mut().push(q.to_string());
        true
    }
}

/// Four meeting notes (only the three newest count) and a note that is not a meeting.
fn notes() -> PathBuf {
    let d = std::env::temp_dir().join(format!("kestrel_ch8_{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("meetings")).unwrap();
    let write = |name: &str, text: &str| fs::write(d.join(name), text).unwrap();
    write(
        "meetings/2026-10-15-kickoff.md",
        "tags: meeting\nKickoff.\n- [ ] not in the last three\n",
    );
    write(
        "meetings/2026-10-20-standup.md",
        "tags: meeting\nStandup: budget review moved.\n- [ ] email the budget to asha\n",
    );
    write(
        "meetings/2026-10-21-design.md",
        "tags: meeting\nDesign review.\n- [ ] book a room for the demo\nTODO: send the slides\n",
    );
    write(
        "meetings/2026-10-22-planning.md",
        "tags: meeting\nPlanning.\n- [ ] email the budget to asha\naction: draft the q4 plan\n",
    );
    write("2026-10-22-groceries.md", "milk, eggs\n- [ ] buy milk\n");
    d
}

fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

#[test]
fn the_chapter_8_scenario_end_to_end() {
    let notes = notes();
    let db = notes.with_extension("db");
    let _ = fs::remove_file(&db);
    let mut memory = Memory::open(&db).unwrap();
    let stated = Draft::new(Kind::Preference, "answer_length", "short");
    memory
        .propose(&stated, Source::UserExplicit, NOW - 2 * DAY)
        .unwrap();
    let sandbox = Sandbox::new(std::slice::from_ref(&notes)).unwrap();
    let mut backend = MemoryBackend::new(sandbox, memory);
    backend.begin_request(1, NOW);

    // 1-2. Input and normalization.
    let req = Request::new(1, REQUEST);
    assert_eq!(
        req.text,
        "summarize my last three meeting notes and create a task list"
    );

    // 3. Context: the approved preference, with its provenance.
    let ctx = backend
        .memory()
        .context(&req.text, NOW, 5, 200, &words)
        .unwrap();
    assert_eq!(
        ctx.text,
        "[pref|user_explicit|2026-10-21] answer_length: short"
    );

    // 4. Tier 1 has no opinion on the compound request; the whole of it would go to Tier 4.
    let (whole, trace) = cascade().route(&req);
    assert_eq!(trace.tiers_consulted, [Tier::Rules, Tier::Generative]);
    assert!(matches!(whole, Outcome::Accepted(ref p) if p.tier == Tier::Generative));

    // 5-6. ADR-025: split it, route each part (Tier 1 accepts both), template plans.
    let parts = split(&req.text);
    let routed: Vec<(Intent, String)> = parts
        .iter()
        .enumerate()
        .map(|(k, part)| {
            let (out, trace) = cascade().route(&Request::new(10 + k as u64, part));
            assert_eq!(trace.tiers_consulted, [Tier::Rules], "{part}");
            let Outcome::Accepted(p) = out else {
                panic!("{part}: {out:?}")
            };
            (p.intent, part.clone())
        })
        .collect();
    assert_eq!(
        routed.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
        [Intent::SummarizeNotes, Intent::CreateTask]
    );
    let documents: Vec<String> = Vec::new();
    let tctx = Context {
        today: Date::parse("2026-10-23").unwrap(),
        documents: &documents,
    };
    let Proposal::Plan(wire) = plan_parts(&routed, &tctx) else {
        panic!("a template plan")
    };

    // 7. Plan validation: schema, allowlisted ops, DAG, node cap.
    let plan = plan::parse(&wire.to_string()).unwrap();
    let ops: Vec<&str> = plan
        .order
        .iter()
        .map(|&i| plan.nodes[i].op.name())
        .collect();
    for op in [
        "fetch_recent_notes",
        "summarize",
        "extract_action_items",
        "create_task",
    ] {
        assert!(ops.contains(&op), "{op} in {ops:?}");
    }

    // 8-12. Retrieval through the sandbox, permission checks, execution, validation.
    let asked = Asked::default();
    let mut text = MarkedItems;
    let mut runner = Runner {
        store: &mut backend,
        text: &mut text,
        confirm: &asked,
        policy: Policy::default(),
        batch_confirm_above: 3,
        log: vec![],
    };
    let ex = plan::execute(
        &plan,
        &CancelToken::new(),
        &Deadline::after(Duration::from_secs(10)),
        |n, i, a| runner.run(n, i, a),
    );
    let log = serde_json::to_string(&runner.log).unwrap();
    assert!(
        ex.status.iter().all(|s| *s == NodeStatus::Done),
        "{:?} {:?}",
        ex.status,
        ex.errors
    );
    assert_eq!(
        asked.0.borrow().as_slice(),
        ["create 4 tasks?"],
        "a batch above the threshold asks once"
    );
    let summary = ex.outputs.iter().flatten().find_map(|d| match d {
        Data::Text(t) => Some(t.as_str()),
        _ => None,
    });
    assert_eq!(
        summary,
        Some("Planning. Design review. Standup: budget review moved.")
    );
    let tasks: Vec<String> = backend
        .memory()
        .tasks()
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(
        tasks,
        [
            "email the budget to asha",
            "draft the q4 plan",
            "book a room for the demo",
            "send the slides"
        ]
    );

    // 13. Memory decision: nothing durable but task history, which expires.
    let items = backend.memory().items(Filter::All).unwrap();
    let history: Vec<_> = items
        .iter()
        .filter(|i| i.kind == Kind::TaskHistory)
        .collect();
    assert_eq!(history.len(), 4);
    assert!(
        history
            .iter()
            .all(|h| h.approved && h.expires_at == Some(NOW + 90 * DAY))
    );
    assert_eq!(items.len(), 5, "the preference and the task history only");

    // 14. Measurement: the tool log is content-free.
    for secret in ["budget", "asha", "slides", "Planning", "meeting"] {
        assert!(!log.contains(secret), "{secret} in the log: {log}");
    }

    // Chapter 20: a correction, confirmed, changes the next request's context.
    let said = Request::new(2, "No, I prefer concise answers unless I ask for details.");
    let Action::StoreAfterConfirm(c) = correction::detect(&said.text) else {
        panic!("a durable correction")
    };
    assert_eq!(
        correction::question(&c),
        "Save: answer_length = short (unless i ask for details)?"
    );
    correction::save(backend.memory_mut(), &c, NOW + 60).unwrap();
    let next = backend
        .memory()
        .context("summarize my latest note", NOW + 120, 5, 200, &words)
        .unwrap();
    assert_eq!(
        next.text,
        "[pref|correction|2026-10-23] answer_length: short (exception: i ask for details)"
    );
    let ops: Vec<String> = backend
        .memory()
        .audit()
        .unwrap()
        .into_iter()
        .map(|e| e.op)
        .collect();
    assert_eq!(
        ops,
        ["store", "store", "store", "store", "store", "confirm"]
    );
    let _ = fs::remove_dir_all(&notes);
    let _ = fs::remove_file(&db);
}
