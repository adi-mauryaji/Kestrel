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
