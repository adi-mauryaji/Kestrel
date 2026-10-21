//! `kestrel route-items`: the real cascade (rules, encoder + head, SLM) over a JSONL file, built
//! from a generated router config. Every request goes through `Request::new`, so each tier sees
//! the same normalized text as in the Python dev runs.

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use kestrel_core::normalize::normalize;
use kestrel_core::{Intent, Request, Tier, TierModel};
use kestrel_inference::llama::{LlamaConfig, LlamaEncoder, LlamaSlm};
use kestrel_inference::prompt::{ChatFormat, ChatPrompt};
use kestrel_inference::tier::GenerativeTier;
use kestrel_nlu::encoder::{EncoderTier, LinearHead};
use kestrel_nlu::rules::{RuleSet, RulesTier};
use kestrel_router::{Cascade, RouterConfig};
use serde_json::Value;

use crate::config::{self, RuntimeConfig};

/// One Tier-4 request may take at most this long (the router's budget is the other bound).
const TIER4_TIMEOUT: Duration = Duration::from_secs(60);
/// One thread per physical core on the target laptop (ADR-011).
const ENCODER_THREADS: i32 = 6;

/// The loaded models and both views of the config; tiers borrow from it.
pub struct Models {
    pub router: RouterConfig,
    pub cfg: RuntimeConfig,
    encoder: LlamaEncoder,
    slm: LlamaSlm,
}

impl Models {
    pub fn load(config_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let (router, cfg) = config::load(&std::fs::read_to_string(config_path)?)?;
        let encoder = LlamaEncoder::load(
            &Path::new("models").join(&cfg.models.tier3.encoder_file),
            ENCODER_THREADS,
        )?;
        let slm = LlamaSlm::load(
            &Path::new("models").join(&cfg.models.tier4.model_file),
            LlamaConfig {
                n_ctx: cfg.models.tier4.n_ctx,
                ..LlamaConfig::default()
            },
        )?;
        Ok(Self {
            router,
            cfg,
            encoder,
            slm,
        })
    }

    /// The config's tiers, each passed through `wrap` (identity for route-items), in a cascade.
    pub fn cascade<'a>(
        &'a self,
        wrap: impl Fn(Box<dyn TierModel + 'a>) -> Box<dyn TierModel + 'a>,
    ) -> Result<Cascade<'a>, Box<dyn std::error::Error>> {
        let cfg = &self.cfg;
        let cost = |t: Tier| cfg.est_cost_ms[t.index()];
        let uses = |t: Tier| cfg.tiers.contains(&(t as u8));

        let head = LinearHead::load(
            &Path::new("experiments/artifacts/tier3").join(&cfg.models.tier3.head),
        )?;
        if (head.temperature() - cfg.calibration.tier3_temperature).abs() > 1e-6 {
            return Err(format!(
                "head temperature {} differs from the config's {}",
                head.temperature(),
                cfg.calibration.tier3_temperature
            )
            .into());
        }
        let arch = self.slm.architecture()?;
        let format =
            ChatFormat::for_architecture(&arch).ok_or(format!("no chat format for {arch:?}"))?;
        let system = std::fs::read_to_string(
            Path::new("datasets/contracts").join(&cfg.models.tier4.prompt),
        )?;
        // Shots are stored raw; the dev runs normalized them like requests.
        let shots = cfg.models.tier4.shots.iter();
        let shots = shots
            .map(|s| (normalize(&s.text), s.label.clone()))
            .collect();
        let prompt = ChatPrompt::new(format, &system).with_examples(shots);

        let mut tiers: Vec<Box<dyn TierModel + 'a>> = Vec::new();
        if uses(Tier::Rules) {
            let rules = RuleSet::default_set();
            tiers.push(wrap(Box::new(RulesTier::new(
                rules,
                cfg.calibration.tier1_confidence,
                cost(Tier::Rules),
            )?)));
        }
        if uses(Tier::Encoder) {
            tiers.push(wrap(Box::new(EncoderTier::new(
                self.encoder.session()?,
                head,
                cost(Tier::Encoder),
            )?)));
        }
        if uses(Tier::Generative) {
            let t = cfg.calibration.tier4_temperature;
            let tier = GenerativeTier::new(
                &self.slm,
                prompt,
                &Intent::ALL,
                t,
                TIER4_TIMEOUT,
                cost(Tier::Generative),
            )?;
            tiers.push(wrap(Box::new(tier)));
        }
        Ok(Cascade::new(tiers, self.router.clone())?)
    }
}

pub fn run(config_path: &Path, items: &Path, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let models = Models::load(config_path)?;
    let cascade = models.cascade(|t| t)?;

    let mut w = BufWriter::new(std::fs::File::create(out)?);
    let mut n = 0u64;
    for line in BufReader::new(std::fs::File::open(items)?).lines() {
        let item: Value = serde_json::from_str(&line?)?;
        let text = item["text"].as_str().ok_or("item without text")?;
        n += 1;
        let t0 = Instant::now();
        let (outcome, trace) = cascade.route(&Request::new(n, text));
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        writeln!(w, "{}", config::row(&item["id"], &outcome, &trace, ms))?;
    }
    w.flush()?;
    eprintln!("routed {n} items");
    Ok(())
}
