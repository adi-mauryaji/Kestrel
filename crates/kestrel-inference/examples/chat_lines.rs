//! One chat model for many questions (v3 code loop, ADR-036): the model loads once (mapped, as in the
//! v3 service), then each stdin line {"system", "user", "max_tokens"} gets one line
//! {"text", "tokens", "ttft_ms", "total_ms", "stopped_by"} (greedy, the model's own chat format with
//! thinking off, the stop string trimmed), or {"error"}. experiments/v3/code_eval.py drives it.
//!
//!     cargo build --release -p kestrel-inference --features llama --example chat_lines
//!     target/release/examples/chat_lines models/Qwen3-4B-Q4_K_M.gguf [N_CTX]

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::Duration;

use kestrel_core::resources::{CancelToken, Deadline};
use kestrel_inference::llama::{LlamaConfig, LlamaSlm};
use kestrel_inference::prompt::{ChatFormat, ChatPrompt};
use kestrel_inference::{GenParams, Generator};
use serde_json::{Value, json};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let model = PathBuf::from(
        args.next()
            .ok_or("usage: chat_lines <model.gguf> [n_ctx]")?,
    );
    let n_ctx = args.next().map(|s| s.parse()).transpose()?.unwrap_or(4096);
    let slm = LlamaSlm::load(
        &model,
        LlamaConfig {
            n_ctx,
            mmap: true,
            ..LlamaConfig::default()
        },
    )?;
    let arch = slm.architecture()?;
    let format =
        ChatFormat::for_architecture(&arch).ok_or(format!("no chat format for {arch:?}"))?;
    let mut session = slm.session()?;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", json!({"ready": true, "architecture": arch}))?;
    out.flush()?;
    for line in std::io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line?).unwrap_or(Value::Null);
        let (system, user) = (
            v["system"].as_str().unwrap_or(""),
            v["user"].as_str().unwrap_or(""),
        );
        let max_tokens = v["max_tokens"].as_u64().unwrap_or(512) as usize;
        let reply = if slm.has_special_tokens(system)? || slm.has_special_tokens(user)? {
            json!({"error": "control-token text in the request"})
        } else {
            let prompt = ChatPrompt::new(format, system);
            let stop = prompt.end_of_answer();
            let params = GenParams {
                stop: vec![stop.into()],
                ..GenParams::greedy(max_tokens)
            };
            let mut text = String::new();
            match session.generate(
                &(prompt.prefix() + &prompt.suffix(user)),
                &params,
                &CancelToken::new(),
                &Deadline::after(Duration::from_secs(600)),
                &mut |t| text.push_str(t),
            ) {
                Ok(st) => {
                    let text = text.strip_suffix(stop).unwrap_or(&text).trim().to_string();
                    json!({"text": text, "tokens": st.tokens, "ttft_ms": st.ttft_ms,
                           "total_ms": st.total_ms, "stopped_by": format!("{:?}", st.stopped_by)})
                }
                Err(e) => json!({"error": e.to_string()}),
            }
        };
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}
