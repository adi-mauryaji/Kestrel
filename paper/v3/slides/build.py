"""paper/v3/slides/build.py -- the v3 presentation, built from its template and the generated numbers.

    .venv/bin/python paper/v3/slides/build.py              # -> kestrel_v3_slides.html, kestrel_v3_slides.pdf, speaker_notes.md
    .venv/bin/python paper/v3/slides/build.py --shots DIR  # also one PNG per slide in DIR (to look at every slide)

Every {{key}} in deck.template.html and speaker_notes.template.md is a value from paper/v3/generated/numbers.json
(experiments/analysis/paper_assets_v3.py makes it from the committed logs); an unknown key stops the build. Every
[[file:path]] (fonts, the screenshot), relative to this folder, is inlined as a data URI, so the HTML works
offline. The PDF is printed by headless Google Chrome: one 1920 x 1080 page per slide, animations at their end.
"""
from __future__ import annotations

import argparse
import base64
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
NUMBERS = HERE.parent / "generated" / "numbers.json"
KEY = re.compile(r"\{\{([^{}]+)\}\}")
FILE = re.compile(r"\[\[file:([^\]]+)\]\]")
MIME = {".woff": "font/woff", ".png": "image/png", ".svg": "image/svg+xml", ".jpg": "image/jpeg"}


def fill(text: str, numbers: dict[str, str], where: str) -> str:
    unknown = sorted({k for k in KEY.findall(text) if k not in numbers})
    if unknown:
        raise SystemExit(f"{where}: keys not in {NUMBERS.name}: {', '.join(unknown)}")
    return KEY.sub(lambda m: numbers[m.group(1)], text)


def inline(text: str) -> str:
    def data(m: re.Match) -> str:
        path = (HERE / m.group(1)).resolve()
        return f"data:{MIME[path.suffix]};base64," + base64.b64encode(path.read_bytes()).decode()
    return FILE.sub(data, text)


def chrome() -> str:
    for name in ("google-chrome", "google-chrome-stable", "chromium", "chromium-browser"):
        if path := shutil.which(name):
            return path
    raise SystemExit("Google Chrome or Chromium is needed to print the PDF")


def run_chrome(*args: str) -> None:
    with tempfile.TemporaryDirectory() as profile:  # a throwaway profile: never the user's
        subprocess.run([chrome(), "--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-first-run",
                        f"--user-data-dir={profile}", "--run-all-compositor-stages-before-draw",
                        "--virtual-time-budget=8000", *args],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120)


def pdf_pages(path: Path) -> int:
    return len(re.findall(rb"/Type\s*/Page(?!s)", path.read_bytes()))


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--shots", type=Path, help="write one PNG per slide into this folder")
    args = ap.parse_args(argv)
    numbers = json.loads(NUMBERS.read_text())

    html = inline(fill((HERE / "deck.template.html").read_text(), numbers, "deck.template.html"))
    out = HERE / "kestrel_v3_slides.html"
    out.write_text(html)
    slides = len(re.findall(r'<section class="slide[ "]', html))
    notes = fill((HERE / "speaker_notes.template.md").read_text(), numbers, "speaker_notes.template.md")
    (HERE / "speaker_notes.md").write_text(notes)
    noted = len(re.findall(r"^## \d+ ", notes, re.M))
    if noted != slides:
        raise SystemExit(f"speaker notes cover {noted} slides, the deck has {slides}")

    pdf = HERE / "kestrel_v3_slides.pdf"
    run_chrome("--no-pdf-header-footer", f"--print-to-pdf={pdf}", out.as_uri() + "?print")
    pages = pdf_pages(pdf)
    if pages != slides:
        raise SystemExit(f"{pdf.name} has {pages} pages for {slides} slides")
    print(f"{slides} slides -> {out.name} ({out.stat().st_size // 1024} KiB), {pdf.name} ({pages} pages), speaker_notes.md")

    if args.shots:
        args.shots.mkdir(parents=True, exist_ok=True)
        for i in range(1, slides + 1):
            shot = args.shots / f"slide-{i:02d}.png"
            run_chrome("--window-size=1920,1080", f"--screenshot={shot}", out.as_uri() + f"?static&slide={i}")
        print(f"{slides} screenshots -> {args.shots}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
