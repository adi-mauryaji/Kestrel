import re
from pathlib import Path

import pytest

from paper_assets import ci, fmt_p, numbers, signed, tex_escape, value_lines


def test_number_formats():
    assert signed(0.0476) == "+0.048" and signed(-0.0317) == "-0.032" and signed(-717.2, 0) == "-717"
    assert ci([0.0376, 0.0578]) == "[+0.038, +0.058]"
    assert fmt_p(0.0) == "<10^{-4}" and fmt_p(6.1e-5) == "6.1\\times 10^{-5}" and fmt_p(0.0446) == "0.045"
    assert fmt_p(1.0) == "1"
    assert tex_escape("R0 untuned & {1,4} 50%") == "R0 untuned \\& \\{1,4\\} 50\\%"


def test_value_lines_define_one_macro_per_key():
    lines = value_lines({"clinc.r3.acc": "0.874", "fam.H1c-CLINC150.p": "0.045"})
    assert lines == ["\\expandafter\\def\\csname kv@clinc.r3.acc\\endcsname{\\ensuremath{0.874}}",
                     "\\expandafter\\def\\csname kv@fam.H1c-CLINC150.p\\endcsname{\\ensuremath{0.045}}"]


def test_words_and_hashes_are_not_typeset_as_math():
    lines = value_lines({"x.outcome": "inconclusive (interim)", "repro.head": "83034fe"})
    assert lines == ["\\expandafter\\def\\csname kv@x.outcome\\endcsname{inconclusive (interim)}",
                     "\\expandafter\\def\\csname kv@repro.head\\endcsname{\\texttt{83034fe}}"]


def test_the_paper_numbers_come_from_the_committed_logs():
    n = numbers()
    assert n["clinc.n"] == "5500" and n["intents.n"] == "252"
    assert n["clinc.h1.outcome"] == "supported"
    assert n["clinc.h1.dacc"] == "+0.048" and n["clinc.h1.dacc.ci"] == "[+0.038, +0.058]"
    assert n["clinc.r1.acc"] == "0.893" and n["clinc.r3.acc"] == "0.874"
    assert n["e5.h5.p"] == "6.1\\times 10^{-5}"
    assert n["err.clinc.oosshare"] == "68\\%"
    assert all("??" not in v for v in n.values())
    assert len(n) == len(set(n)) and len(n) > 120
    with pytest.raises(KeyError):
        n["no.such.key"]


PAPER = Path(__file__).resolve().parents[2] / "paper"


def _paper_tex() -> str:
    """The paper's LaTeX source without comments."""
    text = "".join(p.read_text() for p in sorted(PAPER.glob("*.tex")))
    return re.sub(r"(?<!\\)%.*", "", text)


def test_every_number_the_paper_uses_is_generated():
    used = set(re.findall(r"\\kv\{([^}]+)\}", _paper_tex()))
    assert used, "the paper uses no \\kv numbers"
    assert sorted(used - set(numbers())) == []


def test_every_citation_is_in_the_bibliography():
    cited = {k.strip() for group in re.findall(r"\\cite[pt]?\*?(?:\[[^]]*\])*\{([^}]+)\}", _paper_tex())
             for k in group.split(",")}
    entries = set(re.findall(r"^@\w+\{([^,\s]+),", (PAPER / "references.bib").read_text(), re.M))
    assert cited and sorted(cited - entries) == []
