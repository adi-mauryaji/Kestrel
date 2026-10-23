"""experiments/candidates.py -- the candidate configuration of every experiment (Week 12 "Done", Ch. 52).

    python experiments/candidates.py            # write configs/e4.json, e5.json, e6.json, candidates.json
    python experiments/candidates.py --check    # every recorded hash and cross-reference still holds

A candidate is what a test run would use, chosen on dev; it becomes FROZEN at the PREREG freeze (Week
13), whose manifest-consistency check is `--check`. E1: the router configs (R3, tuned R0, tuned R1 per
dataset). E2: R2 vs R2c, the shared-tau policy with and without the configs' temperatures, risk at
coverage 0.80 and AURC computed on the test outputs (PREREG §3; no threshold to choose). E3: the tier
sets' points (configs/e3_candidates.json). E4: Kestrel-Personas-v0, its conditions and routing; BLOCKED
until the dataset is reviewed and frozen and the PROPOSED protocol accepted. E5: the sweep's selected
configurations and eps (configs/e5.json). E6: the shortlisted SLMs x {Q4, Q8}; BLOCKED until
Kestrel-Tasks-v0 is frozen, delta3 is set and its PREREG metrics item is written.

`--check` re-hashes every recorded file and checks that each router config's config_sha256 matches its
content, that e3_candidates.json was built from the current router configs, that every dataset build
matches its own manifest, and that E6's model hashes match models/MANIFEST.toml.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "experiments" / "router"))
from make_config import verify  # noqa: E402

CONFIGS = "experiments/configs"
DATASETS = ("intents_v0", "clinc150")
ROUTERS = [f"{CONFIGS}/router_{d}{s}.json" for d in DATASETS for s in ("", "_r0", "_r1")]
BUILDS = {  # dataset build -> {manifest key: file}
    "datasets/kestrel_tasks_v0/build": {"items_sha256": "items.jsonl"},
    "datasets/kestrel_memory_v0/build": {"store_sha256": "store.jsonl", "queries_sha256": "queries.jsonl"},
    "datasets/kestrel_personas_v0/build": {"items_sha256": "items.jsonl", "events_sha256": "events.jsonl"},
}
SLMS = ("qwen3-1.7b", "qwen3-4b", "gemma-4-e2b-it")
OUT = f"{CONFIGS}/candidates.json"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _dump(obj) -> str:
    return json.dumps(obj, indent=1, ensure_ascii=False) + "\n"


def _json(rel: str, root: Path = ROOT):
    return json.loads((root / rel).read_text(encoding="utf-8"))


def e4() -> dict:
    personas = _json("datasets/kestrel_personas_v0/build/manifest.json")
    router = _json(f"{CONFIGS}/router_intents_v0.json")
    return {"experiment": "E4 (H4; ADR-028, protocol ADR-030)", "status": "BLOCKED",
            "blocked_by": ["Kestrel-Personas-v0 review and freeze (it is a draft)"],
            "test": "K5 vs off on the 60 exact personalized test requests: bootstrap p over the 20 test aliases, "
                    "each with its 3 exact requests (ADR-030); no regression: general success at K5 >= off - delta",
            "dataset": {"name": "kestrel-personas-v0", "status": personas["status"], "test_personas":
                        sorted(p for p, s in personas["personas"].items() if s == "test"),
                        "items_sha256": personas["items_sha256"], "events_sha256": personas["events_sha256"]},
            "conditions": ["off", "K1", "K3", "K5"],
            "routing": {"primary": "gold intents", "secondary": {"config": f"{CONFIGS}/router_intents_v0.json",
                                                                 "config_sha256": router["config_sha256"],
                                                                 "tiers": [1, 3]}},
            "judge": "kestrel_tools::eval (Kestrel-Tasks-v0's)", "command": "kestrel e4-eval --split test"}


def e5() -> dict:
    sweep = _json("experiments/logs/E5/sweep_dev.json")
    eps = _json("experiments/logs/E5/epsilon_pilot.json")
    return {"experiment": "E5 (H5; ADR-027)", "status": "CANDIDATE",
            "selected": {m: v["cfg"] for m, v in sweep["selected"].items()},
            "orders": sweep["orders"], "seeds": sweep["seeds"],
            "selection": sweep["selection"], "data": sweep["data"],
            "eps": {"eps1_macro_f1": eps["eps1_macro_f1"], "eps2_forgetting": sweep["eps2_forgetting"],
                    "eps3_ece": eps["eps3_ece"]},
            "test_note": "the CLINC150 test items are embedded through the runtime encoder at the test run (Week 14)"}


def e6() -> dict:
    models = tomllib.loads((ROOT / "models" / "MANIFEST.toml").read_text())["model"]
    router = _json(f"{CONFIGS}/router_intents_v0.json")
    grid = [{"model": m["name"], "file": m["file"], "quant": m["quant"], "sha256": m["sha256"]}
            for m in models if m.get("role") == "slm" and m["name"].startswith(SLMS)]
    return {"experiment": "E6 (H6)", "status": "BLOCKED",
            "blocked_by": ["Kestrel-Tasks-v0 review and freeze (it is a draft)",
                           "delta3 from its pilot (~1 h of Gemma, asked before it runs)",
                           "the PREREG E6 metrics item (plan validity / end state, ADR-023) and the title-scoring "
                           "rule"],
            "planner": "LM-only (apps/cli/src/planner.rs), the model file swapped per grid entry (ADR-030); "
                       "ADR-025's hybrid is reported for gemma-4-e2b-it-q4_0 only, outside H6",
            "grid": grid, "baseline": "Q8_0 of the same model", "threads": "6, one per physical core (PREREG §4)",
            "n_ctx": {"LM-only planner (scored)": 4096,
                      "Tier 4 in the hybrid's Intents-v0 cascade (reported only)": router["models"]["tier4"]["n_ctx"]},
            "dataset": "kestrel-tasks-v0 test"}


def build(write: bool = True) -> dict:
    routers = {rel: _json(rel) for rel in ROUTERS}
    texts = {f"{CONFIGS}/e4.json": _dump(e4()), f"{CONFIGS}/e5.json": _dump(e5()), f"{CONFIGS}/e6.json": _dump(e6())}
    if write:
        for rel, text in texts.items():
            (ROOT / rel).write_text(text, encoding="utf-8")
    e6_grid = json.loads(texts[f"{CONFIGS}/e6.json"])["grid"]
    files = {rel: sha256(ROOT / rel) for rel in ROUTERS}
    files |= {f"{CONFIGS}/e3_candidates.json": sha256(ROOT / CONFIGS / "e3_candidates.json")}
    files |= {rel: hashlib.sha256(t.encode()).hexdigest() for rel, t in texts.items()}
    for build_dir, keys in BUILDS.items():
        for name in ("manifest.json", *keys.values()):
            files[f"{build_dir}/{name}"] = sha256(ROOT / build_dir / name)
    for rel in ("models/MANIFEST.toml", "datasets/MANIFEST.toml", "experiments/logs/E5/sweep_dev.json",
                "experiments/logs/E5/epsilon_pilot.json"):
        files[rel] = sha256(ROOT / rel)
    temps = {d: routers[f"{CONFIGS}/router_{d}.json"]["calibration"] for d in DATASETS}
    manifest = {
        "week": 12, "note": "candidates chosen on dev; FROZEN at the PREREG freeze (Week 13)",
        "experiments": {
            "E1": {"status": "CANDIDATE", "files": ROUTERS,
                   "config_sha256": {rel: c["config_sha256"] for rel, c in routers.items()}},
            "E2": {"status": "CANDIDATE", "systems": "R2 (temperatures 1) vs R2c (the R3 configs' temperatures), "
                   "one shared tau over the cascade", "metric": "selective risk at coverage 0.80 and AURC on the "
                   "test outputs (PREREG §3, D4); no threshold is chosen", "temperatures": temps},
            "E3": {"status": "CANDIDATE", "files": [f"{CONFIGS}/e3_candidates.json"]},
            "E4": {"status": "BLOCKED", "blocked_by": json.loads(texts[f"{CONFIGS}/e4.json"])["blocked_by"],
                   "test": json.loads(texts[f"{CONFIGS}/e4.json"])["test"], "files": [f"{CONFIGS}/e4.json"]},
            "E5": {"status": "CANDIDATE", "files": [f"{CONFIGS}/e5.json"]},
            "E6": {"status": "BLOCKED", "blocked_by": json.loads(texts[f"{CONFIGS}/e6.json"])["blocked_by"],
                   "planner": json.loads(texts[f"{CONFIGS}/e6.json"])["planner"], "files": [f"{CONFIGS}/e6.json"],
                   "grid": e6_grid},
        },
        "files": files,
    }
    if write:
        (ROOT / OUT).write_text(_dump(manifest), encoding="utf-8")
    return manifest


def check(manifest: dict, root: Path = ROOT) -> list[str]:
    """Everything that no longer matches the manifest (empty = consistent)."""
    problems = []
    for rel, want in manifest["files"].items():
        path = root / rel
        if not path.exists():
            problems.append(f"{rel}: missing")
        elif sha256(path) != want:
            problems.append(f"{rel}: changed since the manifest")
    for rel in ROUTERS:
        if (root / rel).exists() and not verify(_json(rel, root)):
            problems.append(f"{rel}: config_sha256 does not match its content")
    e3 = root / CONFIGS / "e3_candidates.json"
    if e3.exists():
        for d, v in json.loads(e3.read_text())["datasets"].items():
            cfg = root / CONFIGS / f"router_{d}.json"
            if cfg.exists() and v["router_config_sha256"] != _json(f"{CONFIGS}/router_{d}.json", root)["config_sha256"]:
                problems.append(f"e3_candidates.json: built from an older router_{d}.json")
    for build_dir, keys in BUILDS.items():
        mpath = root / build_dir / "manifest.json"
        if not mpath.exists():
            continue
        built = json.loads(mpath.read_text())
        for key, name in keys.items():
            f = root / build_dir / name
            if f.exists() and sha256(f) != built[key]:
                problems.append(f"{build_dir}/{name}: differs from its build manifest")
    models = root / "models" / "MANIFEST.toml"
    if models.exists():
        shas = {m["name"]: m.get("sha256") for m in tomllib.loads(models.read_text())["model"]}
        for g in manifest["experiments"]["E6"]["grid"]:
            if shas.get(g["model"]) != g["sha256"]:
                problems.append(f"E6 grid: {g['model']} differs from models/MANIFEST.toml")
    return problems


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args(argv)
    if args.check:
        problems = check(json.loads((ROOT / OUT).read_text()))
        print("\n".join(problems) if problems else f"{OUT}: consistent")
        return 1 if problems else 0
    m = build()
    for e, v in m["experiments"].items():
        print(f"{e}: {v['status']}" + (f" ({'; '.join(v['blocked_by'])})" if v["status"] == "BLOCKED" else ""))
    print(f"{len(m['files'])} files hashed -> {OUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
