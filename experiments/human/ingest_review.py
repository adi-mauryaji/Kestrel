"""experiments/human/ingest_review.py -- brings the Dataset Review page's decisions into the repo.

Decisions arrive two ways, as for the Request Lab (ingest.py):
- the page's store: export the `reviews` collection with ArtifactData `list` + `out_dir`, which writes
  each document's body as reviews/<unit id>.json (a {"id", "data"} wrapper is read too);
- pasted text: the page's "Copy review" (a kestrel-review-set), saved as a .txt/.json file.

    python experiments/human/ingest_review.py EXPORT_DIR_OR_INBOX [...]   # validate, merge, write the log
    python experiments/human/ingest_review.py --status                    # from the log

A decision names a unit (review_data.py) and the hash of what the page showed; one for a unit that
changed since is stale, and the unit needs a new look. The latest decision per unit wins, and the
log keeps every earlier ingest's. A dataset is ready to freeze when each of its units has a current
"ok". Out: experiments/logs/human/dataset_review.json (decisions, per-dataset status, the fixes asked).
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from review_data import ROOT, TITLES, all_units

LOG = ROOT / "experiments" / "logs" / "human" / "dataset_review.json"
DECISIONS = ("ok", "fix")


def _json_in(text: str) -> dict:
    start, end = text.find("{"), text.rfind("}")
    if start < 0 or end < start:
        raise ValueError("no JSON object")
    return json.loads(text[start : end + 1])


def parse(text: str) -> list[dict]:
    """The decision records in one exported document or one copied review set."""
    raw = _json_in(text)
    if isinstance(raw.get("data"), dict) and "kind" not in raw:  # an exported store document
        raw = raw["data"]
    if raw.get("version") != 1 or raw.get("kind") not in ("kestrel-review", "kestrel-review-set"):
        raise ValueError(f"not a review: kind {raw.get('kind')!r} v{raw.get('version')}")
    recs = raw["reviews"] if raw["kind"] == "kestrel-review-set" else [raw]
    return [{k: r.get(k, "") for k in ("unit", "hash", "decision", "note", "at")} for r in recs]


def merge(records, units) -> dict[str, dict]:
    """unit id -> its latest decision; unknown units and decisions are refused."""
    known = {u["id"] for u in units}
    out = {}
    for r in records:
        if r["unit"] not in known:
            raise ValueError(f"unknown unit {r['unit']!r}")
        if r["decision"] not in DECISIONS:
            raise ValueError(f"decision {r['decision']!r} for {r['unit']!r} is not one of {DECISIONS}")
        if r["unit"] not in out or r["at"] >= out[r["unit"]]["at"]:
            out[r["unit"]] = {k: r[k] for k in ("hash", "decision", "note", "at")}
    return dict(sorted(out.items()))


def status(decisions: dict, units) -> dict[str, dict]:
    """Per dataset: units, ok, fix, stale (decided on other content), missing, ready."""
    out = {}
    for u in units:
        s = out.setdefault(u["ds"], {"units": 0, "ok": 0, "fix": 0, "stale": 0, "missing": 0})
        s["units"] += 1
        d = decisions.get(u["id"])
        s["missing" if d is None else "stale" if d["hash"] != u["hash"] else d["decision"]] += 1
    for s in out.values():
        s["ready"] = s["ok"] == s["units"]
    return out


def _files(path: Path):
    files = [path] if path.is_file() else sorted(p for p in path.rglob("*") if p.suffix in (".json", ".txt"))
    for f in files:
        yield f, f.read_text(encoding="utf-8")


def ingest(sources, units, log: Path = LOG) -> dict:
    records = []
    if log.exists():
        records += [{"unit": k, **v} for k, v in json.loads(log.read_text())["decisions"].items()]
    for src in sources:
        for f, text in _files(Path(src)):
            try:
                records += parse(text)
            except (ValueError, KeyError) as e:
                raise ValueError(f"{f.name}: {e}") from e
    decisions = merge(records, units)
    current = {u["id"]: u["hash"] for u in units}
    out = {"version": 1, "decisions": decisions, "status": status(decisions, units),
           "fixes": [{"unit": k, "note": d["note"]} for k, d in decisions.items()
                     if d["decision"] == "fix" and d["hash"] == current[k]]}
    log.parent.mkdir(parents=True, exist_ok=True)
    log.write_text(json.dumps(out, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    return out


def report(out: dict) -> str:
    lines = []
    for ds, s in out["status"].items():
        state = "ready to freeze" if s["ready"] else "not ready"
        lines.append(f"{TITLES[ds]}: {s['ok']}/{s['units']} ok, {s['fix']} to fix, {s['stale']} changed since "
                     f"reviewed, {s['missing']} not reviewed → {state}")
    lines += [f"  fix {f['unit']}: {f['note'] or '(no note)'}" for f in out["fixes"]]
    return "\n".join(lines)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("sources", nargs="*", help="exported store documents or saved 'Copy review' text")
    ap.add_argument("--status", action="store_true", help="only report, from the log")
    args = ap.parse_args(argv)
    units = all_units()
    if args.status:
        if not LOG.exists():
            sys.exit(f"no review log yet ({LOG.relative_to(ROOT)})")
        decisions = json.loads(LOG.read_text())["decisions"]
        current = {u["id"]: u["hash"] for u in units}
        out = {"status": status({k: v for k, v in decisions.items() if k in current}, units),
               "fixes": [{"unit": k, "note": d["note"]} for k, d in decisions.items()
                         if d["decision"] == "fix" and current.get(k) == d["hash"]]}
    else:
        if not args.sources:
            ap.error("give export directories or files, or --status")
        out = ingest(args.sources, units)
    print(report(out))


if __name__ == "__main__":
    main()
