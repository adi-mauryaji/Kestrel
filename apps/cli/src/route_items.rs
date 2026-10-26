//! `kestrel route-items`: the real cascade (rules, encoder + head, SLM) over a JSONL file, built
//! from a generated router config. Every request goes through `Request::new`, so each tier sees
//! the same normalized text as in the Python dev runs. It routes in the index space of the
//! config's label list, so the same code serves Kestrel-Intents-v0 and CLINC150 (ADR-019).

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use kestrel_core::normalize::normalize;
use kestrel_core::{Request, Tier, TierModel};
use kestrel_inference::llama::{LlamaConfig, LlamaEmbedSession, LlamaEncoder, LlamaSlm};
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
    /// Where Tier 4 keeps its primed prefix (`load_for_service` only).
    prefix_file: Option<std::path::PathBuf>,
}

type DynError = Box<dyn std::error::Error>;

impl Models {
    /// The generative model, if loaded (the planner shares it, ADR-025).
    pub fn slm(&self) -> Option<&LlamaSlm> {
        self.slm.as_ref()
    }

    /// The models the config's own tiers need.
    pub fn load(config_path: &Path) -> Result<Self, DynError> {
        let (_, cfg) = config::load(&std::fs::read_to_string(config_path)?)?;
        Self::load_for(config_path, &cfg.tiers)
    }

    /// The models that `tiers` need (bench-items: the union of its configs' tiers).
    pub fn load_for(config_path: &Path, tiers: &[u8]) -> Result<Self, DynError> {
        Self::load_with(config_path, tiers, false)
    }

    /// Tier 4 for the always-on service's worker (ADR-036): weights mapped, and the primed prefix
    /// kept in `cache` (when given) under a name for this config, model file and context size.
    /// Research runs use `load` / `load_for`: weights copied (D12), prefix computed every time.
    pub fn load_for_service(config_path: &Path, cache: Option<&Path>) -> Result<Self, DynError> {
        let mut models = Self::load_with(config_path, &[4], true)?;
        if let Some(dir) = cache {
            std::fs::create_dir_all(dir)?;
            let t4 = &models.cfg.models.tier4;
            let meta = std::fs::metadata(Path::new("models").join(&t4.model_file))?;
            let mtime = meta.modified()?.duration_since(std::time::UNIX_EPOCH)?;
            let stem = config_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            models.prefix_file = Some(dir.join(format!(
                "tier4-{stem}-{}-{}-{}.kv",
                meta.len(),
                mtime.as_secs(),
                t4.n_ctx
            )));
        }
        Ok(models)
    }

    fn load_with(config_path: &Path, tiers: &[u8], mmap: bool) -> Result<Self, DynError> {
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
                    mmap,
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
            prefix_file: None,
        })
    }

    /// v3's kind head (ADR-037) on the Tier-3 encoder: experiments/artifacts/tier3/kinds_v0-<encoder>,
    /// refused unless it was trained on the same encoder file as the config's Tier-3 head.
    pub fn kinds_tier(
        &self,
    ) -> Result<EncoderTier<LlamaEmbedSession<'_>, crate::kinds::Kind>, DynError> {
        let t3 = &self.cfg.models.tier3;
        let encoder = self
            .encoder
            .as_ref()
            .ok_or("the kind head needs the Tier-3 encoder")?;
        let heads = Path::new("experiments/artifacts/tier3");
        let stem = Path::new(&t3.encoder_file)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let dir = heads.join(format!("kinds_v0-{stem}"));
        let trained_on = |d: &Path| -> Result<String, DynError> {
            let meta: Value = serde_json::from_str(&std::fs::read_to_string(d.join("head.json"))?)?;
            Ok(meta["encoder_sha256"]
                .as_str()
                .unwrap_or_default()
                .to_string())
        };
        if trained_on(&dir)? != trained_on(&heads.join(&t3.head))? {
            return Err(format!("{} was trained on another encoder", dir.display()).into());
        }
        let head = LinearHead::load(&dir)?;
        let cost = self.cfg.est_cost_ms[Tier::Encoder.index()];
        Ok(EncoderTier::new(
            encoder.session()?,
            head,
            cost,
            crate::kinds::Kind::from_label,
        )?)
    }

    /// v3's kind fallback (ADR-037) on the config's Tier-4 model, as the dev run measured it
    /// (crate::kinds::FALLBACK_CONFIG); refused for another model file. Its primed prefix is kept
    /// beside Tier 4's when the service caches one.
    pub fn kinds_fallback(&self) -> Result<GenerativeTier<'_, crate::kinds::Kind>, DynError> {
        use crate::kinds::{FALLBACK_CONFIG, FallbackConfig, KINDS};
        let slm = self
            .slm
            .as_ref()
            .ok_or("the kind fallback needs the Tier-4 model")?;
        let fb = FallbackConfig::load(Path::new(FALLBACK_CONFIG))?;
        let t4 = &self.cfg.models.tier4;
        if fb.model_file != t4.model_file {
            return Err(format!(
                "the kind fallback was measured on {}, not {}",
                fb.model_file, t4.model_file
            )
            .into());
        }
        let arch = slm.architecture()?;
        let format =
            ChatFormat::for_architecture(&arch).ok_or(format!("no chat format for {arch:?}"))?;
        // As for Tier 4: the dev run normalized the examples like requests.
        let shots = fb
            .shots
            .iter()
            .map(|s| (normalize(&s.text), s.label.clone()))
            .collect();
        let prompt = ChatPrompt::new(format, &fb.system).with_examples(shots);
        let answers: Vec<_> = KINDS.iter().map(|&k| (k, k.label())).collect();
        let prefix = self.prefix_file.as_ref().map(|f| {
            f.with_file_name(format!(
                "kinds-{}",
                f.file_name().unwrap_or_default().to_string_lossy()
            ))
        });
        Ok(GenerativeTier::new_cached(
            slm,
            prompt,
            &answers,
            fb.temperature,
            TIER4_TIMEOUT,
            self.cfg.est_cost_ms[Tier::Generative.index()],
            prefix.as_deref(),
        )?)
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
            let tier = GenerativeTier::new_cached(
                slm,
                prompt,
                &space.answers(),
                cfg.calibration.tier4_temperature,
                TIER4_TIMEOUT,
                cost(Tier::Generative),
                self.prefix_file.as_deref(),
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
