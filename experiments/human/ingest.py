"""experiments/human/ingest.py -- brings Kestrel Request Lab submissions into the repo, and κ.

Submissions arrive two ways:
- pasted text: a person tapped "Copy answers" and sent it (WhatsApp etc.); save each message as a
  .txt/.json file in an inbox folder. Text around the JSON is ignored.
- the page's store: export it with ArtifactData `list` + `out_dir` (collections `annotations` and
  `requests`); each exported document is {"id", "data": submission, ...}.

    python experiments/human/ingest.py INBOX_OR_EXPORT_DIR [...]   # validate + write files
    python experiments/human/ingest.py --kappa datasets/kestrel_intents_v0/annotation/annotator_AB.json

Every submission is validated (human_data.parse_submission) before anything is written; one bad
file stops the run and names the file. Later submissions with the same code replace earlier ones.
Outputs: datasets/kestrel_intents_v0/annotation/annotator_<code>.json and
datasets/kestrel_human_requests_v0/raw/requests_<code>.jsonl.
"""
from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

from human_data import ROOT, cohen_kappa, gold_kappa_label, load_intents_v0, parse_submission

ANNOTATION_DIR = ROOT / "datasets" / "kestrel_intents_v0" / "annotation"
REQUESTS_DIR = ROOT / "datasets" / "kestrel_human_requests_v0" / "raw"
KAPPA_LOG = ROOT / "experiments" / "logs" / "human"


def _json_in(text: str) -> str:
    start, end = text.find("{"), text.rfind("}")
    if start < 0 or end < start:
        raise ValueError("no JSON object")
    return text[start : end + 1]


def _submissions(path: Path):
    """(source file, submission JSON text) for every file under path."""
    for f in sorted(p for p in path.rglob("*") if p.suffix in (".json", ".txt") and p.is_file()):
        raw = json.loads(_json_in(f.read_text(encoding="utf-8")))
        if isinstance(raw.get("data"), dict) and "kind" not in raw:  # an exported store document
            raw = raw["data"]
        yield f, json.dumps(raw)


def ingest(sources, out: Path | None = None) -> list[Path]:
    ids = {r["id"] for r in load_intents_v0()}
    parsed = []
    for src in sources:
        for f, text in _submissions(Path(src)):
            try:
                parsed.append(parse_submission(text, ids))
            except (ValueError, KeyError) as e:
                raise ValueError(f"{f.name}: {e}") from e
    ann_dir = Path(out) if out else ANNOTATION_DIR
    req_dir = Path(out) if out else REQUESTS_DIR
    written = []
    for sub in parsed:
        if sub["kind"] == "kestrel-annotation":
            ann_dir.mkdir(parents=True, exist_ok=True)
            p = ann_dir / f"annotator_{sub['code']}.json"
            p.write_text(json.dumps({"code": sub["code"], "n": len(sub["answers"]),
                                     "answers": dict(sorted(sub["answers"].items()))}, indent=1) + "\n")
        else:
            req_dir.mkdir(parents=True, exist_ok=True)
            p = req_dir / f"requests_{sub['code']}.jsonl"
            p.write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in sub["requests"]))
        if p not in written:
            written.append(p)
    return written


def kappa_report(annotation: dict) -> dict:
    """Cohen's κ of one annotator against the gold labels, overall and per language."""
    rows = {r["id"]: r for r in load_intents_v0()}
    items = [i for i in annotation["answers"] if i in rows]
    gold = [gold_kappa_label(rows[i]) for i in items]
    theirs = [annotation["answers"][i] for i in items]
    out = {"code": annotation["code"], "n": len(items),
           "agreement": sum(g == t for g, t in zip(gold, theirs)) / len(items),
           "kappa": cohen_kappa(gold, theirs), "by_lang": {}, "disagreements": defaultdict(int)}
    for lang in sorted({rows[i]["lang"] for i in items}):
        idx = [k for k, i in enumerate(items) if rows[i]["lang"] == lang]
        out["by_lang"][lang] = {"n": len(idx), "kappa": cohen_kappa([gold[k] for k in idx], [theirs[k] for k in idx])}
    for g, t in zip(gold, theirs):
        if g != t:
            out["disagreements"][f"{g} -> {t}"] += 1
    out["disagreements"] = dict(sorted(out["disagreements"].items(), key=lambda kv: -kv[1]))
    return out


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("sources", nargs="*", type=Path)
    ap.add_argument("--kappa", type=Path, help="an annotator_<code>.json to score against the gold labels")
    args = ap.parse_args(argv)
    for p in ingest(args.sources) if args.sources else []:
        print("wrote", p.relative_to(ROOT))
    if args.kappa:
        rep = kappa_report(json.loads(args.kappa.read_text()))
        KAPPA_LOG.mkdir(parents=True, exist_ok=True)
        log = KAPPA_LOG / f"kappa_{rep['code']}.json"
        log.write_text(json.dumps(rep, indent=1) + "\n")
        print(json.dumps(rep, indent=1))
        print("wrote", log.relative_to(ROOT))


if __name__ == "__main__":
    sys.exit(main())
