"""experiments/data/intents_v0.py -- build Kestrel-Intents-v0 from templates (Ch. 31.2, ADR-010).

    python experiments/data/intents_v0.py --check        # validate the template + filler files
    python experiments/data/intents_v0.py --out DIR      # expand, dedupe, split, write JSONL + manifest

A *family* is one request written as English and Romanized Hinglish templates. Families are split
whole, so a translation never lands in a different set from its original (RQ7). Out-of-scope families
are grouped by domain instead, so cal/val/test hold OOS from domains training never saw. Compound
families (more than one intent) go to val/test only: they are evaluation-only (ADR-010).
"""
from __future__ import annotations

import argparse
import hashlib
import itertools
import json
import re
import sys
import tomllib
from collections import Counter
from pathlib import Path

from prep import NAMES, dataset_fingerprint, dedupe, group_split4, norm_text, seeded_order

ROOT = Path(__file__).resolve().parents[2]
LABELS_FILE = ROOT / "datasets" / "contracts" / "intents.txt"
DATA = ROOT / "datasets" / "kestrel_intents_v0"
LANGS = ("en", "hi-en")
AUTHORS = ("claude-draft", "human")
FAMILY_KEYS = {"id", "intents", "domain", "author", "reviewed", "note", *LANGS}
SINGLE_FRACS = (0.6, 0.1, 0.1, 0.2)
COMPOUND_FRACS = (0.0, 0.0, 1 / 3, 2 / 3)
SLOT = re.compile(r"\{([a-z_]+)\}")


def load_labels(path=LABELS_FILE):
    lines = path.read_text(encoding="utf-8").splitlines()
    return [l for l in lines if l and not l.startswith("#")]


def load(templates=DATA / "templates.toml", fillers=DATA / "fillers.toml"):
    """Families get the file's default author/reviewed unless they set their own."""
    with open(templates, "rb") as f:
        doc = tomllib.load(f)
    if doc.get("schema") != 1:
        raise ValueError(f"{templates}: expected schema = 1")
    defaults = {"author": doc["default_author"], "reviewed": doc["default_reviewed"]}
    families = [{**defaults, **fam} for fam in doc["family"]]
    with open(fillers, "rb") as f:
        return families, tomllib.load(f)


def validate(families, fillers, labels):
    """Returns a list of problems; empty means the files can be built."""
    errors, ids, seen_templates = [], set(), {}
    for lang in LANGS:
        for slot, values in fillers.get(lang, {}).items():
            if not values or len(set(values)) != len(values):
                errors.append(f"fillers [{lang}] {slot}: empty or has duplicates")
    for fam in families:
        fid = fam.get("id", "?")
        where = f"family {fid!r}"
        if fid in ids:
            errors.append(f"duplicate family id {fid!r}")
        ids.add(fid)
        errors += [f"{where}: unknown key {k!r}" for k in sorted(set(fam) - FAMILY_KEYS)]
        intents = fam.get("intents", [])
        if not intents:
            errors.append(f"{where}: no intents")
        errors += [f"{where}: unknown intent {i!r}" for i in intents if i not in labels]
        if len(set(intents)) != len(intents):
            errors.append(f"{where}: repeats an intent")
        is_oos = intents == ["unsupported"]
        if is_oos and not fam.get("domain"):
            errors.append(f"{where}: unsupported family needs a domain")
        if not is_oos and "domain" in fam:
            errors.append(f"{where}: domain is for single-intent unsupported families only")
        if fam.get("author") not in AUTHORS or not isinstance(fam.get("reviewed"), bool):
            errors.append(f"{where}: author must be one of {AUTHORS} and reviewed a bool")
        for lang in LANGS:
            templates = fam.get(lang, [])
            if not templates:
                errors.append(f"{where}: no {lang} template")
            for t in templates:
                slots = SLOT.findall(t)
                errors += [f"{where}: unknown slot {s!r} in {t!r}" for s in slots
                           if s not in fillers.get(lang, {})]
                if len(set(slots)) != len(slots):
                    errors.append(f"{where}: repeated slot in {t!r}")
                if "{" in SLOT.sub("", t) or "}" in SLOT.sub("", t) or not norm_text(SLOT.sub("x", t)):
                    errors.append(f"{where}: malformed template {t!r}")
                key = (lang, norm_text(t))
                if key in seen_templates:
                    errors.append(f"{where}: duplicate {lang} template {t!r} (also in {seen_templates[key]!r})")
                seen_templates.setdefault(key, fid)
    return errors


def expand(families, fillers, per_template=3, seed=0):
    """Fills each template with up to per_template distinct filler combinations, chosen by a
    seeded hash so the output is the same on every run and machine."""
    rows = []
    for fam in families:
        intents = list(fam["intents"])
        compound = len(intents) > 1
        stratum = "compound" if compound else intents[0]
        group = f"oos:{fam['domain']}" if stratum == "unsupported" else fam["id"]
        for lang in LANGS:
            for t_idx, template in enumerate(fam[lang]):
                slots = SLOT.findall(template)
                combos = {"\x1f".join(c): c for c in
                          itertools.product(*(fillers[lang][s] for s in slots))}
                picked = seeded_order(combos, f"{seed}:{fam['id']}:{lang}:{t_idx}")[:per_template]
                for k, key in enumerate(picked):
                    values = dict(zip(slots, combos[key]))
                    rows.append({
                        "id": f"{fam['id']}/{lang}/{t_idx}/{k}",
                        "text": SLOT.sub(lambda m: values[m.group(1)], template),
                        "label": "|".join(intents),
                        "intents": intents,
                        "lang": lang,
                        "family": fam["id"],
                        "group": group,
                        "stratum": stratum,
                        "reviewed": fam["reviewed"],
                    })
    return rows


def assign_splits(rows, seed=0):
    """{name: rows}. Single-intent rows: 0.6/0.1/0.1/0.2 by group. Compound rows: val/test only."""
    out = {name: [] for name in NAMES}
    single = [r for r in rows if r["stratum"] != "compound"]
    compound = [r for r in rows if r["stratum"] == "compound"]
    for subset, fracs in ((single, SINGLE_FRACS), (compound, COMPOUND_FRACS)):
        if subset:
            idx = group_split4(subset, "group", "stratum", fracs, seed)
            for name in NAMES:
                out[name] += [subset[i] for i in idx[name]]
    return {name: sorted(rs, key=lambda r: r["id"]) for name, rs in out.items()}


def _sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def build(out_dir, seed=0, per_template=3):
    families, fillers = load()
    errors = validate(families, fillers, load_labels())
    if errors:
        raise SystemExit("invalid templates:\n  " + "\n  ".join(errors))
    rows, conflicts = dedupe(expand(families, fillers, per_template, seed))
    splits = assign_splits(rows, seed)
    out_dir.mkdir(parents=True, exist_ok=True)
    for name, rs in splits.items():
        with open(out_dir / f"{name}.jsonl", "w", encoding="utf-8") as f:
            f.writelines(json.dumps(r, ensure_ascii=False) + "\n" for r in rs)
    manifest = {
        "dataset": "kestrel-intents-v0",
        "status": "draft" if any(not f["reviewed"] for f in families) else "reviewed",
        "seed": seed,
        "per_template": per_template,
        "families": len(families),
        "unreviewed_families": sum(not f["reviewed"] for f in families),
        "sources": {p.name: _sha256(p) for p in (DATA / "templates.toml", DATA / "fillers.toml", LABELS_FILE)},
        "fingerprint": dataset_fingerprint(rows),
        "conflicts": conflicts,
        "splits": {name: {
            "n": len(rs),
            "fingerprint": dataset_fingerprint(rs),
            "by_label_lang": dict(sorted(Counter(f"{r['label']}/{r['lang']}" for r in rs).items())),
        } for name, rs in splits.items()},
    }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=1, ensure_ascii=False) + "\n")
    return manifest


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--check", action="store_true", help="validate only")
    ap.add_argument("--out", type=Path, help="output directory for {train,cal,val,test}.jsonl")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--per-template", type=int, default=3)
    a = ap.parse_args(argv)
    if a.check:
        errors = validate(*load(), load_labels())
        print("\n".join(errors) or "ok")
        return 1 if errors else 0
    if not a.out:
        ap.error("--out is required unless --check")
    m = build(a.out, a.seed, a.per_template)
    print(json.dumps({k: m[k] for k in ("status", "families", "fingerprint")}),
          {name: s["n"] for name, s in m["splits"].items()}, f"conflicts={len(m['conflicts'])}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
