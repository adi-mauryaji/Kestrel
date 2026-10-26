"""experiments/v3/kind_fallback.py -- Gemma as the kind decision's fallback (ADR-037), on Kinds-v0 dev.

One Gemma run over cal + val (the test split stays closed) through the Tier-4 runtime path
(classify_items: datasets/contracts/tier4_kind_prompt.txt, k shots per (kind, language) from train,
label-constrained decoding), logged per item next to the kind head's probabilities on the same items.
Gemma's temperature is fitted on cal (BCE of confidence vs correctness, as for Tier 4); then, on val:

  head        as kinds.rs decides: app -> the old path; a skill at or above tau -> written; below -> asked;
  head+gemma  the head's top kind below tau -> Gemma; Gemma at or above tau_g -> its kind; otherwise as
              the head alone (a skill -> asked, app -> the old path).

A taken decision is right or wrong: a skill the request did not ask for is written, or a skill request
goes the old way (and is usually refused). An ask is counted, and whether it names the right kind.

The runtime's settings (prompt + its SHA-256, shots, Gemma's T, tau_g) go to
experiments/configs/kinds_fallback_v0.json, which the service's worker reads (src/kinds.rs).

    python experiments/v3/kind_fallback.py            # ~3-5 min of full CPU: Gemma on 231 items
    python experiments/v3/kind_fallback.py --reuse    # the policies again from the logged outputs
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
for sub in ("data", "analysis", "tier3", "tier4"):
    sys.path.insert(0, str(ROOT / "experiments" / sub))
from b3_dev import TASKS, classify, fit_conf_temperature, model_path, select_shots, step_confidence  # noqa: E402
from embed import embed, load_encoder  # noqa: E402
from head import load_head, logits, softmax  # noqa: E402
from splits import load_splits  # noqa: E402

SLM = "gemma-4-e2b-it-q4_0"
ENCODER = "multilingual-e5-small-q8_0"
HEAD = ROOT / "experiments" / "artifacts" / "tier3" / f"kinds_v0-{ENCODER}"
SHOTS_PER = 2  # as for Intents-v0 (ADR-015)
TAU = 0.7  # kinds.rs TAU (draft)
TAU_G = (0.5, 0.6, 0.7, 0.8, 0.9)
# Draft, chosen on val (fallback_dev.json, 2026-10-26): half the asks of the head alone and no wrong
# skill more; lower values let Gemma's lean to chat through. Judged again on the final head.
TAU_G_CHOSEN = 0.9
CONFIG = ROOT / "experiments" / "configs" / "kinds_fallback_v0.json"
LOG = ROOT / "experiments" / "logs" / "v3" / "kinds"
OUTPUTS, SUMMARY = LOG / "fallback_outputs.jsonl", LOG / "fallback_dev.json"


def decide(head, tau, gemma=None, tau_g=None):
    """("take", kind) or ("ask", kind) for one request. head: {kind: p}; gemma: (kind, confidence),
    or None for the head alone."""
    kind, p = max(head.items(), key=lambda kv: kv[1])
    if p >= tau or (kind == "app" and gemma is None):
        return ("take", kind)
    if gemma is not None and gemma[1] >= tau_g:
        return ("take", gemma[0])
    return ("ask", kind) if kind != "app" else ("take", "app")


def score(decisions, gold):
    taken = [(k, g) for (how, k), g in zip(decisions, gold) if how == "take"]
    asked = [(k, g) for (how, k), g in zip(decisions, gold) if how == "ask"]
    return {"n": len(gold), "taken": len(taken), "right": sum(k == g for k, g in taken),
            "skill_not_asked_for": sum(k != g and k != "app" for k, g in taken),
            "skill_sent_old_way": sum(k == "app" and g != "app" for k, g in taken),
            "asked": len(asked), "asked_right_kind": sum(k == g for k, g in asked)}


def fallback_config(meta: dict, temperature: float, tau_g: float, system: str) -> dict:
    """What the worker needs to run the fallback as the dev run did; `system` is the prompt's text,
    so the runtime cannot read another prompt than the one measured."""
    return {"note": "v3 kind fallback (ADR-037), from experiments/v3/kind_fallback.py; tau_g is a draft",
            "dev_log": str(SUMMARY.relative_to(ROOT)), "slm": meta["slm"], "model_file": meta["model_file"],
            "prompt": meta["prompt"], "prompt_sha256": meta["prompt_sha256"], "system": system,
            "labels": "kinds.txt", "shots": meta["shots"], "temperature": temperature, "tau_g": tau_g,
            "head_sha256": meta["head_sha256"]}


def _sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run_models(splits) -> list[dict]:
    """Per item of cal then val: id, split, lang, gold, the head's probabilities, Gemma's output."""
    labels = [l for l in TASKS["kinds_v0"][1].read_text().split("\n") if l and not l.startswith("#")]
    w, b, t, meta = load_head(HEAD)
    if meta["classes"] != labels:
        raise ValueError(f"head classes {meta['classes']} are not {labels}")
    enc = load_encoder(ENCODER)
    shots = select_shots(splits["train"], labels, SHOTS_PER)
    rows = []
    for name in ("cal", "val"):
        items = splits[name]
        p = softmax(logits(embed(enc, [r["text"] for r in items], chunk=2048), w, b), t)
        out = classify(model_path(SLM), items, shots, task="kinds_v0")
        for r, pr, o in zip(items, p, out):
            rows.append({"id": r["id"], "split": name, "lang": r["lang"], "gold": r["label"],
                         "head": {c: float(x) for c, x in zip(labels, pr)}, "gemma": o})
    return rows, shots


def summarize(rows) -> dict:
    cal = [r for r in rows if r["split"] == "cal" and "steps" in r["gemma"]]
    val = [r for r in rows if r["split"] == "val"]
    t_g = fit_conf_temperature([r["gemma"]["steps"] for r in cal],
                               [r["gemma"]["label"] == r["gold"] for r in cal])
    gemma = [(r["gemma"]["label"], step_confidence(r["gemma"]["steps"], t_g)) if "steps" in r["gemma"]
             else ("app", 0.0) for r in val]
    gold = [r["gold"] for r in val]
    head_top = [max(r["head"].items(), key=lambda kv: kv[1]) for r in val]
    unsure = [i for i, (_, p) in enumerate(head_top) if p < TAU]
    ms = np.array([r["gemma"]["ms"] for r in rows if "ms" in r["gemma"]])
    out = {
        "temperature_gemma": t_g, "temperature_note": "fitted on cal (BCE)",
        "cal_n": len(cal), "cal_gemma_accuracy": float(np.mean([r["gemma"]["label"] == r["gold"] for r in cal])),
        "val_gemma_accuracy": float(np.mean([g[0] == y for g, y in zip(gemma, gold)])),
        "val_head_accuracy": float(np.mean([k == y for (k, _), y in zip(head_top, gold)])),
        "val_head_unsure": {"n": len(unsure),
                            "head_right": sum(head_top[i][0] == gold[i] for i in unsure),
                            "gemma_right": sum(gemma[i][0] == gold[i] for i in unsure)},
        "gemma_ms": {"mean": float(ms.mean()), "p50": float(np.median(ms)),
                     "p95": float(np.sort(ms)[int(np.ceil(0.95 * len(ms))) - 1])},
        "tau": TAU,
        "val_head": score([decide(r["head"], TAU) for r in val], gold),
        "val_head_gemma": {str(tg): score([decide(r["head"], TAU, g, tg) for r, g in zip(val, gemma)], gold)
                           for tg in TAU_G},
        "val_unsure_items": [{"id": val[i]["id"], "gold": gold[i], "head": list(head_top[i]),
                              "gemma": [gemma[i][0], round(gemma[i][1], 3)]} for i in unsure],
    }
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--reuse", action="store_true", help="read the logged outputs instead of running the models")
    args = ap.parse_args(argv)
    LOG.mkdir(parents=True, exist_ok=True)
    if args.reuse:
        rows = [json.loads(l) for l in OUTPUTS.read_text().splitlines()]
        meta = json.loads(SUMMARY.read_text())["meta"]
    else:
        rows, shots = run_models(load_splits("kinds_v0"))
        OUTPUTS.write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows))
        meta = {"slm": SLM, "model_file": model_path(SLM).name, "encoder": ENCODER, "head_sha256": _sha(HEAD / "head.bin"),
                "prompt": TASKS["kinds_v0"][0].name, "prompt_sha256": _sha(TASKS["kinds_v0"][0]),
                "shots_per_kind_and_language": SHOTS_PER, "shots": shots,
                "note": "dev timing on the laptop as found; not a measurement"}
    meta.setdefault("model_file", model_path(SLM).name)
    out = {"meta": meta} | summarize(rows)
    SUMMARY.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n")
    prompt = TASKS["kinds_v0"][0]
    if _sha(prompt) != meta["prompt_sha256"]:
        raise ValueError(f"{prompt.name} changed since the dev run: run it again (without --reuse)")
    cfg = fallback_config(meta, out["temperature_gemma"], TAU_G_CHOSEN, prompt.read_text(encoding="utf-8"))
    CONFIG.write_text(json.dumps(cfg, indent=1, ensure_ascii=False) + "\n")
    print(json.dumps({k: v for k, v in out.items() if k not in ("meta", "val_unsure_items")}, indent=1))
    print(f"wrote {SUMMARY.relative_to(ROOT)} and {CONFIG.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
