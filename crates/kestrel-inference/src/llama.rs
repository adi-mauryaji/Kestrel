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

use std::cell::RefCell;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

use kestrel_core::resources::{CancelToken, Deadline, check};
use kestrel_core::{Embedder, KestrelError, Tier};
use llama_cpp_2::TokenToStringError;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::token_type::LlamaTokenAttr;

use crate::generate::{GenParams, GenStats, Generator, StopReason};
use crate::labels::{LabelChoice, LabelTrie, decode_label};
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

    /// The GGUF `general.architecture` (e.g. "qwen3", "gemma4"); selects the chat format.
    pub fn architecture(&self) -> Result<String, KestrelError> {
        self.model
            .meta_val_str("general.architecture")
            .map_err(|e| invalid(format!("general.architecture: {e}")))
    }

    /// One context for repeated generations (one session per concurrent generation).
    pub fn session(&self) -> Result<LlamaSession<'_>, KestrelError> {
        let ctx = self
            .model
            .new_context(backend()?, self.context_params())
            .map_err(|_| KestrelError::ModelUnavailable(Tier::Generative))?;
        Ok(LlamaSession {
            slm: self,
            ctx,
            cached: Vec::new(),
        })
    }

    /// The label set as a trie of token sequences: `label + end` (e.g. `<|im_end|>`), so no
    /// label is a prefix of another.
    pub fn label_trie(&self, labels: &[&str], end: &str) -> Result<LabelTrie, KestrelError> {
        let seqs = labels
            .iter()
            .map(|l| {
                self.model
                    .str_to_token(&format!("{l}{end}"), AddBos::Never)
                    .map(|ts| ts.iter().map(|t| t.0).collect::<Vec<i32>>())
                    .map_err(|e| invalid(format!("tokenize label {l:?}: {e}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        LabelTrie::new(seqs)
    }

    /// True if `text` tokenizes to any control or user-defined special token. `str_to_token`
    /// parses special-token text, so user text with e.g. `<|im_start|>` would inject a turn.
    pub fn has_special_tokens(&self, text: &str) -> Result<bool, KestrelError> {
        let tokens = self
            .model
            .str_to_token(text, AddBos::Never)
            .map_err(|e| invalid(format!("tokenize: {e}")))?;
        Ok(tokens.iter().any(|&t| {
            let attr = self.model.token_attr(t);
            attr.contains(LlamaTokenAttr::Control) || attr.contains(LlamaTokenAttr::UserDefined)
        }))
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
    /// Prompt prefix whose KV entries sit at positions 0..len of sequence 0 (`classify`).
    cached: Vec<LlamaToken>,
}

impl LlamaSession<'_> {
    /// Decodes `prefix` into the KV cache now, so the first `classify` with it pays only for its
    /// request. A long few-shot prefix (CLINC150: ~3k tokens) takes about a minute to prefill on
    /// the laptop: a load-time cost, not a per-request one.
    pub fn prime(&mut self, prefix: &str) -> Result<usize, KestrelError> {
        let tokens = self.slm.tokenize(prefix)?;
        if tokens.len() >= self.slm.cfg.n_ctx as usize {
            return Err(invalid(format!(
                "prefix ({} tokens) must leave room in the context ({})",
                tokens.len(),
                self.slm.cfg.n_ctx
            )));
        }
        let n = tokens.len();
        self.load_prefix(tokens)?;
        Ok(n)
    }

    /// Makes positions 0..len of sequence 0 hold `prefix_tokens` and nothing after them: reuses
    /// the cached prefix when it matches, otherwise clears the cache and decodes it.
    fn load_prefix(&mut self, prefix_tokens: Vec<LlamaToken>) -> Result<(), KestrelError> {
        if self.cached == prefix_tokens {
            // Keep the prefix, drop the previous request (positions after it).
            return self
                .ctx
                .kv_cache_seq_rm(0, Some(self.cached.len() as u32), None)
                .map_err(rt);
        }
        self.ctx.clear_kv_cache();
        self.cached.clear();
        if !prefix_tokens.is_empty() {
            let mut batch = LlamaBatch::new(prefix_tokens.len(), 1);
            for (i, &t) in prefix_tokens.iter().enumerate() {
                batch.add(t, i as i32, &[0], false).map_err(rt)?;
            }
            self.ctx.decode(&mut batch).map_err(rt)?;
        }
        self.cached = prefix_tokens;
        Ok(())
    }

    /// Classifies by constrained label decoding (`labels::decode_label`). `prefix` (instructions
    /// and label list, the same for every request) stays in the KV cache across calls; `suffix`
    /// (the request and the answer cue) is decoded per call. Cancel and deadline are polled
    /// before every label token.
    pub fn classify(
        &mut self,
        prefix: &str,
        suffix: &str,
        trie: &LabelTrie,
        cancel: &CancelToken,
        deadline: &Deadline,
    ) -> Result<LabelChoice, KestrelError> {
        let slm = self.slm;
        let prefix_tokens = slm.tokenize(prefix)?;
        let suffix_tokens = slm
            .model
            .str_to_token(suffix, AddBos::Never)
            .map_err(|e| invalid(format!("tokenize: {e}")))?;
        let n_ctx = slm.cfg.n_ctx as usize;
        let need = prefix_tokens.len() + suffix_tokens.len() + trie.max_len();
        if suffix_tokens.is_empty() || need > n_ctx {
            return Err(invalid(format!(
                "prompt + longest label ({need} tokens) must fit the context ({n_ctx}) and the suffix may not be empty"
            )));
        }
        check(cancel, deadline, Tier::Generative)?;
        self.load_prefix(prefix_tokens)?;
        let mut batch = LlamaBatch::new(n_ctx, 1);
        let base = self.cached.len();
        let last = suffix_tokens.len() - 1;
        for (i, &t) in suffix_tokens.iter().enumerate() {
            batch
                .add(t, (base + i) as i32, &[0], i == last)
                .map_err(rt)?;
        }
        check(cancel, deadline, Tier::Generative)?;
        self.ctx.decode(&mut batch).map_err(rt)?;

        let ctx = &mut self.ctx;
        let mut pos = (base + suffix_tokens.len()) as i32;
        decode_label(trie, |chosen| {
            check(cancel, deadline, Tier::Generative)?;
            if let Some(&tok) = chosen.last() {
                batch.clear();
                batch
                    .add(LlamaToken::new(tok), pos, &[0], true)
                    .map_err(rt)?;
                pos += 1;
                ctx.decode(&mut batch).map_err(rt)?;
            }
            Ok(ctx.get_logits_ith(batch.n_tokens() - 1).to_vec())
        })
    }
}

impl LlamaSession<'_> {
    /// Greedy generation that can only follow the GBNF `grammar` (rule `root`), e.g. a plan
    /// (kestrel-tools `grammar::gbnf`, ADR-023). `prefix` (instructions and examples, the same
    /// for every request) stays in the KV cache; `suffix` is the request and the answer cue.
    /// Returns the text and the stats.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_constrained(
        &mut self,
        prefix: &str,
        suffix: &str,
        grammar: &str,
        root: &str,
        p: &GenParams,
        cancel: &CancelToken,
        deadline: &Deadline,
    ) -> Result<(String, GenStats), KestrelError> {
        let sampler = LlamaSampler::chain_simple([
            LlamaSampler::grammar(&self.slm.model, grammar, root)
                .map_err(|e| invalid(format!("grammar: {e}")))?,
            LlamaSampler::greedy(),
        ]);
        let mut out = String::new();
        let stats = self.decode_loop(
            Some(prefix),
            suffix,
            sampler,
            p,
            cancel,
            deadline,
            &mut |t| out.push_str(t),
        )?;
        Ok((out, stats))
    }
}

impl LlamaSlm {
    /// Whether every token of `text` (tokenized without BOS) is allowed by `grammar` from its
    /// start: checks that a grammar admits a known-good output, e.g. every gold plan.
    pub fn grammar_admits(
        &self,
        grammar: &str,
        root: &str,
        text: &str,
    ) -> Result<bool, KestrelError> {
        let mut s = LlamaSampler::grammar(&self.model, grammar, root)
            .map_err(|e| invalid(format!("grammar: {e}")))?;
        let tokens = self
            .model
            .str_to_token(text, AddBos::Never)
            .map_err(|e| invalid(format!("tokenize: {e}")))?;
        Ok(tokens.into_iter().all(|t| s.try_accept(t).is_ok()))
    }
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
        if p.temperature != 0.0 {
            return Err(invalid(
                "only greedy decoding (temperature 0) is implemented",
            ));
        }
        self.decode_loop(
            None,
            prompt,
            LlamaSampler::greedy(),
            p,
            cancel,
            deadline,
            on_token,
        )
    }
}

impl LlamaSession<'_> {
    /// Prefill `prompt`, then sample with `sampler` until end of generation, a stop sequence or
    /// `max_tokens`; cancel and deadline are polled before every token.
    /// Without `prefix`, the whole `rest` is prefilled into an empty cache (nothing from an
    /// earlier request remains). With it, the prefix stays in the KV cache across calls, as in
    /// `classify`, and only `rest` is decoded after it.
    #[allow(clippy::too_many_arguments)]
    fn decode_loop(
        &mut self,
        prefix: Option<&str>,
        rest: &str,
        mut sampler: LlamaSampler,
        p: &GenParams,
        cancel: &CancelToken,
        deadline: &Deadline,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<GenStats, KestrelError> {
        let start = Instant::now();
        let elapsed_ms = || start.elapsed().as_secs_f64() * 1e3;
        let slm = self.slm;
        let (prefix_tokens, rest_tokens) = match prefix {
            Some(pre) => (
                slm.tokenize(pre)?,
                slm.model
                    .str_to_token(rest, AddBos::Never)
                    .map_err(|e| invalid(format!("tokenize: {e}")))?,
            ),
            None => (Vec::new(), slm.tokenize(rest)?),
        };
        let n_ctx = slm.cfg.n_ctx as usize;
        let need = prefix_tokens.len() + rest_tokens.len() + p.max_tokens;
        if rest_tokens.is_empty() || need > n_ctx {
            return Err(invalid(format!(
                "prompt ({} tokens) + max_tokens ({}) must fit the context ({n_ctx})",
                prefix_tokens.len() + rest_tokens.len(),
                p.max_tokens
            )));
        }

        check(cancel, deadline, Tier::Generative)?;
        if prefix.is_some() {
            self.load_prefix(prefix_tokens)?;
        } else {
            self.ctx.clear_kv_cache(); // nothing from the previous request may leak into this one
            self.cached.clear();
        }
        let base = self.cached.len();
        let ctx = &mut self.ctx;

        // Prefill: the rest of the prompt in one batch; logits only for the last position.
        let mut batch = LlamaBatch::new(n_ctx, 1);
        let last = rest_tokens.len() - 1;
        for (i, &t) in rest_tokens.iter().enumerate() {
            batch
                .add(t, (base + i) as i32, &[0], i == last)
                .map_err(rt)?;
        }
        check(cancel, deadline, Tier::Generative)?;
        ctx.decode(&mut batch).map_err(rt)?;

        let mut utf8 = Utf8Stream::new();
        let (mut ttft, mut out, mut n) = (None, String::new(), 0usize);
        let mut pos = (base + rest_tokens.len()) as i32;
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

/// A GGUF sentence encoder for Tier 3 (multilingual-e5-small Q8_0, ADR-014), loaded without
/// mmap. Embeds like `llama-embedding`, which built the training cache: `str_to_token` with
/// special tokens (= `common_tokenize(ctx, text, true, true)`), the model's pooling, L2 norm.
pub struct LlamaEncoder {
    model: LlamaModel,
    n_threads: i32,
}

impl LlamaEncoder {
    pub fn load(path: &Path, n_threads: i32) -> Result<Self, KestrelError> {
        if !path.is_file() {
            return Err(KestrelError::Storage(format!(
                "encoder file not found: {}",
                path.display()
            )));
        }
        if n_threads < 1 {
            return Err(invalid(format!("n_threads must be >= 1, got {n_threads}")));
        }
        let params = LlamaModelParams::default().with_use_mmap(false);
        let model = LlamaModel::load_from_file(backend()?, path, &params)
            .map_err(|_| KestrelError::ModelUnavailable(Tier::Encoder))?;
        Ok(Self { model, n_threads })
    }

    pub fn session(&self) -> Result<LlamaEmbedSession<'_>, KestrelError> {
        // The encoder's trained context (512 for e5). Non-causal attention needs the whole input
        // in one micro-batch, so n_ubatch = n_batch = n_ctx, as in llama-embedding.
        let n_ctx = self.model.n_ctx_train();
        let params = LlamaContextParams::default()
            .with_embeddings(true)
            .with_n_ctx(NonZeroU32::new(n_ctx))
            .with_n_batch(n_ctx)
            .with_n_ubatch(n_ctx)
            .with_n_threads(self.n_threads)
            .with_n_threads_batch(self.n_threads);
        let ctx = self
            .model
            .new_context(backend()?, params)
            .map_err(|_| KestrelError::ModelUnavailable(Tier::Encoder))?;
        let dim = usize::try_from(self.model.n_embd_out()).map_err(rt)?;
        Ok(LlamaEmbedSession {
            enc: self,
            ctx: RefCell::new(ctx),
            n_ctx: n_ctx as usize,
            dim,
        })
    }
}

/// One context for repeated embeddings. `Embedder::embed` takes `&self`, so the context sits in a
/// `RefCell`; a re-entrant call gets an error instead of a panic.
pub struct LlamaEmbedSession<'m> {
    enc: &'m LlamaEncoder,
    ctx: RefCell<LlamaContext<'m>>,
    n_ctx: usize,
    dim: usize,
}

impl Embedder for LlamaEmbedSession<'_> {
    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>, KestrelError> {
        let tokens = self
            .enc
            .model
            .str_to_token(text, AddBos::Always)
            .map_err(|e| invalid(format!("tokenize: {e}")))?;
        if tokens.is_empty() || tokens.len() > self.n_ctx {
            return Err(invalid(format!(
                "input has {} tokens; the encoder takes 1..={}",
                tokens.len(),
                self.n_ctx
            )));
        }
        let mut ctx = self
            .ctx
            .try_borrow_mut()
            .map_err(|_| invalid("embedder is already in use"))?;
        ctx.clear_kv_cache();
        let mut batch = LlamaBatch::new(tokens.len(), 1);
        for (i, &t) in tokens.iter().enumerate() {
            batch.add(t, i as i32, &[0], true).map_err(rt)?;
        }
        ctx.decode(&mut batch).map_err(rt)?;
        let pooled = ctx.embeddings_seq_ith(0).map_err(rt)?;
        if pooled.len() != self.dim {
            return Err(invalid(format!(
                "pooled {} dims, expected {}",
                pooled.len(),
                self.dim
            )));
        }
        // L2 norm in f64, like llama.cpp's common_embd_normalize.
        let norm = pooled
            .iter()
            .map(|&x| f64::from(x).powi(2))
            .sum::<f64>()
            .sqrt();
        if !(norm > 0.0 && norm.is_finite()) {
            return Err(invalid("embedding has zero or non-finite norm"));
        }
        Ok(pooled
            .iter()
            .map(|&x| (f64::from(x) / norm) as f32)
            .collect())
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

    fn workspace_path(var: &str) -> PathBuf {
        let rel = std::env::var(var).unwrap_or_else(|_| panic!("set {var}"));
        let path = PathBuf::from(&rel);
        if path.is_absolute() {
            path
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(path)
        }
    }

    fn encoder() -> LlamaEncoder {
        LlamaEncoder::load(&workspace_path("KESTREL_ENCODER"), 6).unwrap()
    }

    #[test]
    #[ignore = "needs KESTREL_ENCODER"]
    fn embeds_unit_vectors_of_the_model_dimension() {
        let enc = encoder();
        let s = enc.session().unwrap();
        let v = s.embed("query: remind me to call asha").unwrap();
        assert_eq!(v.len(), s.dim());
        assert_eq!(s.dim(), 384);
        let norm = v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "norm {norm}");
        // The same text twice in one session gives the same vector.
        assert_eq!(v, s.embed("query: remind me to call asha").unwrap());
    }

    #[test]
    #[ignore = "needs KESTREL_ENCODER and KESTREL_EMBED_CASES"]
    /// Cases: `python experiments/tier3/embed.py --embed-cases …`, i.e. llama-embedding with one
    /// text per run. (A many-text CLI batch differs by up to ~3e-3: llama.cpp's numerics depend on
    /// the batch size, which is why the training cache now uses this runtime path.)
    fn matches_llama_embedding_on_single_texts() {
        let enc = encoder();
        let s = enc.session().unwrap();
        let cases: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(workspace_path("KESTREL_EMBED_CASES")).unwrap(),
        )
        .unwrap();
        let tol = cases["tolerance_max_abs"].as_f64().unwrap();
        let inputs = cases["inputs"].as_array().unwrap();
        let want = cases["embeddings"].as_array().unwrap();
        assert_eq!(inputs.len(), 10);
        let mut worst = 0.0f64;
        for (text, w) in inputs.iter().zip(want) {
            let got = s.embed(text.as_str().unwrap()).unwrap();
            for (g, w) in got.iter().zip(w.as_array().unwrap()) {
                worst = worst.max((*g as f64 - w.as_f64().unwrap()).abs());
            }
        }
        assert!(worst <= tol, "max |rust - python| = {worst:e} > {tol:e}");
    }

    #[test]
    #[ignore = "needs KESTREL_ENCODER"]
    fn rejects_input_longer_than_the_context() {
        let enc = encoder();
        let s = enc.session().unwrap();
        let long = "word ".repeat(2000);
        assert!(matches!(s.embed(&long), Err(KestrelError::Invalid(_))));
        assert!(s.embed("still fine afterwards").is_ok());
    }

    const SYS: &str = "<|im_start|>system\nAnswer with exactly one label: fruit, vehicle or colour.<|im_end|>\n<|im_start|>user\n";

    fn ask(item: &str) -> String {
        format!("{item}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n")
    }

    #[test]
    #[ignore = "needs KESTREL_SLM"]
    fn classify_agrees_with_free_greedy_generation() {
        // Constrained decoding must pick what unconstrained greedy decoding writes whenever that
        // is a label. (Not a quality test: with thinking off, Qwen3-1.7B answers "colour" for
        // "bicycle", in both modes.)
        let m = slm();
        let labels = ["fruit", "vehicle", "colour"];
        let trie = m.label_trie(&labels, "<|im_end|>").unwrap();
        let none = CancelToken::new();
        let mut s = m.session().unwrap();
        let mut checked = 0;
        for item in ["banana", "bicycle", "crimson", "truck"] {
            let mut free = String::new();
            s.generate(
                &format!("{SYS}{}", ask(item)),
                &GenParams::greedy(4),
                &none,
                &far(),
                &mut |t| free.push_str(t),
            )
            .unwrap();
            let c = s.classify(SYS, &ask(item), &trie, &none, &far()).unwrap();
            let conf = c.confidence(1.0);
            assert!(conf > 0.0 && conf <= 1.0);
            if let Some(i) = labels.iter().position(|l| free.trim() == *l) {
                assert_eq!(c.label, i, "{item}: free {free:?} vs {c:?}");
                checked += 1;
            }
        }
        assert!(checked >= 2, "too few free answers were labels to compare");
    }

    #[test]
    #[ignore = "needs KESTREL_SLM"]
    fn the_prefix_cache_changes_nothing() {
        let m = slm();
        let trie = m
            .label_trie(&["fruit", "vehicle", "colour"], "<|im_end|>")
            .unwrap();
        let none = CancelToken::new();
        let mut warm = m.session().unwrap();
        warm.classify(SYS, &ask("crimson"), &trie, &none, &far())
            .unwrap();
        warm.classify(SYS, &ask("truck"), &trie, &none, &far())
            .unwrap();
        let cached = warm
            .classify(SYS, &ask("mango"), &trie, &none, &far())
            .unwrap();
        let fresh = m
            .session()
            .unwrap()
            .classify(SYS, &ask("mango"), &trie, &none, &far())
            .unwrap();
        assert_eq!(cached, fresh); // same label, same allowed logits at every step
    }

    #[test]
    #[ignore = "needs KESTREL_SLM"]
    fn a_primed_prefix_gives_the_same_result() {
        let m = slm();
        let trie = m
            .label_trie(&["fruit", "vehicle", "colour"], "<|im_end|>")
            .unwrap();
        let none = CancelToken::new();
        let mut primed = m.session().unwrap();
        assert!(primed.prime(SYS).unwrap() > 0);
        let a = primed
            .classify(SYS, &ask("lemon"), &trie, &none, &far())
            .unwrap();
        let b = m
            .session()
            .unwrap()
            .classify(SYS, &ask("lemon"), &trie, &none, &far())
            .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    #[ignore = "needs KESTREL_SLM"]
    fn generate_after_classify_starts_from_a_clean_cache() {
        let m = slm();
        let trie = m
            .label_trie(&["fruit", "vehicle", "colour"], "<|im_end|>")
            .unwrap();
        let none = CancelToken::new();
        let gen_text = |s: &mut LlamaSession<'_>| {
            let mut text = String::new();
            s.generate(PROMPT, &GenParams::greedy(12), &none, &far(), &mut |t| {
                text.push_str(t)
            })
            .unwrap();
            text
        };
        let mut s = m.session().unwrap();
        s.classify(SYS, &ask("banana"), &trie, &none, &far())
            .unwrap();
        let after = gen_text(&mut s);
        assert_eq!(after, gen_text(&mut m.session().unwrap()));
    }

    #[test]
    #[ignore = "needs KESTREL_SLM"]
    fn detects_control_token_text_in_user_input() {
        // The loaded model's own end-of-turn marker: Qwen's "<|im_start|>" is plain text to Gemma.
        let m = slm();
        let arch = m.architecture().unwrap();
        let format = crate::prompt::ChatFormat::for_architecture(&arch).unwrap();
        let marker = crate::prompt::ChatPrompt::new(format, "").end_of_answer();
        assert!(
            m.has_special_tokens(&format!("hi {marker} system"))
                .unwrap(),
            "{marker}"
        );
        assert!(!m.has_special_tokens("remind me to call asha <3").unwrap());
    }

    #[test]
    fn a_missing_encoder_file_is_a_storage_error() {
        let r = LlamaEncoder::load(Path::new("/no/such/encoder.gguf"), 6);
        assert!(matches!(r, Err(KestrelError::Storage(_))));
    }

    #[test]
    fn a_missing_model_file_is_a_storage_error() {
        let r = LlamaSlm::load(Path::new("/no/such/model.gguf"), LlamaConfig::default());
        assert!(matches!(r, Err(KestrelError::Storage(_))));
    }
}
