//! What Kestrel can do on this device (ADR-049, ADR-052, ADR-053; v4 blueprint §4.2).
//!
//! Each capability is a card (`capabilities/*.json`): what it needs (systems, other cards, programs,
//! model files, the device's writer model, internet, setup steps) and the speed gates it must pass.
//! `evaluate` turns the cards, the model catalog (`models/catalog.json`), what the probe found and
//! what was measured into one state per card: ready (perhaps slow), setup (possible here once the
//! named steps are done) or locked (not on this device, with the reasons). Pure: the probe and the
//! speed test live elsewhere, so every rule is tested here on made-up devices.

use std::collections::{BTreeMap, BTreeSet};

use kestrel_platform::Os;
use serde::Deserialize;

/// What the operating system and other apps are left (estimate; the pilot's devices revisit it).
pub const RESERVE_MIB: u64 = 3072;
/// A gate passed by less than this factor shows as ready but slow.
pub const SLOW_MARGIN: f64 = 1.2;

/// A sentence in English and Hinglish (the page shows one).
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub en: String,
    pub hi: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub id: String,
    pub name: Text,
    #[serde(default)]
    pub needs: Needs,
    #[serde(default)]
    pub gates: Vec<Gate>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Needs {
    /// The systems it runs on (`Os::label`); empty = all three.
    #[serde(default)]
    pub os: Vec<String>,
    /// Why not on the others.
    pub os_reason: Option<Text>,
    /// Other cards that must be ready.
    #[serde(default)]
    pub cards: Vec<String>,
    /// Programs it runs.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Catalog models whose files must be here (the encoder).
    #[serde(default)]
    pub models: Vec<String>,
    /// The device's writer model must be here and pass the gates.
    #[serde(default)]
    pub writer: bool,
    #[serde(default)]
    pub internet: bool,
    #[serde(default)]
    pub setup: Vec<Step>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub id: String,
    pub step: Text,
}

/// A speed the writer model must reach on this device.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub measure: Measure,
    pub min: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    DecodeTokS,
    PrefillTokS,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Writer,
    Encoder,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub file: String,
    pub file_mib: u64,
    /// Resident while it runs: weights, KV cache, buffers.
    pub memory_mib: u64,
    pub role: Role,
    /// Sizes not yet measured on any device.
    #[serde(default)]
    pub estimate: bool,
    /// Of the file, once known (a download is checked against it).
    pub sha256: Option<String>,
    /// Multi-step tool use, from the model card (Tau2, %).
    pub tau2: Option<f64>,
    /// Where the numbers come from.
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub models: Vec<Model>,
}

/// What the probe found.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub os: Os,
    pub ram_total_mib: u64,
    pub gpus: Vec<Gpu>,
    /// Programs on PATH.
    pub tools: Vec<String>,
    /// Catalog ids whose files are here.
    pub models: Vec<String>,
    /// The writer model chosen for this device (the model slot).
    pub writer: Option<String>,
    /// The user switched internet features on.
    pub internet: bool,
    /// `Step::id`s done.
    pub setup_done: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub memory_mib: u64,
    pub kind: GpuKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuKind {
    /// A card with its own memory.
    Discrete,
    /// Apple silicon: Metal's working set (part of the unified memory) is what it may use.
    AppleUnified,
    /// Inside the processor, sharing the RAM already counted (a Vulkan iGPU).
    Integrated,
}

/// Measured on this device.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Speed {
    pub decode_tok_s: f64,
    pub prefill_tok_s: f64,
}

/// Speeds per catalog model id.
pub type Measured = BTreeMap<String, Speed>;

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Ready { slow: bool },
    Setup { steps: Vec<Text> },
    Locked { reasons: Vec<Text> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub id: String,
    pub name: Text,
    pub state: State,
}

/// What a graphics card keeps for itself (its own buffers, the screen).
pub const GPU_RESERVE_MIB: u64 = 512;

/// The memory a model may take here: RAM less `RESERVE_MIB`, or a GPU's, whichever is larger.
pub fn memory_budget_mib(d: &Device) -> u64 {
    let ram = d.ram_total_mib.saturating_sub(RESERVE_MIB);
    let gpu = d
        .gpus
        .iter()
        .map(|g| match g.kind {
            GpuKind::Discrete => g.memory_mib.saturating_sub(GPU_RESERVE_MIB),
            GpuKind::AppleUnified => g.memory_mib,
            GpuKind::Integrated => 0,
        })
        .max()
        .unwrap_or(0);
    ram.max(gpu)
}

/// Cards from their JSON files, checked: unique ids, known dependencies without cycles, gates
/// only on cards that use the writer.
pub fn parse_cards(files: &[&str]) -> Result<Vec<Card>, String> {
    let cards = files
        .iter()
        .map(|f| serde_json::from_str::<Card>(f).map_err(|e| format!("a card file: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let mut ids = BTreeSet::new();
    for c in &cards {
        if !ids.insert(c.id.as_str()) {
            return Err(format!("two cards are called {}", c.id));
        }
        if !c.gates.is_empty() && !c.needs.writer {
            return Err(format!(
                "{}: gates measure the writer model, so the card needs \"writer\": true",
                c.id
            ));
        }
    }
    for c in &cards {
        if let Some(d) = c.needs.cards.iter().find(|d| !ids.contains(d.as_str())) {
            return Err(format!("{} needs an unknown card {d}", c.id));
        }
    }
    // Depth first: a card met again while its own needs are being followed closes a cycle.
    fn follow<'a>(
        id: &'a str,
        by_id: &BTreeMap<&'a str, &'a Card>,
        open: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<(), String> {
        if done.contains(id) {
            return Ok(());
        }
        if !open.insert(id) {
            return Err(format!("a cycle of cards through {id}"));
        }
        for d in &by_id[id].needs.cards {
            follow(d, by_id, open, done)?;
        }
        open.remove(id);
        done.insert(id);
        Ok(())
    }
    let by_id: BTreeMap<&str, &Card> = cards.iter().map(|c| (c.id.as_str(), c)).collect();
    let (mut open, mut done) = (BTreeSet::new(), BTreeSet::new());
    for c in &cards {
        follow(&c.id, &by_id, &mut open, &mut done)?;
    }
    Ok(cards)
}

/// The cards in `dir`: its `*.json` files in file-name order (the page's order).
pub fn load_cards(dir: &std::path::Path) -> Result<Vec<Card>, String> {
    let io = |e: std::io::Error| format!("{}: {e}", dir.display());
    let mut paths = std::fs::read_dir(dir)
        .map_err(io)?
        .map(|e| e.map(|e| e.path()).map_err(io))
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|p| p.extension().is_some_and(|x| x == "json"));
    paths.sort();
    let mut texts = Vec::new();
    for p in &paths {
        let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
        // Each file alone first, so a broken one is named.
        serde_json::from_str::<Card>(&text).map_err(|e| format!("{}: {e}", p.display()))?;
        texts.push(text);
    }
    parse_cards(&texts.iter().map(String::as_str).collect::<Vec<_>>())
        .map_err(|e| format!("{}: {e}", dir.display()))
}

pub fn load_catalog(path: &std::path::Path) -> Result<Catalog, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The cards and the catalog agree: unique model ids, a writer model, and every model a card
/// needs in the catalog.
pub fn check(cards: &[Card], catalog: &Catalog) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    if let Some(m) = catalog.models.iter().find(|m| !ids.insert(m.id.as_str())) {
        return Err(format!("the catalog lists {} twice", m.id));
    }
    if !catalog.models.iter().any(|m| m.role == Role::Writer) {
        return Err("the catalog has no writer model".into());
    }
    for c in cards {
        if let Some(m) = c.needs.models.iter().find(|m| !ids.contains(m.as_str())) {
            return Err(format!("{} needs {m}, which the catalog lacks", c.id));
        }
    }
    Ok(())
}

/// One verdict per card, in the cards' order.
pub fn evaluate(cards: &[Card], catalog: &Catalog, d: &Device, m: &Measured) -> Vec<Verdict> {
    let by_id: BTreeMap<&str, &Card> = cards.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut known = BTreeMap::new();
    cards
        .iter()
        .map(|c| Verdict {
            id: c.id.clone(),
            name: c.name.clone(),
            state: state_of(c, &by_id, catalog, d, m, &mut known),
        })
        .collect()
}

/// A card's state, its needed cards first; `known` holds the states found so far.
fn state_of(
    c: &Card,
    by_id: &BTreeMap<&str, &Card>,
    catalog: &Catalog,
    d: &Device,
    m: &Measured,
    known: &mut BTreeMap<String, State>,
) -> State {
    if let Some(s) = known.get(&c.id) {
        return s.clone();
    }
    // Met again before it is decided: a cycle (parse_cards refuses them), so locked.
    known.insert(
        c.id.clone(),
        State::Locked {
            reasons: vec![say(
                format!("{} depends on itself", c.id),
                format!("{} khud pe tika hai", c.id),
            )],
        },
    );
    let (mut reasons, mut steps, mut slow) = (Vec::new(), Vec::new(), false);
    for need in &c.needs.cards {
        let Some(other) = by_id.get(need.as_str()) else {
            reasons.push(say(
                format!("unknown card {need}"),
                format!("anjaan card {need}"),
            ));
            continue;
        };
        match state_of(other, by_id, catalog, d, m, known) {
            State::Ready { slow: s } => slow |= s,
            State::Setup { steps: theirs } => theirs.into_iter().for_each(|t| add(&mut steps, t)),
            State::Locked { .. } => reasons.push(say(
                format!("needs “{}”, which is not on this device", other.name.en),
                format!("“{}” chahiye, jo is device pe nahi hai", other.name.hi),
            )),
        }
    }
    let os = d.os.label();
    if !c.needs.os.is_empty() && !c.needs.os.iter().any(|o| o == os) {
        reasons.push(
            c.needs
                .os_reason
                .clone()
                .unwrap_or_else(|| say(format!("not on {os} yet"), format!("{os} pe abhi nahi"))),
        );
    }
    for tool in c.needs.tools.iter().filter(|t| !d.tools.contains(t)) {
        add(
            &mut steps,
            say(format!("install {tool}"), format!("{tool} install karo")),
        );
    }
    for id in &c.needs.models {
        match catalog.models.iter().find(|x| &x.id == id) {
            None => reasons.push(unknown_model(id)),
            Some(x) if !d.models.contains(id) => add(
                &mut steps,
                say(
                    format!("get the {id} model ({})", size(x.file_mib)),
                    format!("{id} model laao ({})", size(x.file_mib)),
                ),
            ),
            Some(_) => {}
        }
    }
    if c.needs.writer {
        match writer(c, catalog, d, m) {
            Writer::Ok { slow: s } => slow |= s,
            Writer::Steps(theirs) => theirs.into_iter().for_each(|t| add(&mut steps, t)),
            Writer::Locks(theirs) => reasons.extend(theirs),
        }
    }
    if c.needs.internet && !d.internet {
        add(
            &mut steps,
            say(
                "switch internet features on (Settings)".into(),
                "Settings mein internet wale features on karo".into(),
            ),
        );
    }
    for s in c
        .needs
        .setup
        .iter()
        .filter(|s| !d.setup_done.contains(&s.id))
    {
        add(&mut steps, s.step.clone());
    }
    let state = if !reasons.is_empty() {
        State::Locked { reasons }
    } else if !steps.is_empty() {
        State::Setup { steps }
    } else {
        State::Ready { slow }
    };
    known.insert(c.id.clone(), state.clone());
    state
}

enum Writer {
    Ok { slow: bool },
    Steps(Vec<Text>),
    Locks(Vec<Text>),
}

/// The device's writer model against the card's gates.
fn writer(c: &Card, catalog: &Catalog, d: &Device, m: &Measured) -> Writer {
    let budget = memory_budget_mib(d);
    let Some(id) = &d.writer else {
        // None chosen yet: the smallest writer is the one to suggest, if it fits at all.
        return match catalog
            .models
            .iter()
            .filter(|x| x.role == Role::Writer)
            .min_by_key(|x| x.memory_mib)
        {
            None => Writer::Locks(vec![say(
                "no writer model in the catalog".into(),
                "catalog mein koi writer model nahi".into(),
            )]),
            Some(x) if x.memory_mib <= budget => Writer::Steps(vec![say(
                format!("choose a model: {} ({})", x.id, size(x.file_mib)),
                format!("ek model chuno: {} ({})", x.id, size(x.file_mib)),
            )]),
            Some(x) => Writer::Locks(vec![say(
                format!(
                    "needs about {} of memory for the smallest model; this device can spare {}",
                    size(x.memory_mib),
                    size(budget)
                ),
                format!(
                    "sabse chhote model ko lagbhag {} memory chahiye; is device pe {} khaali ho sakti hai",
                    size(x.memory_mib),
                    size(budget)
                ),
            )]),
        };
    };
    let Some(x) = catalog
        .models
        .iter()
        .find(|x| &x.id == id && x.role == Role::Writer)
    else {
        return Writer::Locks(vec![unknown_model(id)]);
    };
    if x.memory_mib > budget {
        return Writer::Locks(vec![say(
            format!(
                "{id} needs about {} of memory; this device can spare {}: choose a smaller model",
                size(x.memory_mib),
                size(budget)
            ),
            format!(
                "{id} ko lagbhag {} memory chahiye; is device pe {} khaali ho sakti hai: chhota model chuno",
                size(x.memory_mib),
                size(budget)
            ),
        )]);
    }
    if !d.models.contains(id) {
        return Writer::Steps(vec![say(
            format!("download {id} ({})", size(x.file_mib)),
            format!("{id} download karo ({})", size(x.file_mib)),
        )]);
    }
    let Some(speed) = m.get(id) else {
        return Writer::Steps(vec![say(
            "run the speed test (1–2 min)".into(),
            "speed test chalao (1–2 min)".into(),
        )]);
    };
    let (mut locks, mut slow) = (Vec::new(), false);
    for g in &c.gates {
        let (v, en, hi) = match g.measure {
            Measure::DecodeTokS => (speed.decode_tok_s, "writes", "likhta"),
            Measure::PrefillTokS => (speed.prefill_tok_s, "reads", "padhta"),
        };
        // Writing speed to a tenth, reading speed whole: as the page shows them.
        let v_text = match g.measure {
            Measure::DecodeTokS => format!("{v:.1}"),
            Measure::PrefillTokS => format!("{v:.0}"),
        };
        if v < g.min {
            locks.push(say(
                format!("{id} {en} {v_text} tokens/s here; this needs {}", g.min),
                format!(
                    "{id} yahan {v_text} tokens/s {hi} hai; isko {} chahiye",
                    g.min
                ),
            ));
        } else if v < g.min * SLOW_MARGIN {
            slow = true;
        }
    }
    match locks.is_empty() {
        true => Writer::Ok { slow },
        false => Writer::Locks(locks),
    }
}

fn say(en: String, hi: String) -> Text {
    Text { en, hi }
}

fn unknown_model(id: &str) -> Text {
    say(format!("unknown model {id}"), format!("anjaan model {id}"))
}

/// Adds a step once.
fn add(steps: &mut Vec<Text>, t: Text) {
    if !steps.contains(&t) {
        steps.push(t);
    }
}

/// "126 MB", "2.6 GB".
fn size(mib: u64) -> String {
    match mib {
        0..1024 => format!("{mib} MB"),
        _ => format!("{:.1} GB", mib as f64 / 1024.0),
    }
}

#[cfg(test)]
mod tests;
