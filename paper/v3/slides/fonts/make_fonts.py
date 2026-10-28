"""paper/v3/slides/fonts/make_fonts.py -- the deck's fonts, cut down from the Noto fonts installed on this machine.

    .venv/bin/python paper/v3/slides/fonts/make_fonts.py    # -> paper/v3/slides/fonts/*.woff (run once; committed)

The deck must work offline from one HTML file, so build.py inlines these files as data URIs. Subsetting to the
characters the deck can show keeps that file small: Latin, Latin-1, general punctuation, arrows, the rupee and
minus signs, and (for Noto Sans Devanagari) only the Devanagari characters that appear in the deck's template.
Noto is under the SIL Open Font License 1.1, which allows subsetting and embedding; each font's own licence
string is printed below and kept in the subset.
"""
from __future__ import annotations

import re
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

HERE = Path(__file__).resolve().parent
TEMPLATE = HERE.parent / "deck.template.html"
SRC = Path("/usr/share/fonts")
FONTS = {  # output name: installed file
    "sans-regular": "google-noto/NotoSans-Regular.ttf",
    "sans-medium": "google-noto/NotoSans-Medium.ttf",
    "sans-semibold": "google-noto/NotoSans-SemiBold.ttf",
    "cond-semibold": "google-noto/NotoSans-CondensedSemiBold.ttf",
    "xcond-light": "google-noto/NotoSans-ExtraCondensedLight.ttf",
    "serif-italic": "google-noto/NotoSerif-Italic.ttf",
    "deva": "google-noto-vf/NotoSansDevanagari[wght].ttf",
}
LATIN = ([*range(0x20, 0x7F), *range(0xA0, 0x100), *range(0x2010, 0x2028), *range(0x2030, 0x203B),
          *range(0x2190, 0x219A), 0x20B9, 0x2212, 0x2248, 0x2264, 0x2265, 0x2009, 0x202F, 0x2713, 0x2717])


def devanagari() -> list[int]:
    chars = set(re.findall(r"[ऀ-ॿ]", TEMPLATE.read_text()))
    return sorted(ord(c) for c in chars) + [0x200C, 0x200D, 0x20, 0x2C]


def main() -> int:
    for name, rel in FONTS.items():
        options = subset.Options()
        options.flavor = "woff"
        options.layout_features = ["*"]
        options.name_IDs = ["*"]
        options.notdef_outline = True
        font = TTFont(SRC / rel)
        sub = subset.Subsetter(options)
        sub.populate(unicodes=devanagari() if name == "deva" else LATIN)
        sub.subset(font)
        out = HERE / f"{name}.woff"
        font.flavor = "woff"
        font.save(out)
        licence = font["name"].getDebugName(13) or "?"
        print(f"{out.name}: {out.stat().st_size // 1024} KiB  ({licence[:60]})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
