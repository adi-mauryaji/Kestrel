//! One command for Week 6 "Done" (blueprint Ch. 26.4): load the pinned SLM, generate, report.
//!
//!     cargo run --release -p kestrel-inference --features llama --example generate -- \
//!         models/Qwen3-1.7B-Q4_K_M.gguf 64 "The three primary colours are"
//!
//! Prints load time, session (context) creation, then two generations of the same prompt in
//! one session: the second shows TTFT without context creation. Memory per PREREG D12
//! (weights load without mmap; VmHWM with the RssAnon / RssFile split).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use kestrel_bench::{kib_to_mib, rss_breakdown};
use kestrel_core::resources::{CancelToken, Deadline};
use kestrel_inference::llama::{LlamaConfig, LlamaSlm};
use kestrel_inference::{GenParams, Generator};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let model = PathBuf::from(
        args.next()
            .ok_or("usage: generate <model.gguf> [max_tokens] [prompt]")?,
    );
    let max_tokens: usize = args.next().map(|s| s.parse()).transpose()?.unwrap_or(64);
    let prompt = args
        .next()
        .unwrap_or_else(|| "The three primary colours are".to_string());

    let t0 = Instant::now();
    let slm = LlamaSlm::load(&model, LlamaConfig::default())?;
    let load_ms = t0.elapsed().as_secs_f64() * 1e3;
    let t1 = Instant::now();
    let mut session = slm.session()?;
    let session_ms = t1.elapsed().as_secs_f64() * 1e3;
    let prompt_tokens = slm.tokenize(&prompt)?.len();

    let mut text = String::new();
    let mut runs = Vec::new();
    for _ in 0..2 {
        text.clear();
        runs.push(session.generate(
            &prompt,
            &GenParams::greedy(max_tokens),
            &CancelToken::new(),
            &Deadline::after(Duration::from_secs(600)),
            &mut |t| text.push_str(t),
        )?);
    }
    let st = &runs[1];
    let mem = rss_breakdown().ok_or("cannot read /proc/self/status")?;

    println!("model            {}", model.display());
    println!("load_ms          {load_ms:.0}  (no mmap)");
    println!("session_ms       {session_ms:.1}  (context created once per session)");
    println!("prompt_tokens    {prompt_tokens}");
    println!("tokens           {}  ({:?})", st.tokens, st.stopped_by);
    println!(
        "ttft_ms          {:.0}  (first call {:.0})",
        st.ttft_ms.unwrap_or(f64::NAN),
        runs[0].ttft_ms.unwrap_or(f64::NAN)
    );
    println!(
        "decode_tok_per_s {:.2}",
        st.decode_tokens_per_s().unwrap_or(f64::NAN)
    );
    println!("total_ms         {:.0}", st.total_ms);
    println!(
        "memory_mib       peak {:.0}  anon {:.0}  file {:.0}",
        kib_to_mib(mem.peak_kib),
        kib_to_mib(mem.anon_kib),
        kib_to_mib(mem.file_kib)
    );
    println!("---\n{prompt}{text}");
    Ok(())
}
