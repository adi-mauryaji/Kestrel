//! Kestrel Tier 4 (blueprint Ch. 12, 26, A.8): the runtime-agnostic `Generator` contract and a
//! deterministic `MockGenerator`; the llama.cpp adapter sits behind the "llama" feature.

pub mod generate;
pub mod labels;
pub mod prompt;
pub mod utf8;

pub use generate::{GenParams, GenStats, Generator, MockGenerator, StopReason};
pub use utf8::Utf8Stream;

#[cfg(feature = "llama")]
pub mod llama;
#[cfg(feature = "llama")]
pub mod tier;
