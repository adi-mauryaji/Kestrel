#!/usr/bin/env python3
"""The pilot's counts, added up (docs/v4/PILOT.md). A folder with one file per person, P01.json and so on,
each the text copied from Settings, "How Kestrel is used".

    python3 experiments/v4/pilot_summary.py FOLDER [--weeks 2] [--very K --of N]

--very and --of are the survey's: how many answered "very disappointed", of how many people. A file that
cannot be read is named, and the run exits 1 rather than quietly leaving someone out.
"""
import argparse
import json
import math
import statistics
import sys
from collections import Counter
from pathlib import Path

BAR = 0.40  # blueprint §1
# Things that happened, as against ways a request was answered.
EVENTS = {"request", "not_understood", "task_added", "document_added", "reminder_rang",
          "document_reminder_rang", "brief_rang", "weather_reminder_rang"}
RANG = ("reminder_rang", "document_reminder_rang", "brief_rang", "weather_reminder_rang")


def wilson(k: int, n: int, z: float = 1.959964) -> tuple[float, float]:
    """95% Wilson score interval for k of n (as experiments/analysis/paper_assets_v3.py)."""
    p = k / n
    centre = (p + z * z / (2 * n)) / (1 + z * z / n)
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return max(0.0, centre - half), min(1.0, centre + half)


def person(code: str, sent: dict, weeks: float) -> dict:
    """One person's numbers. KeyError or TypeError when `sent` is not the counts text."""
    total: Counter = Counter()
    for day in sent["days"]:
        total.update(day["counts"])
    return {
        "code": code,
        "os": sent["os"],
        "days_used": sent["days_used"],
        "days_per_week": sent["days_used"] / weeks,
        "requests": total["request"],
        "not_understood": total["not_understood"],
        "tasks": total["task_added"],
        "documents": total["document_added"],
        "reminders": sum(total[name] for name in RANG),
        "total": total,
    }


def read_folder(folder: Path, weeks: float) -> tuple[list[dict], list[str]]:
    """Everyone who could be read, and the names of the files that could not."""
    people, bad = [], []
    for path in sorted(folder.glob("*.json")):
        try:
            people.append(person(path.stem, json.loads(path.read_text()), weeks))
        except (ValueError, KeyError, TypeError, AttributeError, OSError):
            bad.append(path.name)
    return people, bad


def summarize(people: list[dict]) -> dict:
    total: Counter = Counter()
    for p in people:
        total.update(p["total"])
    ways = Counter({name: n for name, n in total.items() if name not in EVENTS})
    return {
        "people": len(people),
        "systems": dict(Counter(p["os"] for p in people)),
        "used_at_all": sum(1 for p in people if p["days_used"] > 0),
        "median_days_per_week": statistics.median(p["days_per_week"] for p in people),
        "requests": total["request"],
        "not_understood_share": total["not_understood"] / total["request"] if total["request"] else 0.0,
        "tasks": total["task_added"],
        "documents": total["document_added"],
        "reminders": sum(total[name] for name in RANG),
        "ways": sorted(ways.items(), key=lambda kv: (-kv[1], kv[0])),
    }


def render(people: list[dict], s: dict) -> list[str]:
    lines = ["| Person | System | Days used | Days/week | Requests | Not placed | Tasks | Documents | Reminders |",
             "|---|---|---|---|---|---|---|---|---|"]
    lines += [f"| {p['code']} | {p['os']} | {p['days_used']} | {p['days_per_week']:.1f} | {p['requests']} | "
              f"{p['not_understood']} | {p['tasks']} | {p['documents']} | {p['reminders']} |" for p in people]
    systems = ", ".join(f"{name} {n}" for name, n in sorted(s["systems"].items()))
    lines += ["",
              f"{s['people']} {'person' if s['people'] == 1 else 'people'} ({systems}), "
              f"{s['used_at_all']} used it at least once.",
              f"Median days used per week: {s['median_days_per_week']:.1f}.",
              f"Requests: {s['requests']}; nothing could place {100 * s['not_understood_share']:.0f}% of them.",
              f"Added: {s['tasks']} tasks, {s['documents']} documents. Reminders shown: {s['reminders']}."]
    if s["ways"]:
        lines.append("Used most: " + ", ".join(f"{name} {n}" for name, n in s["ways"][:8]) + ".")
    return lines


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("folder", type=Path)
    ap.add_argument("--weeks", type=float, default=2.0, help="how long the pilot ran")
    ap.add_argument("--very", type=int, help='how many answered "very disappointed"')
    ap.add_argument("--of", type=int, help="of how many people")
    args = ap.parse_args(argv)
    if (args.very is None) != (args.of is None):
        ap.error("--very and --of go together")
    if args.of is not None and not 0 <= args.very <= args.of or args.of == 0:
        ap.error("--very must be between 0 and --of, and --of above 0")

    people, bad = read_folder(args.folder, args.weeks)
    if people:
        print("\n".join(render(people, summarize(people))))
    else:
        print(f"no counts could be read in {args.folder}")
    if args.very is not None:
        lo, hi = wilson(args.very, args.of)
        met = "meets" if args.very / args.of >= BAR else "is below"
        print(f'Very disappointed: {args.very} of {args.of}, {100 * args.very / args.of:.0f}% '
              f"(95% interval {100 * lo:.0f}% to {100 * hi:.0f}%), which {met} the {100 * BAR:.0f}% bar.")
    if bad:
        print(f"could not be read: {', '.join(bad)}")
    return 1 if bad or not people else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
