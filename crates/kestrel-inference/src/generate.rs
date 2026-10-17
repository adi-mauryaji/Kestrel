//! The runtime-agnostic generation contract (blueprint Ch. 12.5, A.8). The llama.cpp adapter
//! implements `Generator`; everything above it is testable with `MockGenerator`.
//!
//! Changes from A.8: cancellation and deadline go through `kestrel_core::resources::check`, so a
//! deadline is `Timeout { tier: Generative }` (A.8: `Invalid`); the mock's script is owned
//! strings; `GenParams::greedy` is the default for classification and plans (Ch. 12.2).
//! As in A.8, the piece that completes a stop sequence is still passed to `on_token`; callers
//! that render text must trim the stop string.

use std::time::Instant;

use kestrel_core::KestrelError;
use kestrel_core::Tier;
use kestrel_core::resources::{CancelToken, Deadline, check};

#[derive(Debug, Clone, PartialEq)]
pub struct GenParams {
    pub max_tokens: usize,
    /// 0.0 = greedy (tool calls, plans, labels).
    pub temperature: f32,
    pub stop: Vec<String>,
}

impl GenParams {
    pub fn greedy(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            temperature: 0.0,
            stop: Vec::new(),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum StopReason {
    MaxTokens,
    StopSequence,
    EndOfText,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenStats {
    pub tokens: usize,
    /// Time from the call to the first generated token: prefill + one decode step.
    pub ttft_ms: Option<f64>,
    pub total_ms: f64,
    pub stopped_by: StopReason,
}

impl GenStats {
    /// Decode throughput after the first token (excludes prefill). None with fewer than 2 tokens.
    pub fn decode_tokens_per_s(&self) -> Option<f64> {
        let decode_ms = self.total_ms - self.ttft_ms?;
        (self.tokens >= 2 && decode_ms > 0.0).then(|| (self.tokens - 1) as f64 / (decode_ms / 1e3))
    }
}

pub trait Generator {
    fn generate(
        &mut self,
        prompt: &str,
        p: &GenParams,
        cancel: &CancelToken,
        deadline: &Deadline,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<GenStats, KestrelError>;
}

/// Deterministic stand-in: emits its script one piece per "token".
pub struct MockGenerator {
    pub script: Vec<String>,
}

impl MockGenerator {
    pub fn new(script: &[&str]) -> Self {
        Self {
            script: script.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Generator for MockGenerator {
    fn generate(
        &mut self,
        _prompt: &str,
        p: &GenParams,
        cancel: &CancelToken,
        deadline: &Deadline,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<GenStats, KestrelError> {
        let start = Instant::now();
        let elapsed_ms = || start.elapsed().as_secs_f64() * 1e3;
        let done = |tokens, ttft_ms, stopped_by| GenStats {
            tokens,
            ttft_ms,
            total_ms: elapsed_ms(),
            stopped_by,
        };
        let (mut ttft, mut out, mut n) = (None, String::new(), 0usize);
        for piece in &self.script {
            check(cancel, deadline, Tier::Generative)?; // polled before every token
            if n >= p.max_tokens {
                return Ok(done(n, ttft, StopReason::MaxTokens));
            }
            on_token(piece);
            out.push_str(piece);
            n += 1;
            ttft.get_or_insert_with(elapsed_ms);
            if p.stop.iter().any(|s| out.ends_with(s.as_str())) {
                return Ok(done(n, ttft, StopReason::StopSequence));
            }
        }
        Ok(done(n, ttft, StopReason::EndOfText))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn params(max: usize, stop: &[&str]) -> GenParams {
        GenParams {
            stop: stop.iter().map(|s| s.to_string()).collect(),
            ..GenParams::greedy(max)
        }
    }

    fn far() -> Deadline {
        Deadline::after(Duration::from_secs(60))
    }

    #[test]
    fn greedy_params_have_zero_temperature_and_no_stop() {
        let p = GenParams::greedy(64);
        assert_eq!((p.max_tokens, p.temperature), (64, 0.0));
        assert!(p.stop.is_empty());
    }

    #[test]
    fn streams_tokens_and_records_ttft() {
        let mut g = MockGenerator::new(&["Hel", "lo", "!"]);
        let mut seen = String::new();
        let st = g
            .generate(
                "hi",
                &params(10, &[]),
                &CancelToken::new(),
                &far(),
                &mut |t| seen.push_str(t),
            )
            .unwrap();
        assert_eq!(seen, "Hello!");
        assert_eq!((st.tokens, st.stopped_by), (3, StopReason::EndOfText));
        assert!(st.ttft_ms.is_some());
        assert!(st.total_ms >= st.ttft_ms.unwrap());
    }

    #[test]
    fn honors_max_tokens_and_stop_sequences() {
        let mut g = MockGenerator::new(&["a", "b", "c", "d"]);
        let none = CancelToken::new();
        let st = g
            .generate("", &params(2, &[]), &none, &far(), &mut |_| {})
            .unwrap();
        assert_eq!((st.tokens, st.stopped_by), (2, StopReason::MaxTokens));
        // A stop string spanning two pieces still stops ("b" + "c").
        let st = g
            .generate("", &params(10, &["bc"]), &none, &far(), &mut |_| {})
            .unwrap();
        assert_eq!((st.tokens, st.stopped_by), (3, StopReason::StopSequence));
    }

    #[test]
    fn cancellation_mid_stream_stops_within_one_token() {
        let mut g = MockGenerator::new(&["a", "b", "c", "d"]);
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        let mut count = 0;
        let r = g.generate("", &params(10, &[]), &cancel, &far(), &mut |_| {
            count += 1;
            if count == 2 {
                c2.cancel();
            }
        });
        assert!(matches!(r, Err(KestrelError::Cancelled)));
        assert_eq!(count, 2);
    }

    #[test]
    fn an_expired_deadline_is_a_generative_timeout() {
        let mut g = MockGenerator::new(&["a"]);
        let r = g.generate(
            "",
            &params(10, &[]),
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
    fn decode_rate_needs_two_tokens_and_excludes_ttft() {
        let s = GenStats {
            tokens: 1,
            ttft_ms: Some(10.0),
            total_ms: 10.0,
            stopped_by: StopReason::EndOfText,
        };
        assert!(s.decode_tokens_per_s().is_none());
        let s = GenStats {
            tokens: 11,
            ttft_ms: Some(100.0),
            total_ms: 1100.0,
            stopped_by: StopReason::EndOfText,
        };
        assert!((s.decode_tokens_per_s().unwrap() - 10.0).abs() < 1e-9);
    }
}
