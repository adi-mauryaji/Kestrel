//! `kestrel route-items`: the real cascade (rules, encoder + head, SLM) over a JSONL file, built
//! from a generated router config. Every request goes through `Request::new`, so each tier sees
//! the same normalized text as in the Python dev runs. It routes in the index space of the
//! config's label list, so the same code serves Kestrel-Intents-v0 and CLINC150 (ADR-019).

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use kestrel_core::normalize::normalize;
use kestrel_core::{Request, Tier, TierModel};
use kestrel_inference::llama::{LlamaConfig, LlamaEncoder, LlamaSlm};
use kestrel_inference::prompt::{ChatFormat, ChatPrompt};
use kestrel_inference::tier::GenerativeTier;
use kestrel_nlu::encoder::{EncoderTier, LinearHead};
use kestrel_nlu::rules::{RuleSet, RulesTier};
use kestrel_router::{Cascade, RouterConfig};
use serde_json::Value;

use crate::config::{self, RuntimeConfig};
use crate::labels::{self, LabelList, LabelSpace, MapLabels};

/// One Tier-4 request may take at most this long (the router's budget is the other bound).
const TIER4_TIMEOUT: Duration = Duration::from_secs(60);
/// One thread per physical core on the target laptop (ADR-011).
const ENCODER_THREADS: i32 = 6;

/// The loaded models, both views of the config and its label list; tiers borrow from it.
pub struct Models {
    pub router: RouterConfig,
    pub cfg: RuntimeConfig,
    /// Tier 4's answers in order, from datasets/contracts/<models.tier4.labels>.
    pub labels: Vec<String>,
    /// Loaded only when Tier 3 / Tier 4 is wanted, so a one-tier system's RSS is its own.
    encoder: Option<LlamaEncoder>,
    slm: Option<LlamaSlm>,
}

type DynError = Box<dyn std::error::Error>;

impl Models {
    /// The models the config's own tiers need.
    pub fn load(config_path: &Path) -> Result<Self, DynError> {
        let (_, cfg) = config::load(&std::fs::read_to_string(config_path)?)?;
        Self::load_for(config_path, &cfg.tiers)
    }

    /// The models that `tiers` need (bench-items: the union of its configs' tiers).
    pub fn load_for(config_path: &Path, tiers: &[u8]) -> Result<Self, DynError> {
        let (router, cfg) = config::load(&std::fs::read_to_string(config_path)?)?;
        let labels = labels::parse_list(&std::fs::read_to_string(
            Path::new("datasets/contracts").join(&cfg.models.tier4.labels),
        )?);
        let encoder = tiers.contains(&3).then(|| {
            LlamaEncoder::load(
                &Path::new("models").join(&cfg.models.tier3.encoder_file),
                ENCODER_THREADS,
            )
        });
        let slm = tiers.contains(&4).then(|| {
            LlamaSlm::load(
                &Path::new("models").join(&cfg.models.tier4.model_file),
                LlamaConfig {
                    n_ctx: cfg.models.tier4.n_ctx,
                    ..LlamaConfig::default()
                },
            )
        });
        Ok(Self {
            router,
            cfg,
            labels,
            encoder: encoder.transpose()?,
            slm: slm.transpose()?,
        })
    }

    /// The config's tiers answering in `space`, each passed through `wrap` (identity for
    /// route-items), in a cascade.
    pub fn cascade<'a, S: LabelSpace>(
        &'a self,
        space: &'a S,
        wrap: impl Fn(Box<dyn TierModel<S::Label> + 'a>) -> Box<dyn TierModel<S::Label> + 'a>,
    ) -> Result<Cascade<'a, S::Label>, DynError> {
        let tiers = self.tiers(space, wrap, &self.cfg.tiers)?;
        Ok(Cascade::new(tiers, self.router.clone())?)
    }

    /// The tiers numbered in `which`, cheapest first, answering in `space` and each passed
    /// through `wrap`. The rules answer Kestrel intents, so they are mapped into `space`.
    pub fn tiers<'a, S: LabelSpace>(
        &'a self,
        space: &'a S,
        wrap: impl Fn(Box<dyn TierModel<S::Label> + 'a>) -> Box<dyn TierModel<S::Label> + 'a>,
        which: &[u8],
    ) -> Result<Vec<Box<dyn TierModel<S::Label> + 'a>>, DynError> {
        let cfg = &self.cfg;
        let cost = |t: Tier| cfg.est_cost_ms[t.index()];
        let uses = |t: Tier| which.contains(&(t as u8));

        let mut tiers: Vec<Box<dyn TierModel<S::Label> + 'a>> = Vec::new();
        if uses(Tier::Rules) {
            // config::load refuses Tier 1 without its confidence.
            let confidence = cfg.calibration.tier1_confidence.unwrap_or_default();
            let rules = RulesTier::new(RuleSet::default_set(), confidence, cost(Tier::Rules))?;
            tiers.push(wrap(Box::new(MapLabels::new(rules, space))));
        }
        if uses(Tier::Encoder) {
            let encoder = self
                .encoder
                .as_ref()
                .ok_or("Tier 3 wanted, encoder not loaded")?;
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
            tiers.push(wrap(Box::new(EncoderTier::new(
                encoder.session()?,
                head,
                cost(Tier::Encoder),
                |c| space.label(c),
            )?)));
        }
        if uses(Tier::Generative) {
            let slm = self.slm.as_ref().ok_or("Tier 4 wanted, SLM not loaded")?;
            let arch = slm.architecture()?;
            let format = ChatFormat::for_architecture(&arch)
                .ok_or(format!("no chat format for {arch:?}"))?;
            let system = std::fs::read_to_string(
                Path::new("datasets/contracts").join(&cfg.models.tier4.prompt),
            )?;
            // Shots are stored raw; the dev runs normalized them like requests.
            let shots = cfg.models.tier4.shots.iter();
            let shots = shots
                .map(|s| (normalize(&s.text), s.label.clone()))
                .collect();
            let prompt = ChatPrompt::new(format, &system).with_examples(shots);
            let tier = GenerativeTier::new(
                slm,
                prompt,
                &space.answers(),
                cfg.calibration.tier4_temperature,
                TIER4_TIMEOUT,
                cost(Tier::Generative),
            )?;
            tiers.push(wrap(Box::new(tier)));
        }
        Ok(tiers)
    }
}

pub fn run(config_path: &Path, items: &Path, out: &Path) -> Result<(), DynError> {
    let models = Models::load(config_path)?;
    let space = LabelList::new(models.labels.clone())?;
    let cascade = models.cascade(&space, |t| t)?;

    let mut w = BufWriter::new(std::fs::File::create(out)?);
    let mut n = 0u64;
    for line in BufReader::new(std::fs::File::open(items)?).lines() {
        let item: Value = serde_json::from_str(&line?)?;
        let text = item["text"].as_str().ok_or("item without text")?;
        n += 1;
        let t0 = Instant::now();
        let (outcome, trace) = cascade.route(&Request::new(n, text));
        let ms = t0.elapsed().as_secs_f64() * 1e3;
        writeln!(
            w,
            "{}",
            config::row(&space, &item["id"], &outcome, &trace, ms)
        )?;
    }
    w.flush()?;
    eprintln!("routed {n} items");
    Ok(())
}
