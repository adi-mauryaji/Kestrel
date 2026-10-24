import json
import re
from pathlib import Path

import pytest

from paper_assets import LOGS, ci, fmt_p, numbers, signed, tex_escape, val_operating_points, value_lines


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


def test_tier4_reach_splits_into_answered_and_abstained():
    n = numbers()
    # CLINC150: the SLM is consulted on 17.0% of requests but answers only 6.0%; the other 11.0% abstain.
    assert (n["clinc.r3.tfour"], n["clinc.r3.tfourans"], n["clinc.r3.abstain"]) == ("17.0\\%", "6.0\\%", "11.0\\%")
    assert (n["intents.r3.tfourans"], n["intents.r3.abstain"]) == ("53.6\\%", "0.0\\%")


def test_a_temperature_reorders_tier3_confidences_across_requests():
    n = numbers()
    assert (n["clinc.tthree.kendall"], n["intents.tthree.kendall"]) == ("0.92", "0.94")


def test_validation_search_on_raw_vs_calibrated_confidences():
    clinc = val_operating_points("clinc150")
    frozen = json.loads((LOGS / "router" / "policies_clinc150.json").read_text())["R3"]["conjunction"]["points"]["0.0"]
    # The calibrated search is the one that chose the frozen R3 point.
    assert clinc["cal"]["cost"] == pytest.approx(frozen["cost"]) and clinc["cal"]["tau"] == frozen["tau"]
    n = numbers()
    assert (n["clinc.valop.acc"], n["clinc.valop.raw.cost"], n["clinc.valop.cal.cost"]) == ("0.912", "128", "140")
    assert (n["intents.valop.acc"], n["intents.valop.raw.cost"], n["intents.valop.cal.cost"]) == ("0.984", "467", "478")


PAPER = Path(__file__).resolve().parents[2] / "paper"


def _paper_tex() -> str:
    """The paper's LaTeX source without comments."""
    text = "".join(p.read_text() for p in sorted(PAPER.glob("*.tex")))
    return re.sub(r"(?<!\\)%.*", "", text)


def test_every_number_the_paper_uses_is_generated():
    used = set(re.findall(r"\\kv\{([^}]+)\}", _paper_tex()))
    assert used, "the paper uses no \\kv numbers"
    assert sorted(used - set(numbers())) == []


def test_slide_only_numbers():
    n = numbers()
    assert n["clinc.h1.p"] == "<10^{-4}"
    assert (n["e5.replay.buffer"], n["e5.naive.buffer"]) == ("9.3", "0.0")


def test_every_number_the_slides_use_is_generated():
    slides = re.sub(r"(?<!\\)%.*", "", (PAPER / "slides" / "kestrel_slides.tex").read_text())
    used = set(re.findall(r"\\kv\{([^}]+)\}", slides))
    assert used, "the slides use no \\kv numbers"
    assert sorted(used - set(numbers())) == []


def test_every_citation_is_in_the_bibliography():
    cited = {k.strip() for group in re.findall(r"\\cite[pt]?\*?(?:\[[^]]*\])*\{([^}]+)\}", _paper_tex())
             for k in group.split(",")}
    entries = set(re.findall(r"^@\w+\{([^,\s]+),", (PAPER / "references.bib").read_text(), re.M))
    assert cited and sorted(cited - entries) == []
