//! Tier-4 label classification over a JSONL of requests (B3 / R0 runs on dev splits).
//!
//!     classify_items <model.gguf> <system_prompt.txt> <labels.txt> <items.jsonl> <out.jsonl> [examples.jsonl]
//!
//! examples (optional): few-shot `{"text", "label"}` lines from the training split, rendered as
//! earlier chat turns in the KV-cached prefix.
//! items: one `{"id", "text"}` per line, `text` already normalized (the Request text every tier
//! sees). out: one line per item with the label, every constrained step (allowed tokens, their
//! logits, the pick) so the confidence can be recomputed at any temperature, and the wall time.
//! Requests whose text tokenizes to control tokens are not sent to the model (`"error"`).

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use kestrel_core::resources::{CancelToken, Deadline};
use kestrel_inference::llama::{LlamaConfig, LlamaSlm};
use kestrel_inference::prompt::{ChatFormat, ChatPrompt};
use serde_json::{Value, json};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (model, system, labels, items, out, examples) = match args.as_slice() {
        [m, s, l, i, o] => (m, s, l, i, o, None),
        [m, s, l, i, o, e] => (m, s, l, i, o, Some(e)),
        _ => return Err("usage: classify_items <model.gguf> <system.txt> <labels.txt> <items.jsonl> <out.jsonl> [examples.jsonl]".into()),
    };
    let labels: Vec<String> = std::fs::read_to_string(labels)?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect();
    let mut shots = Vec::new();
    if let Some(path) = examples {
        for line in std::fs::read_to_string(path)?.lines() {
            let e: Value = serde_json::from_str(line)?;
            let (text, label) = (e["text"].as_str(), e["label"].as_str());
            shots.push((
                text.ok_or("example without text")?.to_string(),
                label.ok_or("example without label")?.to_string(),
            ));
        }
    }
    // Context: KESTREL_N_CTX, default 2048 (Intents-v0 at k = 2 needs ~1250 tokens; CLINC150 with
    // 150 labels and examples ~3k). The size does not change results, only what fits.
    let n_ctx = match std::env::var("KESTREL_N_CTX") {
        Ok(v) => v.parse::<u32>()?,
        Err(_) => LlamaConfig::default().n_ctx,
    };
    let cfg = LlamaConfig {
        n_ctx,
        ..LlamaConfig::default()
    };
    let slm = LlamaSlm::load(&PathBuf::from(model), cfg)?;
    let arch = slm.architecture()?;
    let format =
        ChatFormat::for_architecture(&arch).ok_or(format!("no chat format for {arch:?}"))?;
    let prompt = ChatPrompt::new(format, &std::fs::read_to_string(system)?).with_examples(shots);
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let trie = slm.label_trie(&label_refs, prompt.end_of_answer())?;
    let mut session = slm.session()?;
    let prefix = prompt.prefix();
    let none = CancelToken::new();
    let t0 = Instant::now();
    let n_prefix = session.prime(&prefix)?;
    eprintln!(
        "prefix: {n_prefix} tokens primed in {:.1} s",
        t0.elapsed().as_secs_f64()
    );

    let mut w = BufWriter::new(std::fs::File::create(out)?);
    let (mut n, mut total_ms) = (0usize, 0.0f64);
    for line in BufReader::new(std::fs::File::open(items)?).lines() {
        let item: Value = serde_json::from_str(&line?)?;
        let (id, text) = (
            &item["id"],
            item["text"].as_str().ok_or("item without text")?,
        );
        let row = if slm.has_special_tokens(text)? {
            json!({"id": id, "error": "control-token text in the request"})
        } else {
            let t0 = Instant::now();
            let c = session.classify(
                &prefix,
                &prompt.suffix(text),
                &trie,
                &none,
                &Deadline::after(Duration::from_secs(60)),
            )?;
            let ms = t0.elapsed().as_secs_f64() * 1e3;
            total_ms += ms;
            let steps: Vec<Value> = c
                .steps
                .iter()
                .map(|s| json!({"allowed": s.allowed, "logits": s.logits, "chosen": s.chosen}))
                .collect();
            json!({"id": id, "label": labels[c.label], "steps": steps, "ms": ms})
        };
        writeln!(w, "{row}")?;
        n += 1;
        if n % 25 == 0 {
            eprintln!("{n} items, mean {:.0} ms", total_ms / n as f64);
        }
    }
    w.flush()?;
    eprintln!("done: {n} items, mean {:.0} ms", total_ms / n.max(1) as f64);
    Ok(())
}
