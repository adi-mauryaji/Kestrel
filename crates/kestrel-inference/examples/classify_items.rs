//! Tier-4 label classification over a JSONL of requests (B3 / R0 runs on dev splits).
//!
//!     classify_items <model.gguf> <system_prompt.txt> <labels.txt> <items.jsonl> <out.jsonl>
//!
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
    let [model, system, labels, items, out] = args.as_slice() else {
        return Err("usage: classify_items <model.gguf> <system.txt> <labels.txt> <items.jsonl> <out.jsonl>".into());
    };
    let labels: Vec<String> = std::fs::read_to_string(labels)?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect();
    let prompt = ChatPrompt::new(ChatFormat::Qwen3NoThink, &std::fs::read_to_string(system)?);
    let cfg = LlamaConfig {
        n_ctx: 1024,
        ..LlamaConfig::default()
    };
    let slm = LlamaSlm::load(&PathBuf::from(model), cfg)?;
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let trie = slm.label_trie(&label_refs, prompt.end_of_answer())?;
    let mut session = slm.session()?;
    let prefix = prompt.prefix();
    let none = CancelToken::new();

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
