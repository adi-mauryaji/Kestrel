//! The always-on service's Gemma worker (ADR-036): weights mapped, the primed prompt prefix saved
//! at the first start and loaded at the next, the same answer bytes either way. Needs models/ and
//! a llama build: `cargo test --release -p kestrel-cli --features llama --test worker -- --ignored`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One worker process: waits for ready, reads its file-backed memory, answers one request.
fn start_and_ask(root: &Path, cache: &Path) -> (String, u64) {
    start_and_send(
        root,
        cache,
        r#"{"id":1,"text":"kal shaam 6 baje bill bharna yaad dila dena"}"#,
    )
}

/// One worker process, one message: its reply lines (any `{"part"}` lines, then the answer).
fn start_and_send(root: &Path, cache: &Path, msg: &str) -> (String, u64) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kestrel"))
        .args(["tier4-worker", "experiments/configs/router_intents_v0.json"])
        .current_dir(root)
        .env("XDG_CACHE_HOME", cache)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), r#"{"ready":true}"#);
    let status = std::fs::read_to_string(format!("/proc/{}/status", child.id())).unwrap();
    let file_kib = kestrel_bench::rss::parse_status_kib(&status, "RssFile:").unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{msg}").unwrap();
    let mut reply = String::new();
    loop {
        line.clear();
        out.read_line(&mut line).unwrap();
        reply.push_str(&line);
        if !line.starts_with(r#"{"part":"#) {
            break;
        }
    }
    drop(stdin);
    assert!(child.wait().unwrap().success());
    (reply, file_kib)
}

#[test]
#[ignore = "needs models/ and --features llama"]
fn the_worker_maps_gemma_and_reuses_its_saved_prefix_with_the_same_answer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::temp_dir().join(format!("kestrel_worker_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let (first, file_kib) = start_and_ask(&root, &cache);
    assert!(
        file_kib > 1 << 20,
        "RssFile {file_kib} KiB: the weights were copied, not mapped"
    );
    let saved: Vec<PathBuf> = std::fs::read_dir(cache.join("kestrel"))
        .map(|d| d.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default();
    assert_eq!(saved.len(), 1, "{saved:?}");
    let (second, _) = start_and_ask(&root, &cache);
    assert_eq!(first, second);
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
#[ignore = "needs models/ and --features llama"]
fn the_worker_writes_a_skill_draft_the_same_way_every_time() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::temp_dir().join(format!("kestrel_write_{}", std::process::id()));
    let msg = r#"{"id":0,"write":{"kind":"translate","text":"translate to english: mujhe kal der ho jayegi"}}"#;
    let (first, _) = start_and_send(&root, &cache, msg);
    let lines: Vec<serde_json::Value> = first
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let (answer, parts) = lines.split_last().unwrap();
    let text = answer["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text: {first}"));
    assert!(
        !text.trim().is_empty() && !text.contains("<end_of_turn>"),
        "{text:?}"
    );
    // Shown while written: the parts join to the text.
    let shown: String = parts.iter().map(|p| p["part"].as_str().unwrap()).collect();
    assert!(parts.len() > 1, "{first}");
    assert_eq!(shown, text);
    assert_eq!(start_and_send(&root, &cache, msg).0, first);
    let _ = std::fs::remove_dir_all(&cache);
}

/// What the dev run logged for a val item: Gemma's kind and its confidence at the fitted T.
fn logged_kind(root: &Path, id: &str, t: f64) -> (String, String, f64) {
    let rows =
        std::fs::read_to_string(root.join("experiments/logs/v3/kinds/fallback_outputs_v1.jsonl"))
            .unwrap();
    let row: serde_json::Value = rows
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|r| r["id"] == id)
        .unwrap();
    let val =
        std::fs::read_to_string(root.join("datasets/kestrel_kinds_v1/splits/val.jsonl")).unwrap();
    let text = val
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|r| r["id"] == id)
        .unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    let mut p = 1.0;
    for s in row["gemma"]["steps"].as_array().unwrap() {
        let z: Vec<f64> = s["logits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap() / t)
            .collect();
        let m = z.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let sum: f64 = z.iter().map(|x| (x - m).exp()).sum();
        p *= (z[s["chosen"].as_u64().unwrap() as usize] - m).exp() / sum;
    }
    (text, row["gemma"]["label"].as_str().unwrap().to_string(), p)
}

#[test]
#[ignore = "needs models/ and --features llama"]
fn the_worker_chooses_kinds_as_the_dev_run_did() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::temp_dir().join(format!("kestrel_kind_{}", std::process::id()));
    let cfg: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("experiments/configs/kinds_fallback_v1.json")).unwrap(),
    )
    .unwrap();
    let t = cfg["temperature"].as_f64().unwrap();
    // Three the Kinds-v1 head was unsure of (fallback_dev_v1.json): Gemma chat 0.93, app 0.95, and
    // chat 0.84 for a current fact (below tau_g: the head's answer stands).
    for id in [
        "chat-15/hi-en/0",
        "intents:oos-facts-02/hi-en/0/0",
        "intents:oos-facts-01/hi-en/0/0",
    ] {
        let (text, label, p) = logged_kind(&root, id, t);
        let msg = serde_json::json!({"id": 0, "kind": text}).to_string();
        let (reply, _) = start_and_send(&root, &cache, &msg);
        let v: serde_json::Value = serde_json::from_str(&reply).unwrap();
        let got = &v["probs"][0];
        assert_eq!(got[0].as_str(), Some(label.as_str()), "{id}: {reply}");
        assert!(
            (got[1].as_f64().unwrap() - p).abs() < 1e-4,
            "{id}: {reply} vs {p}"
        );
    }
    let _ = std::fs::remove_dir_all(&cache);
}

#[test]
#[ignore = "needs models/ and --features llama"]
fn the_worker_answers_from_the_notes_or_says_they_do_not() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::temp_dir().join(format!("kestrel_qa_{}", std::process::id()));
    let note = std::fs::read_to_string(
        root.join("datasets/kestrel_notesq_v0/notes/2026-09-02-budget-plan.md"),
    )
    .unwrap();
    let ask = |q: &str| {
        let msg = serde_json::json!({"id": 0, "answer": {"question": q, "notes": [{"name": "2026-09-02-budget-plan.md", "text": note}]}});
        let (reply, _) = start_and_send(&root, &cache, &msg.to_string());
        serde_json::from_str::<serde_json::Value>(&reply).unwrap()
    };
    let v = ask("how much is my rent?");
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("12000") || text.contains("12,000"), "{v}");
    assert!(v["ungrounded"].is_null(), "{v}");
    let v = ask("when does my passport expire?");
    assert!(
        v["text"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .starts_with("not in notes"),
        "{v}"
    );
    let _ = std::fs::remove_dir_all(&cache);
}
