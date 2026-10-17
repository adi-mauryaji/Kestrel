//! llama.cpp adapter (feature "llama"): `Generator` over `llama-cpp-2 =0.1.157`, which vendors
//! llama.cpp 26394b4e, the commit the CLI benchmarks use (ADR-006/007). Safe API only: the
//! workspace forbids `unsafe`, and the crate keeps all FFI on its side.
//!
//! - Weights load without mmap (PREREG D12), so peak RSS is the real footprint.
//! - Greedy decoding only; any other temperature is rejected until sampling is implemented with
//!   a recorded seed.
//! - `LlamaSlm` owns the weights; `LlamaSession` owns one context (KV cache) and implements
//!   `Generator`. A session clears its KV cache before each call instead of creating a new
//!   context: creating one cost ~160 ms per call (Qwen3-1.7B, n_ctx 2048), which would have
//!   inflated every Tier-4 latency and the always-SLM baseline R0.
//! - Prompts are tokenized by `str_to_token`, which parses special-token text. Callers that put
//!   user text into a prompt must neutralize control tokens first (prompt builder, Week 6).

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

use kestrel_core::resources::{CancelToken, Deadline, check};
use kestrel_core::{KestrelError, Tier};
use llama_cpp_2::TokenToStringError;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use crate::generate::{GenParams, GenStats, Generator, StopReason};
use crate::utf8::Utf8Stream;

/// llama.cpp's backend may be initialized once per process.
static BACKEND: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();

fn backend() -> Result<&'static LlamaBackend, KestrelError> {
    BACKEND
        .get_or_init(|| {
            let mut b = LlamaBackend::init().map_err(|e| e.to_string())?;
            b.void_logs(); // llama.cpp logs to stderr by default; Kestrel reports its own errors
            Ok(b)
        })
        .as_ref()
        .map_err(|e| KestrelError::Invalid(format!("llama.cpp backend: {e}")))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LlamaConfig {
    /// Context window in tokens (prompt + generation). Caps the KV cache (Ch. 28).
    pub n_ctx: u32,
    /// Threads for prefill and decode: one per physical core (6 on the target laptop, ADR-011).
    pub n_threads: i32,
}

impl Default for LlamaConfig {
    fn default() -> Self {
        Self {
            n_ctx: 2048,
            n_threads: 6,
        }
    }
}

/// A loaded GGUF model. Generation happens in a `LlamaSession` borrowed from it.
pub struct LlamaSlm {
    model: LlamaModel,
    cfg: LlamaConfig,
}

impl LlamaSlm {
    pub fn load(path: &Path, cfg: LlamaConfig) -> Result<Self, KestrelError> {
        if !path.is_file() {
            return Err(KestrelError::Storage(format!(
                "model file not found: {}",
                path.display()
            )));
        }
        if cfg.n_ctx == 0 || cfg.n_threads < 1 {
            return Err(invalid(format!("bad config {cfg:?}")));
        }
        let params = LlamaModelParams::default().with_use_mmap(false);
        let model = LlamaModel::load_from_file(backend()?, path, &params)
            .map_err(|_| KestrelError::ModelUnavailable(Tier::Generative))?;
        Ok(Self { model, cfg })
    }

    fn context_params(&self) -> LlamaContextParams {
        LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.cfg.n_ctx))
            .with_n_batch(self.cfg.n_ctx)
            .with_n_threads(self.cfg.n_threads)
            .with_n_threads_batch(self.cfg.n_threads)
    }

    /// One context for repeated generations (one session per concurrent generation).
    pub fn session(&self) -> Result<LlamaSession<'_>, KestrelError> {
        let ctx = self
            .model
            .new_context(backend()?, self.context_params())
            .map_err(|_| KestrelError::ModelUnavailable(Tier::Generative))?;
        Ok(LlamaSession { slm: self, ctx })
    }

    /// Prompt tokens as the model will see them (special-token text is parsed).
    pub fn tokenize(&self, prompt: &str) -> Result<Vec<LlamaToken>, KestrelError> {
        self.model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| invalid(format!("tokenize: {e}")))
    }

    /// The bytes a token renders to (control tokens render as nothing).
    fn piece(&self, token: LlamaToken) -> Result<Vec<u8>, KestrelError> {
        match self.model.token_to_piece_bytes(token, 32, false, None) {
            Err(TokenToStringError::InsufficientBufferSpace(need)) => self
                .model
                .token_to_piece_bytes(token, need.unsigned_abs() as usize, false, None)
                .map_err(rt),
            Err(TokenToStringError::UnknownTokenType) => Ok(Vec::new()),
            other => other.map_err(rt),
        }
    }
}

pub struct LlamaSession<'m> {
    slm: &'m LlamaSlm,
    ctx: LlamaContext<'m>,
}

impl Generator for LlamaSession<'_> {
    fn generate(
        &mut self,
        prompt: &str,
        p: &GenParams,
        cancel: &CancelToken,
        deadline: &Deadline,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<GenStats, KestrelError> {
        let start = Instant::now();
        let elapsed_ms = || start.elapsed().as_secs_f64() * 1e3;
        if p.temperature != 0.0 {
            return Err(invalid(
                "only greedy decoding (temperature 0) is implemented",
            ));
        }
        let slm = self.slm;
        let prompt_tokens = slm.tokenize(prompt)?;
        let n_ctx = slm.cfg.n_ctx as usize;
        if prompt_tokens.is_empty() || prompt_tokens.len() + p.max_tokens > n_ctx {
            return Err(invalid(format!(
                "prompt ({} tokens) + max_tokens ({}) must fit the context ({n_ctx})",
                prompt_tokens.len(),
                p.max_tokens
            )));
        }

        let ctx = &mut self.ctx;
        ctx.clear_kv_cache(); // nothing from the previous request may leak into this one

        // Prefill: the whole prompt in one batch; logits only for the last position.
        let mut batch = LlamaBatch::new(n_ctx, 1);
        let last = prompt_tokens.len() - 1;
        for (i, &t) in prompt_tokens.iter().enumerate() {
            batch.add(t, i as i32, &[0], i == last).map_err(rt)?;
        }
        check(cancel, deadline, Tier::Generative)?;
        ctx.decode(&mut batch).map_err(rt)?;

        let mut sampler = LlamaSampler::greedy();
        let mut utf8 = Utf8Stream::new();
        let (mut ttft, mut out, mut n) = (None, String::new(), 0usize);
        let mut pos = prompt_tokens.len() as i32;
        let stopped_by = loop {
            check(cancel, deadline, Tier::Generative)?; // within one token of a cancel
            if n >= p.max_tokens {
                break StopReason::MaxTokens;
            }
            let token = sampler.sample(ctx, batch.n_tokens() - 1);
            if slm.model.is_eog_token(token) {
                let tail = std::mem::take(&mut utf8).finish();
                if !tail.is_empty() {
                    on_token(&tail);
                }
                break StopReason::EndOfText;
            }
            n += 1;
            ttft.get_or_insert_with(elapsed_ms);
            let text = utf8.push(&slm.piece(token)?);
            if !text.is_empty() {
                on_token(&text);
                out.push_str(&text);
            }
            if p.stop.iter().any(|s| out.ends_with(s.as_str())) {
                break StopReason::StopSequence;
            }
            batch.clear();
            batch.add(token, pos, &[0], true).map_err(rt)?;
            pos += 1;
            ctx.decode(&mut batch).map_err(rt)?;
        };
        Ok(GenStats {
            tokens: n,
            ttft_ms: ttft,
            total_ms: elapsed_ms(),
            stopped_by,
        })
    }
}

fn invalid(msg: impl Into<String>) -> KestrelError {
    KestrelError::Invalid(msg.into())
}

fn rt(e: impl std::fmt::Display) -> KestrelError {
    invalid(format!("llama.cpp: {e}"))
}

#[cfg(test)]
mod tests {
    //! Need a model file: `KESTREL_SLM=models/Qwen3-1.7B-Q4_K_M.gguf cargo test -p
    //! kestrel-inference --features llama -- --ignored --test-threads 1`.
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;

    fn slm() -> LlamaSlm {
        let rel = std::env::var("KESTREL_SLM").expect("set KESTREL_SLM to a GGUF model path");
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = PathBuf::from(&rel);
        let path = if path.is_absolute() {
            path
        } else {
            workspace.join(path)
        };
        LlamaSlm::load(&path, LlamaConfig::default()).unwrap()
    }

    fn far() -> Deadline {
        Deadline::after(Duration::from_secs(300))
    }

    const PROMPT: &str = "The three primary colours are";

    #[test]
    #[ignore = "needs KESTREL_SLM"]
    fn generates_up_to_max_tokens_with_timing() {
        let m = slm();
        let mut g = m.session().unwrap();
        let mut text = String::new();
        let st = g
            .generate(
                PROMPT,
                &GenParams::greedy(64),
                &CancelToken::new(),
                &far(),
                &mut |t| text.push_str(t),
            )
            .unwrap();
        assert!(st.tokens > 0 && st.tokens <= 64, "{st:?}");
        assert!(!text.is_empty());
        assert!(st.ttft_ms.unwrap() > 0.0);
        if st.tokens >= 2 {
            assert!(st.decode_tokens_per_s().unwrap() > 0.0);
        }
    }

    #[test]
    #[ignore = "needs KESTREL_SLM"]
    fn greedy_decoding_is_deterministic_and_sessions_forget_earlier_calls() {
        let m = slm();
        let mut g = m.session().unwrap();
        let mut run = || {
            let mut text = String::new();
            g.generate(
                PROMPT,
                &GenParams::greedy(24),
                &CancelToken::new(),
                &far(),
                &mut |t| text.push_str(t),
            )
            .unwrap();
            text
        };
        let first = run();
        // Same session: the KV cache from the first call must not change the second.
        assert_eq!(first, run());
        let fresh = {
            let mut s = m.session().unwrap();
            let mut text = String::new();
            s.generate(
                PROMPT,
                &GenParams::greedy(24),
                &CancelToken::new(),
                &far(),
                &mut |t| text.push_str(t),
            )
            .unwrap();
            text
        };
        assert_eq!(first, fresh);
    }

    #[test]
    #[ignore = "needs KESTREL_SLM"]
    fn cancellation_stops_within_one_token() {
        let m = slm();
        let mut g = m.session().unwrap();
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        let mut pieces = 0;
        let r = g.generate(PROMPT, &GenParams::greedy(64), &cancel, &far(), &mut |_| {
            pieces += 1;
            if pieces == 3 {
                c2.cancel();
            }
        });
        assert!(matches!(r, Err(KestrelError::Cancelled)), "{r:?}");
        assert_eq!(pieces, 3);
    }

    #[test]
    #[ignore = "needs KESTREL_SLM"]
    fn an_expired_deadline_times_out() {
        let m = slm();
        let mut g = m.session().unwrap();
        let r = g.generate(
            PROMPT,
            &GenParams::greedy(8),
            &CancelToken::new(),
            &Deadline::after(Duration::ZERO),
            &mut |_| {},
        );
        assert!(matches!(
            r,
            Err(KestrelError::Timeout {
                tier: Tier::Generative
            })
        ));
    }

    #[test]
    #[ignore = "needs KESTREL_SLM"]
    fn rejects_sampling_and_oversized_requests() {
        let m = slm();
        let mut g = m.session().unwrap();
        let sampled = GenParams {
            temperature: 0.7,
            ..GenParams::greedy(8)
        };
        let none = CancelToken::new();
        assert!(
            g.generate(PROMPT, &sampled, &none, &far(), &mut |_| {})
                .is_err()
        );
        let huge = GenParams::greedy(LlamaConfig::default().n_ctx as usize);
        assert!(
            g.generate(PROMPT, &huge, &none, &far(), &mut |_| {})
                .is_err()
        );
    }

    #[test]
    fn a_missing_model_file_is_a_storage_error() {
        let r = LlamaSlm::load(Path::new("/no/such/model.gguf"), LlamaConfig::default());
        assert!(matches!(r, Err(KestrelError::Storage(_))));
    }
}
