use super::*;

const CATALOG: &str = r#"{"models": [
  {"id": "e5", "file": "e5.gguf", "file_mib": 126, "memory_mib": 200, "role": "encoder", "source": "test"},
  {"id": "gemma-e2b", "file": "e2b.gguf", "file_mib": 2710, "memory_mib": 3900, "role": "writer", "source": "test"},
  {"id": "gemma-12b", "file": "12b.gguf", "file_mib": 7000, "memory_mib": 8500, "role": "writer",
   "estimate": true, "source": "test"}
]}"#;

const CARDS: &[&str] = &[
    r#"{"id": "tasks", "name": {"en": "Tasks", "hi": "Tasks"}, "needs": {"models": ["e5"]}}"#,
    r#"{"id": "reminders.desktop", "name": {"en": "Desktop reminders", "hi": "Desktop reminders"},
        "needs": {"os": ["linux", "macos"],
                  "os_reason": {"en": "Windows gets them with the app", "hi": "Windows pe app ke saath"}}}"#,
    r#"{"id": "notes.search", "name": {"en": "Notes search", "hi": "Notes search"},
        "needs": {"models": ["e5"]}}"#,
    r#"{"id": "chat", "name": {"en": "Chat", "hi": "Baatcheet"}, "needs": {"writer": true},
        "gates": [{"measure": "decode_tok_s", "min": 7.0}, {"measure": "prefill_tok_s", "min": 40.0}]}"#,
    r#"{"id": "notes.qa", "name": {"en": "Questions on notes", "hi": "Notes pe sawaal"},
        "needs": {"cards": ["notes.search"], "writer": true},
        "gates": [{"measure": "decode_tok_s", "min": 7.0}]}"#,
    r#"{"id": "code.check", "name": {"en": "Code check", "hi": "Code check"},
        "needs": {"os": ["linux"], "os_reason": {"en": "Linux only until phase A4", "hi": "A4 tak sirf Linux"},
                  "tools": ["bwrap", "python3"], "writer": true},
        "gates": [{"measure": "decode_tok_s", "min": 7.0}]}"#,
    r#"{"id": "email.send", "name": {"en": "Send mail", "hi": "Mail bhejna"},
        "needs": {"cards": ["chat"], "internet": true,
                  "setup": [{"id": "gmail.app_password",
                             "step": {"en": "add your Gmail app password", "hi": "Gmail app password daalo"}}]}}"#,
];

fn catalog() -> Catalog {
    serde_json::from_str(CATALOG).unwrap()
}

fn cards() -> Vec<Card> {
    parse_cards(CARDS).unwrap()
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn speed(decode: f64, prefill: f64) -> Speed {
    Speed {
        decode_tok_s: decode,
        prefill_tok_s: prefill,
    }
}

/// This laptop: Ryzen 5 5500U, 15 308 MiB, no GPU; Gemma E2B measured (E0: 12.8 / 58.5 tok/s).
fn laptop() -> Device {
    Device {
        os: Os::Linux,
        ram_total_mib: 15_308,
        gpus: vec![],
        tools: strings(&["bwrap", "python3"]),
        models: strings(&["e5", "gemma-e2b"]),
        writer: Some("gemma-e2b".into()),
        internet: false,
        setup_done: vec![],
    }
}

fn laptop_speed(decode: f64) -> Measured {
    Measured::from([("gemma-e2b".to_string(), speed(decode, 58.5))])
}

/// A typical 8 GB Windows laptop, freshly installed: the encoder ships with the app, no writer yet.
fn windows_8gb() -> Device {
    Device {
        os: Os::Windows,
        ram_total_mib: 7_987,
        gpus: vec![],
        tools: vec![],
        models: strings(&["e5"]),
        writer: None,
        internet: false,
        setup_done: vec![],
    }
}

/// The human's Mac: M4, 16 GB unified, Metal may use about 11 GB; Gemma here but not measured.
fn m4() -> Device {
    Device {
        os: Os::MacOs,
        ram_total_mib: 16_384,
        gpus: vec![Gpu {
            name: "Apple M4".into(),
            memory_mib: 10_923,
            kind: GpuKind::AppleUnified,
        }],
        tools: strings(&["python3"]),
        models: strings(&["e5", "gemma-e2b"]),
        writer: Some("gemma-e2b".into()),
        internet: false,
        setup_done: vec![],
    }
}

/// 32 GB and an 8 GB graphics card, running the 12B.
fn big() -> Device {
    Device {
        os: Os::Linux,
        ram_total_mib: 32_768,
        gpus: vec![Gpu {
            name: "RTX 4060".into(),
            memory_mib: 8_192,
            kind: GpuKind::Discrete,
        }],
        tools: strings(&["python3"]),
        models: strings(&["e5", "gemma-12b"]),
        writer: Some("gemma-12b".into()),
        internet: true,
        setup_done: strings(&["gmail.app_password"]),
    }
}

fn tiny() -> Device {
    Device {
        os: Os::Linux,
        ram_total_mib: 4_096,
        ..windows_8gb()
    }
}

/// The verdicts by card id.
fn run(d: &Device, m: &Measured) -> BTreeMap<String, State> {
    evaluate(&cards(), &catalog(), d, m)
        .into_iter()
        .map(|v| (v.id, v.state))
        .collect()
}

fn ready(slow: bool) -> State {
    State::Ready { slow }
}

/// A state's steps or reasons in English (the Hinglish is checked to be there).
fn en(s: &State) -> Vec<String> {
    let texts = match s {
        State::Ready { .. } => return vec![],
        State::Setup { steps } => steps,
        State::Locked { reasons } => reasons,
    };
    assert!(texts.iter().all(|t| !t.hi.is_empty()), "{texts:?}");
    texts.iter().map(|t| t.en.clone()).collect()
}

fn is_setup(s: &State) -> bool {
    matches!(s, State::Setup { .. })
}

fn is_locked(s: &State) -> bool {
    matches!(s, State::Locked { .. })
}

#[test]
fn the_budget_is_ram_less_the_reserve_or_a_gpu_whichever_is_larger() {
    assert_eq!(memory_budget_mib(&laptop()), 15_308 - 3072);
    assert_eq!(memory_budget_mib(&windows_8gb()), 7_987 - 3072);
    assert_eq!(memory_budget_mib(&m4()), 16_384 - 3072); // RAM > Metal's share
    assert_eq!(memory_budget_mib(&big()), 32_768 - 3072);
    let gpu_heavy = Device {
        ram_total_mib: 8_192,
        gpus: vec![Gpu {
            name: "RTX 4090".into(),
            memory_mib: 24_576,
            kind: GpuKind::Discrete,
        }],
        ..big()
    };
    assert_eq!(memory_budget_mib(&gpu_heavy), 24_576 - 512); // a card keeps some for itself
    assert_eq!(memory_budget_mib(&tiny()), 1_024);
    // A GPU inside the processor shares the RAM already counted: it adds nothing.
    let integrated = Device {
        gpus: vec![Gpu {
            name: "AMD Radeon (Ryzen 5 5500U)".into(),
            memory_mib: 7_000,
            kind: GpuKind::Integrated,
        }],
        ..laptop()
    };
    assert_eq!(memory_budget_mib(&integrated), 15_308 - 3072);
}

#[test]
fn on_this_laptop_everything_built_is_ready_and_mail_waits_for_its_setup() {
    let v = run(&laptop(), &laptop_speed(12.8));
    for id in [
        "tasks",
        "reminders.desktop",
        "notes.search",
        "chat",
        "notes.qa",
        "code.check",
    ] {
        assert_eq!(v[id], ready(false), "{id}");
    }
    assert_eq!(
        en(&v["email.send"]),
        [
            "switch internet features on (Settings)",
            "add your Gmail app password"
        ]
    );
}

#[test]
fn a_fresh_8gb_windows_laptop_is_asked_to_choose_a_model() {
    let v = run(&windows_8gb(), &Measured::new());
    assert_eq!(v["tasks"], ready(false));
    assert_eq!(v["notes.search"], ready(false));
    assert_eq!(en(&v["chat"]), ["choose a model: gemma-e2b (2.6 GB)"]);
    assert_eq!(en(&v["notes.qa"]), ["choose a model: gemma-e2b (2.6 GB)"]);
    assert_eq!(
        en(&v["reminders.desktop"]),
        ["Windows gets them with the app"]
    );
    assert!(is_locked(&v["reminders.desktop"]));
    // Locked by the system: the missing programs and model are not listed as steps.
    assert!(is_locked(&v["code.check"]));
    assert_eq!(en(&v["code.check"]), ["Linux only until phase A4"]);
    // A card that needs another one in setup carries that card's steps.
    assert_eq!(
        en(&v["email.send"]),
        [
            "choose a model: gemma-e2b (2.6 GB)",
            "switch internet features on (Settings)",
            "add your Gmail app password"
        ]
    );
}

#[test]
fn on_the_m4_writing_waits_for_the_speed_test_and_code_check_is_linux_only() {
    let v = run(&m4(), &Measured::new());
    assert_eq!(v["reminders.desktop"], ready(false));
    assert_eq!(en(&v["chat"]), ["run the speed test (1–2 min)"]);
    assert!(is_setup(&v["chat"]));
    assert!(is_locked(&v["code.check"]));
}

#[test]
fn a_big_machine_runs_the_12b_and_only_lacks_bubblewrap() {
    let m = Measured::from([("gemma-12b".to_string(), speed(30.0, 400.0))]);
    let v = run(&big(), &m);
    assert_eq!(v["chat"], ready(false));
    assert_eq!(v["email.send"], ready(false));
    assert_eq!(en(&v["code.check"]), ["install bwrap"]);
}

#[test]
fn a_device_too_small_for_any_writer_is_locked_with_the_numbers() {
    let v = run(&tiny(), &Measured::new());
    assert_eq!(v["tasks"], ready(false));
    let why = ["needs about 3.8 GB of memory for the smallest model; this device can spare 1.0 GB"];
    assert_eq!(en(&v["chat"]), why);
    assert!(is_locked(&v["chat"]));
    assert_eq!(en(&v["notes.qa"]), why);
    // A card that needs a locked card is locked too, by name.
    assert_eq!(
        en(&v["email.send"]),
        ["needs “Chat”, which is not on this device"]
    );
}

#[test]
fn speed_decides_ready_slow_and_locked() {
    assert_eq!(run(&laptop(), &laptop_speed(8.0))["chat"], ready(true)); // < 7 × 1.2
    assert_eq!(run(&laptop(), &laptop_speed(8.4))["chat"], ready(false));
    let v = run(&laptop(), &laptop_speed(5.0));
    assert_eq!(
        en(&v["chat"]),
        ["gemma-e2b writes 5.0 tokens/s here; this needs 7"]
    );
    assert!(is_locked(&v["chat"]));
    let slow_reading = Measured::from([("gemma-e2b".to_string(), speed(12.8, 30.0))]);
    assert_eq!(
        en(&run(&laptop(), &slow_reading)["chat"]),
        ["gemma-e2b reads 30 tokens/s here; this needs 40"]
    );
}

#[test]
fn the_chosen_model_must_fit_be_here_and_be_known() {
    let too_big = Device {
        writer: Some("gemma-12b".into()),
        ..windows_8gb()
    };
    assert_eq!(
        en(&run(&too_big, &Measured::new())["chat"]),
        [
            "gemma-12b needs about 8.3 GB of memory; this device can spare 4.8 GB: choose a smaller model"
        ]
    );
    let not_here = Device {
        writer: Some("gemma-e2b".into()),
        ..windows_8gb()
    };
    assert_eq!(
        en(&run(&not_here, &Measured::new())["chat"]),
        ["download gemma-e2b (2.6 GB)"]
    );
    let unknown = Device {
        writer: Some("llama-9000".into()),
        ..laptop()
    };
    assert_eq!(
        en(&run(&unknown, &laptop_speed(12.8))["chat"]),
        ["unknown model llama-9000"]
    );
}

#[test]
fn a_missing_encoder_is_a_step_and_travels_to_the_cards_that_need_it() {
    let d = Device {
        models: strings(&["gemma-e2b"]),
        ..laptop()
    };
    let v = run(&d, &laptop_speed(12.8));
    assert_eq!(en(&v["tasks"]), ["get the e5 model (126 MB)"]);
    assert_eq!(en(&v["notes.qa"]), ["get the e5 model (126 MB)"]);
    assert_eq!(v["chat"], ready(false));
}

#[test]
fn a_system_without_its_own_reason_gets_a_plain_one() {
    let c = parse_cards(&[
        r#"{"id": "x", "name": {"en": "X", "hi": "X"}, "needs": {"os": ["macos"]}}"#,
    ])
    .unwrap();
    let v = evaluate(&c, &catalog(), &laptop(), &Measured::new());
    assert_eq!(en(&v[0].state), ["not on linux yet"]);
}

#[test]
fn verdicts_come_in_the_cards_order() {
    let ids: Vec<String> = evaluate(&cards(), &catalog(), &laptop(), &laptop_speed(12.8))
        .into_iter()
        .map(|v| v.id)
        .collect();
    let want: Vec<String> = cards().into_iter().map(|c| c.id).collect();
    assert_eq!(ids, want);
}

#[test]
fn bad_card_files_are_refused() {
    let card = |id: &str, extra: &str| {
        format!(r#"{{"id": "{id}", "name": {{"en": "{id}", "hi": "{id}"}}{extra}}}"#)
    };
    let a = card("a", "");
    let dup = parse_cards(&[&a, &a]).unwrap_err();
    assert!(dup.contains("a"), "{dup}");
    let unknown = card("b", r#", "needs": {"cards": ["nope"]}"#);
    assert!(parse_cards(&[&unknown]).unwrap_err().contains("nope"));
    let c1 = card("c1", r#", "needs": {"cards": ["c2"]}"#);
    let c2 = card("c2", r#", "needs": {"cards": ["c1"]}"#);
    assert!(parse_cards(&[&c1, &c2]).unwrap_err().contains("cycle"));
    let gate_without_writer = card("g", r#", "gates": [{"measure": "decode_tok_s", "min": 7}]"#);
    assert!(
        parse_cards(&[&gate_without_writer])
            .unwrap_err()
            .contains("writer")
    );
    let typo = card("t", r#", "needs": {"writter": true}"#);
    assert!(parse_cards(&[&typo]).is_err());
    assert!(parse_cards(&["not json"]).is_err());
}

fn repo() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn shipped() -> (Vec<Card>, Catalog) {
    let cards = load_cards(&repo().join("capabilities")).unwrap();
    let catalog = load_catalog(&repo().join("models/catalog.json")).unwrap();
    (cards, catalog)
}

const E5: &str = "multilingual-e5-small-q8_0";
const E2B: &str = "gemma-4-E2B-it-Q4_0";

#[test]
fn the_shipped_cards_and_catalog_load_and_agree() {
    let (cards, catalog) = shipped();
    check(&cards, &catalog).unwrap();
    assert_eq!(cards[0].id, "tasks", "file-name order");
    assert_eq!(cards.last().unwrap().id, "code.check");
    assert!(
        cards
            .iter()
            .all(|c| !c.name.en.is_empty() && !c.name.hi.is_empty())
    );
    // Measured models say so; the ladder's sizes are estimates until measured.
    let e2b = catalog.models.iter().find(|m| m.id == E2B).unwrap();
    assert!(!e2b.estimate && e2b.sha256.is_some());
    assert!(
        catalog
            .models
            .iter()
            .filter(|m| m.sha256.is_none())
            .all(|m| m.estimate)
    );
}

#[test]
fn the_shipped_cards_on_this_laptop_and_on_a_fresh_windows_laptop() {
    let (cards, catalog) = shipped();
    let here = Device {
        models: strings(&[E5, E2B]),
        writer: Some(E2B.into()),
        ..laptop()
    };
    let m = Measured::from([(E2B.to_string(), speed(12.8, 58.5))]);
    for v in evaluate(&cards, &catalog, &here, &m) {
        assert_eq!(v.state, ready(false), "{}", v.id);
    }
    let fresh = Device {
        models: strings(&[E5]),
        ..windows_8gb()
    };
    let v: BTreeMap<String, State> = evaluate(&cards, &catalog, &fresh, &Measured::new())
        .into_iter()
        .map(|v| (v.id, v.state))
        .collect();
    assert_eq!(v["tasks"], ready(false));
    assert_eq!(v["notes.search"], ready(false));
    for id in [
        "chat",
        "email.draft",
        "rewrite.translate",
        "notes.qa",
        "notes.summary",
    ] {
        assert_eq!(
            en(&v[id]),
            ["choose a model: gemma-4-E2B-it-Q4_0 (2.6 GB)"],
            "{id}"
        );
    }
    assert!(is_locked(&v["reminders.desktop"]));
    assert!(is_locked(&v["code.check"]));
}

#[test]
fn check_refuses_a_catalog_that_does_not_fit_the_cards() {
    let (cards, catalog) = shipped();
    let mut no_encoder = catalog.clone();
    no_encoder.models.retain(|m| m.id != E5);
    assert!(check(&cards, &no_encoder).unwrap_err().contains(E5));
    let mut no_writer = catalog.clone();
    no_writer.models.retain(|m| m.role != Role::Writer);
    assert!(check(&cards, &no_writer).unwrap_err().contains("writer"));
    let mut twice = catalog.clone();
    twice.models.push(twice.models[0].clone());
    assert!(check(&cards, &twice).unwrap_err().contains(E5));
}

#[test]
fn a_bad_card_file_is_named() {
    let dir = std::env::temp_dir().join(format!("kestrel_cards_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("10-a.json"),
        r#"{"id": "a", "name": {"en": "A", "hi": "A"}}"#,
    )
    .unwrap();
    std::fs::write(dir.join("20-b.json"), r#"{"id": "b", "nam": {}}"#).unwrap();
    std::fs::write(dir.join("notes.txt"), "not a card").unwrap();
    assert!(load_cards(&dir).unwrap_err().contains("20-b.json"));
    std::fs::remove_file(dir.join("20-b.json")).unwrap();
    assert_eq!(load_cards(&dir).unwrap().len(), 1, "only *.json");
    let _ = std::fs::remove_dir_all(&dir);
}

fn entry(model: &str, file: (u64, u64), decode: f64) -> Entry {
    Entry {
        model: model.into(),
        file,
        binary: (900, 1),
        decode_tok_s: decode,
        prefill_tok_s: 60.0,
        threads: 6,
        measured_at: 1_761_600_000,
    }
}

#[test]
fn a_speed_counts_while_its_model_file_and_binary_are_the_same() {
    let p = Profile {
        entries: vec![
            entry("gemma-e2b", (2_841, 7), 12.8),
            entry("gemma-12b", (7_000, 3), 30.0),
        ],
        ..Profile::default()
    };
    let files = |id: &str| match id {
        "gemma-e2b" => Some((2_841, 7)),
        "gemma-12b" => Some((7_000, 4)), // replaced since
        _ => None,
    };
    let m = p.measured(&files, (900, 1));
    assert_eq!(m.len(), 1);
    assert_eq!(m["gemma-e2b"], speed(12.8, 60.0));
    assert!(
        p.measured(&files, (901, 1)).is_empty(),
        "a new binary measures again"
    );
    assert!(
        p.measured(&|_| None, (900, 1)).is_empty(),
        "the files are gone"
    );
}

#[test]
fn recording_replaces_the_models_earlier_entry_and_survives_json() {
    let mut p = Profile::default();
    p.record(entry("gemma-e2b", (2_841, 7), 11.0));
    p.record(entry("gemma-12b", (7_000, 3), 30.0));
    p.record(entry("gemma-e2b", (2_841, 7), 12.8));
    assert_eq!(p.entries.len(), 2);
    assert_eq!(
        p.entries
            .iter()
            .find(|e| e.model == "gemma-e2b")
            .unwrap()
            .decode_tok_s,
        12.8
    );
    let text = serde_json::to_string_pretty(&p).unwrap();
    assert_eq!(serde_json::from_str::<Profile>(&text).unwrap(), p);
}

const LADDER: &str = r#"{"models": [
  {"id": "e5", "file": "e5.gguf", "file_mib": 126, "memory_mib": 200, "role": "encoder", "source": "t"},
  {"id": "e2b", "file": "e2b.gguf", "file_mib": 2710, "memory_mib": 3900, "role": "writer", "source": "t"},
  {"id": "e4b", "file": "e4b.gguf", "file_mib": 4250, "memory_mib": 6100, "role": "writer", "source": "t"},
  {"id": "12b", "file": "12b.gguf", "file_mib": 6350, "memory_mib": 9100, "role": "writer", "source": "t"},
  {"id": "26b", "file": "26b.gguf", "file_mib": 13390, "memory_mib": 19300, "active_mib": 2020,
   "role": "writer", "source": "t"}
]}"#;

fn ladder() -> Catalog {
    serde_json::from_str(LADDER).unwrap()
}

const GATES: &[Gate] = &[
    Gate {
        measure: Measure::DecodeTokS,
        min: 7.0,
    },
    Gate {
        measure: Measure::PrefillTokS,
        min: 40.0,
    },
];

fn fit<'a>(c: &'a Choice, id: &str) -> &'a Fit {
    c.fits.iter().find(|f| f.model == id).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.05
}

#[test]
fn an_unmeasured_writer_is_estimated_from_a_measured_one_by_its_weights() {
    let m = Measured::from([("e2b".to_string(), speed(12.5, 91.0))]);
    let c = ladder();
    let model = |id: &str| c.models.iter().find(|x| x.id == id).unwrap();
    assert_eq!(
        predict(model("e2b"), &c, &m),
        Some((speed(12.5, 91.0), false))
    );
    let (e4b, estimated) = predict(model("e4b"), &c, &m).unwrap();
    assert!(estimated);
    assert!(near(e4b.decode_tok_s, 12.5 * 2710.0 / 4250.0), "{e4b:?}");
    assert!(near(e4b.prefill_tok_s, 91.0 * 2710.0 / 4250.0), "{e4b:?}");
    // A mixture of experts reads only its active weights per token.
    let (moe, _) = predict(model("26b"), &c, &m).unwrap();
    assert!(near(moe.decode_tok_s, 12.5 * 2710.0 / 2020.0), "{moe:?}");
    assert_eq!(predict(model("e4b"), &c, &Measured::new()), None);
}

#[test]
fn on_this_laptop_e2b_stays_and_e4b_is_the_one_worth_a_download() {
    let d = Device {
        models: strings(&["e5", "e2b"]),
        ..laptop()
    };
    let m = Measured::from([("e2b".to_string(), speed(12.5, 91.0))]);
    let c = choose(&ladder(), &d, &m, GATES);
    assert_eq!(
        c.fits.iter().map(|f| f.model.as_str()).collect::<Vec<_>>(),
        ["26b", "12b", "e4b", "e2b"],
        "largest first"
    );
    assert_eq!(c.best.as_deref(), Some("e2b"));
    assert_eq!(
        c.could_run.as_deref(),
        Some("e4b"),
        "about 8 tokens/s: passes 7"
    );
    assert_eq!(fit(&c, "12b").passes, Some(false), "about 5.3 tokens/s");
    assert!(!fit(&c, "26b").fits, "19.3 GB > 11.9 GB");
    assert!(fit(&c, "e2b").here && !fit(&c, "e2b").estimated);
}

#[test]
fn a_big_machine_picks_its_largest_passing_model_and_points_at_the_next() {
    let d = Device {
        models: strings(&["e5", "e2b", "12b"]),
        ..big()
    };
    let m = Measured::from([("12b".to_string(), speed(30.0, 400.0))]);
    let c = choose(&ladder(), &d, &m, GATES);
    assert_eq!(
        c.best.as_deref(),
        Some("12b"),
        "measured, passes, larger than e2b"
    );
    assert_eq!(
        c.could_run.as_deref(),
        Some("26b"),
        "fits 29 GB; ~94 tokens/s estimated"
    );
}

#[test]
fn without_any_speed_the_smallest_model_here_waits_for_the_speed_test() {
    let d = Device {
        models: strings(&["e5", "e2b", "e4b"]),
        ..laptop()
    };
    let c = choose(&ladder(), &d, &Measured::new(), GATES);
    assert_eq!(c.best.as_deref(), Some("e2b"));
    assert_eq!(c.could_run, None, "nothing to estimate from");
    assert_eq!(fit(&c, "e4b").passes, None);
}

#[test]
fn a_model_too_slow_here_is_never_the_best() {
    let d = Device {
        models: strings(&["e5", "e2b", "12b"]),
        ..laptop()
    };
    let m = Measured::from([
        ("e2b".to_string(), speed(12.5, 91.0)),
        ("12b".to_string(), speed(5.1, 35.0)),
    ]);
    let c = choose(&ladder(), &d, &m, GATES);
    assert_eq!(c.best.as_deref(), Some("e2b"));
    assert_eq!(fit(&c, "12b").passes, Some(false));
    let tiny_only = choose(&ladder(), &tiny(), &Measured::new(), GATES);
    assert_eq!(tiny_only.best, None, "nothing fits 1 GB");
}

#[test]
fn the_writer_gates_are_the_strictest_per_measure() {
    let g = writer_gates(&cards());
    assert_eq!(g.len(), 2);
    assert!(g.contains(&Gate {
        measure: Measure::DecodeTokS,
        min: 7.0
    }));
    assert!(g.contains(&Gate {
        measure: Measure::PrefillTokS,
        min: 40.0
    }));
}

fn calibrated(model: &str, file: (u64, u64), tau_g: f32) -> Calibrated {
    Calibrated {
        model: model.into(),
        file,
        binary: (900, 1),
        temperature: 1.74,
        tau: 0.8,
        tau_g,
        val_n: 152,
        val_wrong: 4,
        val_asked: 7,
        measured_at: 1_790_000_000,
        notes_qa: None,
        code_tasks: None,
    }
}

#[test]
fn a_calibration_is_kept_per_model_and_counts_while_file_and_binary_stay() {
    let mut p = Profile::default();
    p.calibrate(calibrated("gemma-e2b", (2_841, 7), 0.7));
    p.calibrate(calibrated("gemma-e4b", (4_000, 2), 0.6));
    p.calibrate(calibrated("gemma-e2b", (2_841, 7), 0.8));
    assert_eq!(
        p.kinds.len(),
        2,
        "the model's earlier calibration is replaced"
    );
    let files = |m: &str| match m {
        "gemma-e2b" => Some((2_841, 7)),
        _ => None,
    };
    assert_eq!(
        p.calibration("gemma-e2b", &files, (900, 1)).unwrap().tau_g,
        0.8
    );
    assert!(
        p.calibration("gemma-e2b", &files, (901, 1)).is_none(),
        "a new binary"
    );
    assert!(
        p.calibration("gemma-e2b", &|_| Some((1, 1)), (900, 1))
            .is_none(),
        "another file"
    );
    assert!(
        p.calibration("gemma-e4b", &files, (900, 1)).is_none(),
        "the file is gone"
    );
    let text = serde_json::to_string(&p).unwrap();
    assert_eq!(serde_json::from_str::<Profile>(&text).unwrap(), p);
}

#[test]
fn a_profile_from_before_calibrations_still_loads() {
    let p: Profile = serde_json::from_str(r#"{"entries": []}"#).unwrap();
    assert!(p.kinds.is_empty());
}

#[test]
fn pass_rates_ride_with_the_calibration_and_today_s_entries_without_them_still_load() {
    let mut c = calibrated("gemma-e2b", (2_841, 7), 0.7);
    c.notes_qa = Some(NotesQa {
        answerable: Tally {
            n: 20,
            right: 20,
            wrong: 0,
        },
        unanswerable: Tally {
            n: 6,
            right: 4,
            wrong: 2,
        },
    });
    c.code_tasks = Some(CodeTasks {
        n: 18,
        pass1: 12,
        pass_loop: 13,
    });
    let text = serde_json::to_string(&c).unwrap();
    assert_eq!(serde_json::from_str::<Calibrated>(&text).unwrap(), c);
    assert_eq!(c.notes_qa.as_ref().unwrap().answerable.abstained(), 0);
    assert_eq!(
        Tally {
            n: 20,
            right: 17,
            wrong: 0
        }
        .abstained(),
        3
    );
    // As `kestrel calibrate` wrote it on 2026-10-29, before the pass rates.
    let old = r#"{"model": "gemma-4-E2B-it-Q4_0", "file": [2841481184, 1791608727],
        "binary": [10529576, 1793260597], "temperature": 1.7411059, "tau": 0.8, "tau_g": 0.7,
        "val_n": 152, "val_wrong": 6, "val_asked": 4, "measured_at": 1793260792}"#;
    let old: Calibrated = serde_json::from_str(old).unwrap();
    assert_eq!((old.notes_qa, old.code_tasks), (None, None));
    let plain = serde_json::to_string(&calibrated("m", (1, 1), 0.7)).unwrap();
    assert!(
        !plain.contains("notes_qa") && !plain.contains("code_tasks"),
        "{plain}"
    );
}
