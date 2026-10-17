//! Text -> fixed-size vector, the input of the Tier-3 head. In core (like `TierModel`) so the
//! tier crate (kestrel-nlu) and the runtime crate (kestrel-inference) both depend on core only
//! (Ch. 24.2). Implementations receive exactly what the head was trained on:
//! `input_prefix + normalized text`.

use crate::KestrelError;

pub trait Embedder {
    fn dim(&self) -> usize;
    fn embed(&self, text: &str) -> Result<Vec<f32>, KestrelError>;
}
