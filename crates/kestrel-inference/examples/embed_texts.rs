//! Embeds texts one at a time through the Tier-3 runtime path (`LlamaEmbedSession`), so the
//! training cache sees exactly the vectors the runtime will: llama.cpp's numerics depend on the
//! batch size (a 100-text CLI batch differs from single texts by up to ~3e-3 per component).
//!
//!     embed_texts <encoder.gguf> <inputs.txt (one model input per line)> <out.f32>
//!
//! Writes little-endian f32, row-major [n x dim]; prints `n dim` to stdout.

use std::io::Write;
use std::path::PathBuf;

use kestrel_core::Embedder;
use kestrel_inference::llama::LlamaEncoder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [model, inputs, out] = args.as_slice() else {
        return Err("usage: embed_texts <encoder.gguf> <inputs.txt> <out.f32>".into());
    };
    let enc = LlamaEncoder::load(&PathBuf::from(model), 6)?;
    let session = enc.session()?;
    let text = std::fs::read_to_string(inputs)?;
    let mut file = std::io::BufWriter::new(std::fs::File::create(out)?);
    let mut n = 0usize;
    for line in text.lines() {
        for x in session.embed(line)? {
            file.write_all(&x.to_le_bytes())?;
        }
        n += 1;
    }
    file.flush()?;
    println!("{n} {}", session.dim());
    Ok(())
}
