//! Embeds texts one at a time through the Tier-3 runtime path (`LlamaEmbedSession`), so the
//! training cache sees exactly the vectors the runtime will: llama.cpp's numerics depend on the
//! batch size (a 100-text CLI batch differs from single texts by up to ~3e-3 per component).
//!
//!     embed_texts <encoder.gguf> <inputs.txt (one model input per line)> <out.f32> [ms.txt]
//!
//! Writes little-endian f32, row-major [n x dim]; prints `n dim` to stdout. With `ms.txt`, also
//! writes each text's embedding time in ms (after load), one per line: the Tier-3 dev cost.

use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use kestrel_core::Embedder;
use kestrel_inference::llama::LlamaEncoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (model, inputs, out, times) = match args.as_slice() {
        [m, i, o] => (m, i, o, None),
        [m, i, o, t] => (m, i, o, Some(t)),
        _ => {
            return Err("usage: embed_texts <encoder.gguf> <inputs.txt> <out.f32> [ms.txt]".into());
        }
    };
    let enc = LlamaEncoder::load(&PathBuf::from(model), 6)?;
    let session = enc.session()?;
    let text = std::fs::read_to_string(inputs)?;
    let mut file = std::io::BufWriter::new(std::fs::File::create(out)?);
    let mut n = 0usize;
    let mut ms = Vec::new();
    for line in text.lines() {
        let t0 = Instant::now();
        let v = session.embed(line)?;
        ms.push(t0.elapsed().as_secs_f64() * 1e3);
        for x in v {
            file.write_all(&x.to_le_bytes())?;
        }
        n += 1;
    }
    file.flush()?;
    if let Some(path) = times {
        let lines: Vec<String> = ms.iter().map(|m| format!("{m:.4}")).collect();
        std::fs::write(path, lines.join("\n") + "\n")?;
    }
    println!("{n} {}", session.dim());
    Ok(())
}
