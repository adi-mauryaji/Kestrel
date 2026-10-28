"""experiments/analysis/paper_assets_v3.py -- v3's paper, report and slides numbers, tables and figures, from the
committed logs.

    python experiments/analysis/paper_assets_v3.py   # -> paper/v3/generated/*, paper/v3/figures/*.pdf and *.png

Every number the v3 paper and report state is a macro `\\kv{key}` from paper/v3/generated/numbers.tex, and every
number the slides state is `{{key}}`, filled from paper/v3/generated/numbers.json by paper/v3/slides/build.py. The
values come from experiments/logs/v3 (kind decision, notes by meaning, notes Q&A, code loop, writer language, mmap,
worker start, acceptance), the dataset manifests, and E0's throughput sessions. Where a log keeps the per-item rows,
the summary it logged is counted again from them and must agree (`_check`). A few values exist only in a log's
README (the worker's RssAnon at ready, its prefix length, the acceptance test counts); they are read from there.

Shares come with 95% Wilson intervals, computed here (the logs keep counts). Synthetic data is labelled as such
in the paper; nothing in these sets is the user's own data.
"""
from __future__ import annotations

import csv
import json
import math
import re
import statistics
import sys
import tomllib
from decimal import ROUND_HALF_UP, Decimal
from fractions import Fraction
from pathlib import Path

from paper_assets import BLUE, GRID, INK, MUTED, ORANGE, _style, value_lines

ROOT = Path(__file__).resolve().parents[2]
LOGS = ROOT / "experiments" / "logs"
V3 = LOGS / "v3"
DATA = ROOT / "datasets"
OUT = ROOT / "paper" / "v3" / "generated"
FIGS = ROOT / "paper" / "v3" / "figures"
AQUA, YELLOW = "#1baf7a", "#eda100"  # dataviz reference palette, slots 3 and 4
SKILLS = ("chat", "code", "email", "notes", "rewrite", "translate")
OUTCOMES = (("right", "right"), ("asked", "asked first"), ("skill_sent_old_way", "sent the old way"),
            ("skill_not_asked_for", "a skill not asked for"))


def _load(rel: str):
    return json.loads((V3 / rel).read_text())


def _check(logged, counted, what: str):
    """A logged summary must equal the same value counted again from the log's own rows."""
    if logged != counted:
        raise ValueError(f"{what}: logged {logged}, counted from the rows {counted}")


def frac(k: int, n: int) -> str:
    return f"{k}/{n}"


def pct(k: int, n: int, nd: int = 1) -> str:
    """k of n as a percentage, rounded half up from the exact fraction (78/96 is 81.3%, not float's 81.2%)."""
    exact = Fraction(100 * k, n)
    q = Decimal(exact.numerator) / Decimal(exact.denominator)
    return f"{q.quantize(Decimal(1).scaleb(-nd), rounding=ROUND_HALF_UP)}%"


def fmt(x: float, nd: int) -> str:
    """x rounded half up to nd decimals, from its shortest repr (0.625 -> "0.63", where float formatting gives
    "0.62")."""
    return str(Decimal(repr(x)).quantize(Decimal(1).scaleb(-nd), rounding=ROUND_HALF_UP))


def wilson(k: int, n: int, z: float = 1.959964) -> tuple[float, float]:
    """95% Wilson score interval for k successes in n."""
    p = k / n
    centre = (p + z * z / (2 * n)) / (1 + z * z / n)
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return max(0.0, centre - half), min(1.0, centre + half)


def wci(k: int, n: int) -> str:
    lo, hi = wilson(k, n)
    return f"[{100 * lo:.1f}%, {100 * hi:.1f}%]"


def share(n: dict, key: str, k: int, total: int):
    """key = "k/n", key.pct = its share, key.ci = the share's 95% Wilson interval."""
    n[key], n[f"{key}.pct"], n[f"{key}.ci"] = frac(k, total), pct(k, total), wci(k, total)


def _md_table(path: Path) -> list[dict[str, str]]:
    """The rows of the first Markdown table in a file, as dicts keyed by its header."""
    rows = [line for line in path.read_text().splitlines() if line.startswith("|")]
    head = [c.strip() for c in rows[0].strip("|").split("|")]
    return [dict(zip(head, (c.strip() for c in r.strip("|").split("|")))) for r in rows[2:]]


# --- E0: model throughput on this laptop (context) -----------------------------------------------------------------
def e0(n: dict):
    sys.path.insert(0, str(ROOT / "experiments" / "analysis"))
    from e0_summary import across_sessions, per_session
    sessions = []
    for s in ("20261011T101743Z", "20261017T132649Z", "20261023T163212Z"):  # the three sessions of the v1 paper
        with open(LOGS / "E0" / f"{s}.csv", newline="") as f:
            sessions.append(per_session(list(csv.DictReader(f))))
    a = across_sessions(sessions)
    for short, model in (("gemma", "gemma-4-e2b-it-q4_0"), ("qsmall", "qwen3-1.7b-q4_k_m"),
                         ("qbig", "qwen3-4b-q4_k_m")):
        n[f"e0.{short}.tg"] = fmt(a[(model, 'tg64')]['mean'], 1)
        n[f"e0.{short}.pp"] = fmt(a[(model, 'pp128')]['mean'], 0)


def machine(n: dict):
    """The laptop (PREREG §4) and the encoder's own latency on it (E1, the encoder-alone system R1)."""
    m = re.search(r"Machine: (Ryzen [^(]+) \((\d+)C/(\d+)T\), ([\d.]+) GiB RAM", (ROOT / "experiments" / "PREREG.md").read_text())
    n["hw.cpu"], n["hw.cores"], n["hw.threads"], n["hw.ram"] = m.group(1).strip(), m.group(2), m.group(3), m.group(4)
    e1 = json.loads((LOGS / "E1" / "analysis_intents_v0_test.json").read_text())
    n["e1.encoder.median"] = fmt(e1['systems']['R1']['latency_ms']['median'], 1)


# --- datasets ------------------------------------------------------------------------------------------------------
def datasets(n: dict):
    for short, d in (("kv0", "kestrel_kinds_v0"), ("kv1", "kestrel_kinds_v1"), ("kv2", "kestrel_kinds_v2")):
        m = json.loads((DATA / d / "splits" / "manifest.json").read_text())
        n[f"{short}.labels"] = str(len(m["labels"]))
        n[f"{short}.status"] = m["status"].replace("-", " ")
        for split, v in m["splits"].items():
            n[f"{short}.{split}"] = str(v["items"])
            if "fresh_items" in v:
                n[f"{short}.{split}.fresh"] = str(v["fresh_items"])
        n[f"{short}.items"] = str(sum(v["items"] for v in m["splits"].values()))
    m = json.loads((DATA / "kestrel_notesq_v0" / "splits" / "manifest.json").read_text())
    notes = sorted(p for p in (DATA / "kestrel_notesq_v0" / "notes").rglob("*") if p.is_file())
    n["notes.fixture"] = str(len(notes))
    for split, v in m["splits"].items():
        n[f"notesq.{split}.q"], n[f"notesq.{split}.none"] = str(v["queries"]), str(v["none_queries"])
        n[f"notesq.{split}.fam"] = str(v["families"])
        n[f"notesq.{split}.found"] = str(v["queries"] - v["none_queries"])
    m = json.loads((DATA / "kestrel_notesqa_v0" / "splits" / "manifest.json").read_text())
    for split, v in m["splits"].items():
        n[f"notesqa.{split}.q"], n[f"notesqa.{split}.un"] = str(v["questions"]), str(v["unanswerable"])
        n[f"notesqa.{split}.ans"] = str(v["questions"] - v["unanswerable"])
        n[f"notesqa.{split}.fam"] = str(v["families"])
    m = json.loads((DATA / "kestrel_codetasks_v0" / "splits" / "manifest.json").read_text())
    n["code.status"] = m["status"].replace("-", " ")
    for split, v in m["splits"].items():
        n[f"code.{split}.n"], n[f"code.{split}.fam"] = str(v["requests"]), str(v["families"])
        n[f"code.{split}.pertype"] = str(v["by_type"]["function"])
        _check(set(v["by_type"].values()), {v["by_type"]["function"]}, f"CodeTasks-v0 {split} types are balanced")
    fams = tomllib.loads((DATA / "kestrel_kinds_v2" / "families.toml").read_text())["family"]
    n["kv2.fam.n"] = str(len(fams))
    for kind in sorted({f["kind"] for f in fams}):
        n[f"kv2.fam.{kind}"] = str(sum(f["kind"] == kind for f in fams))
    tasks = tomllib.loads((DATA / "kestrel_codetasks_v0" / "tasks.toml").read_text())
    n["code.families"] = str(len(tasks["family"]))


# --- the kind decision ---------------------------------------------------------------------------------------------
def _skill_outcomes(by_gold: dict, policy: str) -> dict[str, int]:
    """The decisions on requests for a skill (every gold kind but app), summed over the kinds."""
    out = {k: 0 for k in ("n", "right", "asked", "skill_sent_old_way", "skill_not_asked_for")}
    for kind, v in by_gold.items():
        if kind != "app":
            for k in out:
                out[k] += v[policy][k]
    return out


def kinds(n: dict):
    for v in ("1", "2"):
        f = _load(f"kinds/final_kinds_v{v}.json")
        p = f"kh{v}"
        n[f"{p}.c"], n[f"{p}.T"], n[f"{p}.folds"] = f"{f['c']:g}", fmt(f['temperature'], 2), str(f["folds"])
        n[f"{p}.oof.acc"] = fmt(f['oof']['accuracy'], 3)
        n[f"{p}.val.n"] = str(f["val_calibrated"]["n"])
        n[f"{p}.val.acc"], n[f"{p}.val.f1"] = fmt(f['val_calibrated']['accuracy'], 3), fmt(f['val_calibrated']['macro_f1'], 3)
        n[f"{p}.val.ece.raw"], n[f"{p}.val.ece"] = fmt(f['val_raw']['ece'], 3), fmt(f['val_calibrated']['ece'], 3)
        d = _load(f"kinds/fallback_dev_v{v}.json")
        n[f"fb{v}.T"] = fmt(d['temperature_gemma'], 2)
        n[f"fb{v}.ms.p50"], n[f"fb{v}.ms.p95"] = fmt(d['gemma_ms']['p50'], 0), fmt(d['gemma_ms']['p95'], 0)
        n[f"fb{v}.val.gemma.acc"] = fmt(d['val_gemma_accuracy'], 3)
        chosen = d["chosen"]
        # Validation items below the chosen tau, from the per-item outputs (the log's val_head_unsure is at the
        # draft tau 0.7): how often the head's top kind and Gemma's label were right there.
        out = [json.loads(line) for line in (V3 / "kinds" / f"fallback_outputs_v{v}.jsonl").read_text().splitlines()]
        unsure = [r for r in out if r["split"] == "val" and max(r["head"].values()) < chosen["tau"]]
        if chosen["tau"] == d["tau"]:
            u = d["val_head_unsure"]
            _check((u["n"], u["head_right"], u["gemma_right"]),
                   (len(unsure), sum(max(r["head"], key=r["head"].get) == r["gold"] for r in unsure),
                    sum(r["gemma"]["label"] == r["gold"] for r in unsure)), f"Kinds-v{v} unsure items")
        n[f"fb{v}.unsure.n"] = str(len(unsure))
        n[f"fb{v}.unsure.head"] = str(sum(max(r["head"], key=r["head"].get) == r["gold"] for r in unsure))
        n[f"fb{v}.unsure.gemma"] = str(sum(r["gemma"]["label"] == r["gold"] for r in unsure))
        n[f"fb{v}.tau"], n[f"fb{v}.taug"] = fmt(chosen['tau'], 1), fmt(chosen['tau_g'], 1)
        val = d["val_grid"][f"{chosen['tau']}/{chosen['tau_g']}"]
        n[f"fb{v}.val.right"] = frac(val["right"], val["n"])
        t = _load(f"kinds/fallback_test_v{v}.json")
        _check((t["tau"], t["tau_g"]), (chosen["tau"], chosen["tau_g"]), f"Kinds-v{v} test ran the chosen tau, tau_g")
        p = f"kt{v}"
        n[f"{p}.n"] = str(t["n"])
        n[f"{p}.head.acc"], n[f"{p}.head.f1"] = fmt(t['head']['accuracy'], 3), fmt(t['head']['macro_f1'], 3)
        n[f"{p}.head.ece"] = fmt(t['head']['ece'], 3)
        n[f"{p}.gemma.acc"] = fmt(t['gemma_accuracy'], 3)
        for pol, short in (("head_alone", "ha"), ("head_gemma", "hg")):
            a = t["all"][pol]
            _check(a["taken"], a["right"] + a["skill_not_asked_for"] + a["skill_sent_old_way"],
                   f"Kinds-v{v} {pol}: a decision taken is right, a skill not asked for, or sent the old way")
            _check(a["n"], a["taken"] + a["asked"], f"Kinds-v{v} {pol}: every request is taken or asked")
            share(n, f"{p}.{short}.right", a["right"], a["n"])
            for k in ("asked", "skill_sent_old_way", "skill_not_asked_for"):
                n[f"{p}.{short}.{k.replace('skill_', '').replace('_', '')}"] = str(a[k])
            s = _skill_outcomes(t["by_gold"], pol)
            _check(s["n"], t["n"] - t["by_gold"]["app"][pol]["n"], f"Kinds-v{v} skill requests")
            share(n, f"{p}.{short}.skill", s["right"], s["n"])
            app = t["by_gold"]["app"][pol]
            share(n, f"{p}.{short}.app", app["right"], app["n"])
            for lang, short_lang in (("en", "en"), ("hi-en", "hi")):
                b = t["by_lang"][lang][pol]
                share(n, f"{p}.{short}.{short_lang}", b["right"], b["n"])
            for kind in t["by_gold"]:
                g = t["by_gold"][kind][pol]
                n[f"{p}.{short}.kind.{kind}"] = frac(g["right"], g["n"])
        _check(len(t["wrong"]), t["all"]["head_gemma"]["n"] - t["all"]["head_gemma"]["right"] - t["all"]["head_gemma"]["asked"],
               f"Kinds-v{v}: the wrong decisions listed")
        for kind, r in t["head"]["per_class"].items():
            n[f"{p}.head.recall.{kind}"] = fmt(r['recall'], 2)
    t = _load("kinds/fallback_test_v2.json")
    for part in ("fresh", "seen"):
        for pol, short in (("head_alone", "ha"), ("head_gemma", "hg")):
            a = t[part][pol]
            share(n, f"kt2.{part}.{short}.right", a["right"], a["n"])
            n[f"kt2.{part}.{short}.asked"] = str(a["asked"])
            n[f"kt2.{part}.{short}.wrongskill"] = str(a["skill_not_asked_for"])
    _check(int(n["kt2.n"]) - int(n["kv2.test.fresh"]), int(n["kv1.test"]), "Kinds-v2 test = Kinds-v1 test + fresh")


# --- notes by meaning and notes Q&A --------------------------------------------------------------------------------
def notes(n: dict):
    for name, short in (("meaning_dev", "md"), ("meaning_dev_translated", "mdt"), ("meaning_test_translated", "mtt")):
        m = _load(f"notes/{name}.json")
        qs, theta = m["queries"], m["theta"]
        want = [q for q in qs if q["notes"]]
        none = [q for q in qs if not q["notes"]]
        # No query's keywords found its note, so the policy adds the closest note by meaning when it reaches theta.
        _check(sum(bool(q["keyword"]) for q in qs), 0, f"{name}: keyword hits")
        found = [q for q in want if q["best"][0] in q["notes"] and q["best"][1] >= theta]
        noise = [q for q in none if q["best"][1] >= theta]
        rank1 = [q for q in want if q["note_rank"] == 1]
        _check(round(m["found_and_noise_at_theta"][0] * len(want)), len(found), f"{name}: found at theta")
        _check(round(m["found_and_noise_at_theta"][1] * len(none)), len(noise), f"{name}: noise at theta")
        _check(round(m["meaning_rank1"] * len(want)), len(rank1), f"{name}: closest note is the right one")
        n[f"{short}.theta"] = fmt(theta, 2)
        share(n, f"{short}.found", len(found), len(want))
        n[f"{short}.noise"] = frac(len(noise), len(none))
        share(n, f"{short}.rank1", len(rank1), len(want))
        for lang, sl in (("en", "en"), ("hi-en", "hi")):
            w = [q for q in want if f"/{lang}/" in q["id"]]
            share(n, f"{short}.found.{sl}", sum(q in found for q in w), len(w))
            share(n, f"{short}.rank1.{sl}", sum(q in rank1 for q in w), len(w))
        n[f"{short}.chunks"] = str(m["chunks"])
    for split in ("dev", "test"):
        q = _load(f"notes/qa_{split}.json")
        rows = q["questions"]
        un_ids = {r["id"] for r in rows if r["id"].startswith("qa-none")}  # NotesQA-v0's unanswerable families
        a = q["at_theta"]["answerable"]
        u = q["at_theta"]["unanswerable"]
        _check(a["right"] + a["wrong"] + a.get("abstained", 0) + u["right"] + u["wrong"], len(rows), f"qa {split}: graded")
        _check(sum(r["grade"] == "right" for r in rows if r["id"] not in un_ids), a["right"], f"qa {split}: answerable right")
        _check(sum(r["grade"] == "right" for r in rows if r["id"] in un_ids), u["right"], f"qa {split}: unanswerable right")
        share(n, f"qa.{split}.ans", a["right"], a["right"] + a["wrong"] + a.get("abstained", 0))
        share(n, f"qa.{split}.un", u["right"], u["right"] + u["wrong"])
        share(n, f"qa.{split}.all", a["right"] + u["right"], len(rows))
        # From the per-question times: the logged mean is itself rounded (4.95), and rounding it again gives 5.0
        # where the times average 4.947.
        mean = statistics.fmean(r["s"] for r in rows)
        _check(round(mean, 2), q["answer_s"]["mean"], f"qa {split}: mean seconds")
        n[f"qa.{split}.s.mean"], n[f"qa.{split}.s.max"] = fmt(mean, 1), fmt(max(r["s"] for r in rows), 1)
        for lang, sl in (("en", "en"), ("hi-en", "hi")):
            w = [r for r in rows if f"/{lang}/" in r["id"]]
            share(n, f"qa.{split}.all.{sl}", sum(r["grade"] == "right" for r in w), len(w))
    lang = _load("notes/qa_dev.json")["answer_language"]["hi-en"]
    share(n, "qa.dev.hinglish", lang["same_language"], lang["answered"])
    # The test run (177122b) came before the Hinglish answer note (e12cb18); its answers' language, counted here
    # with the dev run's own check (writer_lang.is_hinglish) on the Hinglish questions that got an answer.
    sys.path.insert(0, str(ROOT / "experiments" / "v3"))
    from writer_lang import is_hinglish
    hi = [r for r in _load("notes/qa_test.json")["questions"]
          if "/hi-en/" in r["id"] and r["answer"] != "NOT IN NOTES"]
    share(n, "qa.test.hinglish", sum(is_hinglish(r["answer"]) for r in hi), len(hi))


# --- the code loop -------------------------------------------------------------------------------------------------
def code(n: dict):
    runs = (("test", "test_gemma-4-e2b.json"), ("devfinal", "dev_gemma-4-e2b_adr044.json"),
            ("dev.gemma", "dev_gemma-4-e2b.json"), ("dev.qsmall", "dev_qwen3-1.7b.json"),
            ("dev.qbig", "dev_qwen3-4b.json"))
    for short, name in runs:
        c = _load(f"code/{name}")
        rows = c["results"]
        for part, v in c["summary"].items():
            if not isinstance(v, dict):
                continue
            mine = [r for r in rows if part == "all" or r["type"] == part or r["lang"] == part]
            _check(v["pass1"], sum(r["pass1"] for r in mine), f"{name} {part}: pass@1")
            _check(v["pass_loop"], sum(r["pass_loop"] for r in mine), f"{name} {part}: after the loop")
            _check(v["attempts"], sum(len(r["attempts"]) for r in mine), f"{name} {part}: attempts")
            key = "en" if part == "en" else "hi" if part == "hi-en" else part
            share(n, f"code.{short}.{key}", v["pass_loop"], v["n"])
            n[f"code.{short}.{key}.p1"] = frac(v["pass1"], v["n"])
        a = c["summary"]["all"]
        n[f"code.{short}.retries"] = str(a["attempts"] - a["n"])
        n[f"code.{short}.s"] = fmt(a['seconds'] / a['n'], 1)
        n[f"code.{short}.loopgain"] = str(a["pass_loop"] - a["pass1"])
        ms = [x["ms"] for r in rows for x in r["attempts"]]
        n[f"code.{short}.ms.median"] = fmt(statistics.median(ms) / 1000, 1)
        n[f"code.{short}.s.max"] = fmt(max(sum(x['ms'] for x in r['attempts']) for r in rows) / 1000, 1)
        ex = [r for r in rows if r["type"] == "explain"]
        n[f"code.{short}.explain.hi.hinglish"] = frac(sum(bool(r["hinglish"]) for r in ex if r["lang"] == "hi-en"),
                                                     sum(r["lang"] == "hi-en" for r in ex))
        # English explain requests answered in English (the prompt asks for the request's language).
        n[f"code.{short}.explain.en.english"] = frac(sum(not r["hinglish"] for r in ex if r["lang"] == "en"),
                                                    sum(r["lang"] == "en" for r in ex))
        _check(n[f"code.{short}.explain.hi.hinglish"], c["summary"]["explain_hi_en_in_hinglish"], f"{name}: Hinglish")
    r = _load("code/dev_rescored_review.json")["models"]
    for short, model in (("gemma", "gemma-4-e2b"), ("qsmall", "qwen3-1.7b"), ("qbig", "qwen3-4b")):
        m = r[model]
        _check(m["strict_before"], int(n[f"code.dev.{short}.all"].split("/")[0]), f"{model}: strict dev score")
        n[f"code.resc.{short}"] = frac(m["rescored"], 24)
        n[f"code.resc.{short}.writefix"] = m["code_write_fix"]
        n[f"code.resc.{short}.script"] = m["script"]
        n[f"code.resc.{short}.s"] = fmt(m['gen_s_per_request'], 1)


# --- writer language (dev only) ------------------------------------------------------------------------------------
def writer(n: dict):
    for tag in ("baseline", "final"):
        w = json.loads((V3 / "writer" / f"lang_{tag}.json").read_text())
        _check(w["pass"], sum(i["pass"] for i in w["items"]), f"writer {tag}: pass")
        n[f"writer.{tag}"], n["writer.n"] = frac(w["pass"], w["n"]), str(w["n"])
        for g, v in w["groups"].items():
            n[f"writer.{tag}.{g}"] = frac(v["pass"], v["n"])


# --- the service: mmap, worker start -------------------------------------------------------------------------------
def systems(n: dict):
    rows = {(r["load"], r["cache"]): r for r in _md_table(V3 / "mmap" / "20261026T140055Z.md")}
    for (load, cache), r in rows.items():
        p = f"mmap.{load}.{cache}"
        n[f"{p}.load"], n[f"{p}.first"], n[f"{p}.tps"] = r["load ms"], r["first token ms"], r["decode tok/s"]
        n[f"{p}.anon"], n[f"{p}.file"] = r["anon MiB"], r["file MiB"]
    copy, mapped = int(rows[("copy", "cold")]["anon MiB"]), int(rows[("map", "cold")]["anon MiB"])
    _check(rows[("copy", "warm")]["anon MiB"], str(copy), "anon MiB does not depend on the page cache")
    n["mmap.anon.saved"] = str(copy - mapped)
    n["mmap.anon.cut"] = pct(copy - mapped, copy)
    firsts = [int(r["first token ms"]) for r in rows.values()]
    n["mmap.first.min"], n["mmap.first.max"] = str(min(firsts)), str(max(firsts))
    with open(V3 / "worker" / "20261026T142304Z.tsv", newline="") as f:
        tsv = list(csv.DictReader(f, delimiter="\t"))
    med = {}
    for binary, short in (("v2", "v2"), ("miss", "miss"), ("hit", "hit")):
        s = [float(r["seconds"]) for r in tsv if r["binary"] == binary]
        med[short] = statistics.median(s)
        n[f"worker.{short}.n"], n[f"worker.{short}.s"] = str(len(s)), fmt(statistics.median(s), 1)
        n[f"worker.{short}.min"], n[f"worker.{short}.max"] = fmt(min(s), 1), fmt(max(s), 1)
    readme = (V3 / "worker" / "README.md").read_text()
    n["worker.prefix.tokens"] = re.search(r"prompt prefix is \*\*(\d+) tokens", readme).group(1)
    table = _md_table(V3 / "worker" / "README.md")
    col = "RssAnon / RssFile at ready, MiB"
    n["worker.v2.anon"] = next(r for r in table if r["worker"].startswith("v2"))[col].split("/")[0].strip()
    n["worker.hit.anon"] = next(r for r in table if "cache saved" in r["worker"])[col].split("/")[0].strip()
    n["worker.speedup"] = fmt(med['v2'] / med['hit'], 1)


# --- acceptance ----------------------------------------------------------------------------------------------------
def acceptance(n: dict):
    s = _load("acceptance/socket.json")
    res = s["results"]
    _check(s["summary"]["passed"], sum(r["ok"] for r in res), "socket: passed")
    _check(s["summary"]["cases"], len(res), "socket: cases")
    share(n, "acc.socket", s["summary"]["passed"], s["summary"]["cases"])
    n["acc.socket.failed"] = str(len(s["summary"]["failed"]))
    n["acc.socket.minutes"] = fmt(s['summary']['seconds'] / 60, 1)
    first = _load("acceptance/socket_run1.json")["summary"]
    n["acc.run1"] = frac(first["passed"], first["cases"])
    for mode in ("headless", "headed"):
        p = _load(f"acceptance/page_{mode}.json")
        _check(p["passed"], sum(st["ok"] for st in p["steps"]), f"page {mode}: passed")
        n[f"acc.page.{mode}"] = frac(p["passed"], p["total"])
    by = {r["feature"]: r["s"] for r in res}
    for key, feature in (("task", "tasks: create with a time"), ("taskhi", "tasks: Hinglish with a time"),
                         ("search", "notes: keyword search"), ("meaning", "notes: by meaning"),
                         ("meaninghi", "notes: by meaning, Hinglish"), ("summary", "notes: summary"),
                         ("chat", "chat: explanation"), ("poem", "chat: Hinglish poem"),
                         ("emailhi", "email: Hinglish"), ("email", "email: English"),
                         ("rewrite", "rewrite: keeps English"), ("tohindi", "translate: into Hindi"),
                         ("toenglish", "translate: into English"), ("qa", "notes Q&A: Hinglish"),
                         ("qanone", "notes Q&A: not in the notes"), ("codefn", "code: write a function"),
                         ("codefix", "code: fix"), ("explain", "code: explain is chat"),
                         ("essay", "cut-off note")):
        n[f"acc.s.{key}"] = fmt(by[feature], 1)
    fast = [r for r in res if r["s"] < 1]
    share(n, "acc.fast", len(fast), len(res))
    n["acc.fast.median"] = fmt(statistics.median(r['s'] for r in fast), 1)
    readme = (V3 / "acceptance" / "README.md").read_text()
    n["acc.cargo"] = re.search(r"`cargo test --workspace` \| (\d+) passed", readme).group(1)
    n["acc.pytest"] = re.search(r"`pytest` \| (\d+) passed", readme).group(1)
    n["acc.bwrap"] = re.search(r"bubblewrap tests[^|]*\| (\d+) / \d+", readme).group(1)
    n["acc.worker"] = re.search(r"model worker tests[^|]*\| (\d+) / \d+", readme).group(1)
    m = re.search(r"a task for (\d\d:\d\d) made at (\d\d:\d\d:\d\d) \| rang (\d\d:\d\d:\d\d)", readme)
    n["acc.reminder.due"], n["acc.reminder.made"], n["acc.reminder.rang"] = m.groups()
    due, rang = (sum(int(x) * f for x, f in zip(t.split(":"), (3600, 60, 1))) for t in (m.group(1) + ":00", m.group(3)))
    n["acc.reminder.late"] = str(rang - due)
    n["acc.limits"] = str(len(re.findall(r"^\d+\. ", readme.split("## Failures and known limitations")[1], re.M)))


def numbers() -> dict[str, str]:
    """Every value, as plain text: "21/24", "87.5%", "[69.0%, 95.7%]", "0.83"."""
    n: dict[str, str] = {}
    for part in (e0, machine, datasets, kinds, notes, code, writer, systems, acceptance):
        part(n)
    return n


def tex_values(n: dict[str, str]) -> dict[str, str]:
    """The values as the paper typesets them: percent signs escaped, a clock time's colon an ordinary symbol
    (in math mode "17:31" would read "17 : 31")."""
    return {k: v.replace("%", "\\%").replace(":", "{:}") for k, v in n.items()}


# --- tables --------------------------------------------------------------------------------------------------------
def table_datasets(n: dict) -> str:
    rows = [
        ("Kestrel-Kinds-v1", "6 kinds", f"{n['kv1.train']} / {n['kv1.cal']} / {n['kv1.val']} / {n['kv1.test']}",
         n["kv1.status"]),
        ("Kestrel-Kinds-v2", "7 kinds (+ code)",
         f"{n['kv2.train']} / {n['kv2.cal']} / {n['kv2.val']} / {n['kv2.test']} ({n['kv2.test.fresh']} fresh)",
         n["kv2.status"]),
        ("Kestrel-NotesQ-v0", f"{n['notes.fixture']} fixture notes",
         f"dev {n['notesq.dev.q']} ({n['notesq.dev.none']} none) / test {n['notesq.test.q']} ({n['notesq.test.none']} none)",
         "reviewed"),
        ("Kestrel-NotesQA-v0", "same notes",
         f"dev {n['notesqa.dev.q']} ({n['notesqa.dev.un']} unans.) / test {n['notesqa.test.q']} ({n['notesqa.test.un']} unans.)",
         "reviewed"),
        ("Kestrel-CodeTasks-v0", "4 types",
         f"dev {n['code.dev.n']} / test {n['code.test.n']} ({n['code.families']} families)", n["code.status"]),
    ]
    body = "\n".join(f"{a} & {b} & {c} & {d} \\\\" for a, b, c, d in rows)
    return ("\\begin{tabular}{llll}\n\\toprule\nDataset & Labels / content & Items (train / cal / val / test) & "
            "Review \\\\\n\\midrule\n" + body + "\n\\bottomrule\n\\end{tabular}\n")


def table_kinds(n: dict) -> str:
    def row(name, p):
        return (f"{name} & {n[p + '.right']} ({n[p + '.right.pct']}) & {n[p + '.skill']} & {n[p + '.app']} & "
                f"{n[p + '.asked']} & {n[p + '.sentoldway']} & {n[p + '.notaskedfor']} \\\\")
    lines = [row("Kinds-v1, head alone", "kt1.ha"), row("Kinds-v1, head + Gemma", "kt1.hg"), "\\midrule",
             row("Kinds-v2, head alone", "kt2.ha"), row("Kinds-v2, head + Gemma", "kt2.hg")]
    return ("\\begin{tabular}{lrrrrrr}\n\\toprule\n & \\multicolumn{3}{c}{right} & & sent the & skill not \\\\\n"
            "\\cmidrule(lr){2-4}\nPolicy (test split) & all & skill req. & app req. & asked & old way & asked for "
            "\\\\\n\\midrule\n" + tex_escape_rows(lines) + "\n\\bottomrule\n\\end{tabular}\n")


def table_perkind(n: dict) -> str:
    """Right decisions per gold kind on the test splits (Kinds-v1 has no code kind)."""
    lines = []
    for kind in ("app",) + SKILLS:
        cells = [n.get(f"kt{v}.{pol}.kind.{kind}", "--") for v in ("1", "2") for pol in ("ha", "hg")]
        lines.append(f"{kind} & " + " & ".join(cells) + " \\\\")
    return ("\\begin{tabular}{lrrrr}\n\\toprule\n & \\multicolumn{2}{c}{Kinds-v1 test} & "
            "\\multicolumn{2}{c}{Kinds-v2 test} \\\\\n\\cmidrule(lr){2-3}\\cmidrule(lr){4-5}\n"
            "Gold kind & head alone & head + Gemma & head alone & head + Gemma \\\\\n\\midrule\n"
            + "\n".join(lines) + "\n\\bottomrule\n\\end{tabular}\n")


def tex_escape_rows(lines: list[str]) -> str:
    return "\n".join(line.replace("%", "\\%") for line in lines)


def table_code(n: dict) -> str:
    def row(name, p, s):
        return (f"{name} & {n[p + '.function']} & {n[p + '.script']} & {n[p + '.fix']} & {n[p + '.explain']} & "
                f"{n[p + '.all']} & {n[s]} \\\\")
    lines = [row("Gemma-4-E2B, first harness and checks (dev)", "code.dev.gemma", "code.dev.gemma.s"),
             row("Qwen3-1.7B, first harness and checks (dev)", "code.dev.qsmall", "code.dev.qsmall.s"),
             row("Qwen3-4B, first harness and checks (dev)", "code.dev.qbig", "code.dev.qbig.s"),
             "\\midrule",
             row("Gemma-4-E2B, final harness (dev)", "code.devfinal", "code.devfinal.s"),
             row("Gemma-4-E2B, final harness (\\textbf{test}, once)", "code.test", "code.test.s")]
    return ("\\begin{tabular}{lrrrrrr}\n\\toprule\nModel and run & function & script & fix & explain & all & "
            "gen.\\ s / request \\\\\n\\midrule\n" + "\n".join(lines) + "\n\\bottomrule\n\\end{tabular}\n")


def table_notes(n: dict) -> str:
    lines = [
        f"By meaning, no translation (dev, $\\theta={n['md.theta']}$) & {n['md.found']} & {n['md.found.en']} & "
        f"{n['md.found.hi']} & {n['md.noise']} \\\\",
        f"By meaning, Hinglish via English (dev, $\\theta={n['mdt.theta']}$) & {n['mdt.found']} & {n['mdt.found.en']} & "
        f"{n['mdt.found.hi']} & {n['mdt.noise']} \\\\",
        f"By meaning, Hinglish via English (\\textbf{{test}}, $\\theta={n['mtt.theta']}$) & {n['mtt.found']} & "
        f"{n['mtt.found.en']} & {n['mtt.found.hi']} & {n['mtt.noise']} \\\\",
    ]
    return ("\\begin{tabular}{lrrrr}\n\\toprule\nRetrieval run & right note added & English & Hinglish & "
            "note added to a query with none \\\\\n\\midrule\n" + "\n".join(lines) + "\n\\bottomrule\n\\end{tabular}\n")


def table_systems(n: dict) -> str:
    lines = []
    for load in ("copy", "map"):
        for cache in ("cold", "warm"):
            p = f"mmap.{load}.{cache}"
            lines.append(f"{'copied' if load == 'copy' else 'mapped'} & {cache} & {n[p + '.load']} & "
                         f"{n[p + '.first']} & {n[p + '.anon']} & {n[p + '.file']} \\\\")
    return ("\\begin{tabular}{llrrrr}\n\\toprule\nWeights & page cache & load (ms) & first token (ms) & "
            "anonymous (MiB) & file-backed (MiB) \\\\\n\\midrule\n" + "\n".join(lines) +
            "\n\\bottomrule\n\\end{tabular}\n")


# --- figures -------------------------------------------------------------------------------------------------------
def _save(fig, name: str):
    FIGS.mkdir(parents=True, exist_ok=True)
    fig.savefig(FIGS / f"{name}.pdf")
    fig.savefig(FIGS / f"{name}.png")


def fig_kinds(plt):
    """Decisions on the requests for a skill, by policy: right, asked first, sent the old way, a wrong skill."""
    bars = []
    for v in ("1", "2"):
        t = _load(f"kinds/fallback_test_v{v}.json")
        for pol, name in (("head_alone", "head alone"), ("head_gemma", "head + Gemma")):
            bars.append((f"Kinds-v{v} test, {name}", _skill_outcomes(t["by_gold"], pol)))
    colors = (BLUE, ORANGE, AQUA, YELLOW)
    fig, ax = plt.subplots(figsize=(6.6, 2.1))
    ax.set_axisbelow(True)
    for i, (label, s) in enumerate(reversed(bars)):
        left = 0.0
        for (key, name), color in zip(OUTCOMES, colors):
            w = s[key] / s["n"]
            ax.barh(i, w, left=left, color=color, height=0.62, edgecolor="white", linewidth=1.2,
                    label=name if i == 0 else None)
            if w >= 0.06:
                ax.text(left + w / 2, i, str(s[key]), ha="center", va="center", fontsize=7.5,
                        color="white" if color in (BLUE, ORANGE) else INK)
            left += w
        ax.text(1.01, i, f"n = {s['n']}", va="center", fontsize=7.5, color=MUTED)
    ax.set_yticks(range(len(bars)), [b[0] for b in reversed(bars)])
    ax.set_xlim(0, 1)
    ax.set_xticks([0, 0.25, 0.5, 0.75, 1], ["0", "25%", "50%", "75%", "100%"])
    ax.grid(axis="y", visible=False)
    ax.legend(loc="lower center", bbox_to_anchor=(0.4, 1.0), ncol=4, fontsize=7.5, handlelength=1.2)
    _save(fig, "kinds")
    plt.close(fig)


def fig_code(plt):
    """CodeTasks-v0 by request type: Gemma's final-harness dev run and its one test run."""
    types = ("function", "script", "fix", "explain")
    runs = (("dev_gemma-4-e2b_adr044.json", "dev (final harness)", BLUE), ("test_gemma-4-e2b.json", "test (once)", ORANGE))
    fig, ax = plt.subplots(figsize=(4.4, 2.0))
    ax.set_axisbelow(True)
    for j, (name, label, color) in enumerate(runs):
        s = _load(f"code/{name}")["summary"]
        vals = [s[t]["pass_loop"] for t in types]
        xs = [i + (j - 0.5) * 0.36 for i in range(len(types))]
        ax.bar(xs, vals, width=0.34, color=color, label=label)
        for x, v, t in zip(xs, vals, types):
            ax.annotate(f"{v}/{s[t]['n']}", (x, v), textcoords="offset points", xytext=(0, 2), ha="center", fontsize=7)
    ax.set_xticks(range(len(types)), types)
    ax.set_ylim(0, 7.2)
    ax.set_ylabel("requests right (of 6)")
    ax.grid(axis="x", visible=False)
    ax.legend(loc="lower center", bbox_to_anchor=(0.5, 1.0), ncol=2, fontsize=7.5)
    _save(fig, "code")
    plt.close(fig)


def fig_memory(plt):
    """The Gemma worker: memory copied vs mapped (left), start-up time (right). Two panels, two units."""
    rows = {(r["load"], r["cache"]): r for r in _md_table(V3 / "mmap" / "20261026T140055Z.md")}
    with open(V3 / "worker" / "20261026T142304Z.tsv", newline="") as f:
        tsv = list(csv.DictReader(f, delimiter="\t"))
    fig, (a, b) = plt.subplots(1, 2, figsize=(6.6, 2.0), gridspec_kw={"width_ratios": [1, 1.1], "wspace": 0.75})
    a.set_axisbelow(True)
    b.set_axisbelow(True)
    for i, load in enumerate(("copy", "map")):
        anon, file = int(rows[(load, "cold")]["anon MiB"]), int(rows[(load, "cold")]["file MiB"])
        a.bar(i, anon, color=BLUE, width=0.55, label="anonymous (cannot be reclaimed)" if i == 0 else None)
        a.bar(i, file, bottom=anon, color=GRID, width=0.55, edgecolor=MUTED, linewidth=0.5,
              label="file-backed (clean, reclaimable)" if i == 0 else None)
        a.annotate(f"{anon}", (i, anon / 2), ha="center", va="center", fontsize=7.5, color="white")
    a.set_xticks([0, 1], ["copied", "mapped (mmap)"])
    a.set_ylabel("MiB at the end of a run")
    a.set_ylim(0, 4000)
    a.grid(axis="x", visible=False)
    a.legend(loc="lower center", bbox_to_anchor=(0.5, 1.0), ncol=1, fontsize=6.8)
    names = (("v2", "v2"), ("miss", "v3, computed"), ("hit", "v3, loaded"))
    meds = [statistics.median(float(r["seconds"]) for r in tsv if r["binary"] == k) for k, _ in names]
    b.barh(range(3), meds, color=[MUTED, MUTED, ORANGE], height=0.55)
    for i, v in enumerate(meds):
        b.annotate(f"{v:.1f} s", (v, i), textcoords="offset points", xytext=(3, 0), va="center", fontsize=7.5)
    b.set_yticks(range(3), [x[1] for x in names])
    b.invert_yaxis()
    b.set_xlim(0, 23)
    b.set_xlabel("seconds to ready (median of 3)")
    b.set_title("prompt prefix", fontsize=7.5, color=MUTED, loc="left")
    b.grid(axis="y", visible=False)
    _save(fig, "memory")
    plt.close(fig)


def main() -> int:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    _style(plt)
    OUT.mkdir(parents=True, exist_ok=True)
    n = numbers()
    (OUT / "numbers.tex").write_text("% Generated by experiments/analysis/paper_assets_v3.py from the committed logs.\n"
                                     "\\makeatletter\n" + "\n".join(value_lines(tex_values(n))) + "\n\\makeatother\n")
    (OUT / "numbers.json").write_text(json.dumps(n, indent=1, ensure_ascii=False) + "\n")
    tables = {"table_datasets": table_datasets(n), "table_kinds": table_kinds(n), "table_perkind": table_perkind(n), "table_code": table_code(n),
              "table_notes": table_notes(n), "table_systems": table_systems(n)}
    for name, text in tables.items():
        (OUT / f"{name}.tex").write_text("% Generated by experiments/analysis/paper_assets_v3.py.\n" + text)
    fig_kinds(plt)
    fig_code(plt)
    fig_memory(plt)
    print(f"{len(n)} numbers, {len(tables)} tables, 3 figures -> paper/v3/generated, paper/v3/figures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
