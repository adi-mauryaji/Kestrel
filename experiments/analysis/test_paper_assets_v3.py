import re
from pathlib import Path

import pytest

from paper_assets_v3 import _check, fmt, numbers, pct, tex_values, wci, wilson

V3 = Path(__file__).resolve().parents[2] / "paper" / "v3"


def test_rounding_is_half_up_from_the_exact_value():
    assert pct(78, 96) == "81.3%" and pct(1, 8) == "12.5%" and pct(2103, 3247) == "64.8%"
    assert pct(2103, 3247, 0) == "65%"
    assert fmt(0.625, 2) == "0.63" and fmt(4.95, 1) == "5.0" and fmt(523.457693, 0) == "523"


def test_wilson_interval():
    lo, hi = wilson(25, 25)
    assert lo == pytest.approx(25 / (25 + 1.959964 ** 2)) and hi == 1.0
    assert wci(21, 24) == "[69.0%, 95.7%]"


def test_a_logged_summary_that_its_rows_contradict_stops_the_build():
    with pytest.raises(ValueError):
        _check(21, 20, "pass@1")


def test_the_v3_numbers_come_from_the_committed_logs():
    n = numbers()
    assert (n["kt2.n"], n["kt2.hg.right"], n["kt2.hg.asked"], n["kt2.hg.notaskedfor"]) == ("280", "256/280", "13", "6")
    assert (n["kt1.hg.right"], n["kt1.hg.sentoldway"], n["kt2.fresh.hg.right"]) == ("220/256", "25", "23/24")
    assert (n["kt2.hg.skill"], n["kt2.hg.skill.pct"]) == ("78/96", "81.3%")
    assert (n["fb2.tau"], n["fb2.taug"], n["mdt.theta"]) == ("0.8", "0.7", "0.83")
    assert (n["md.found.hi"], n["mdt.found.hi"], n["mtt.found"], n["mtt.noise"]) == ("1/9", "9/9", "14/18", "0/2")
    assert (n["qa.test.ans"], n["qa.test.un"], n["qa.dev.un"]) == ("19/19", "6/6", "4/6")
    assert (n["qa.test.s.mean"], n["qa.dev.s.mean"]) == ("4.9", "5.1")  # from the per-question times
    assert (n["code.test.all"], n["code.test.script"], n["code.test.retries"]) == ("21/24", "3/6", "0")
    assert n["code.test.explain.en.english"] == "0/3"
    assert (n["mmap.copy.cold.anon"], n["mmap.map.cold.anon"], n["mmap.anon.cut"]) == ("3247", "1144", "64.8%")
    assert (n["worker.v2.s"], n["worker.hit.s"]) == ("19.1", "2.0")
    assert (n["acc.socket"], n["acc.page.headed"], n["acc.cargo"], n["acc.pytest"]) == ("34/36", "8/8", "448", "510")
    assert all("??" not in v for v in n.values()) and len(n) > 500


def test_tex_escapes_percent_signs():
    assert tex_values({"a": "81.3%", "b": "[69.0%, 95.7%]"}) == {"a": "81.3\\%", "b": "[69.0\\%, 95.7\\%]"}


def _tex(path: Path) -> str:
    return re.sub(r"(?<!\\)%.*", "", path.read_text())


@pytest.mark.parametrize("doc", sorted(p.name for p in V3.glob("*.tex")))
def test_every_number_a_v3_document_uses_is_generated(doc):
    used = set(re.findall(r"\\kv\{([^}]+)\}", _tex(V3 / doc)))
    assert used, f"{doc} uses no \\kv numbers"
    assert sorted(used - set(numbers())) == []


def test_every_number_the_v3_slides_use_is_generated():
    templates = sorted((V3 / "slides").glob("*.template.*"))
    assert templates, "no slide templates"
    used = {k for t in templates for k in re.findall(r"\{\{([^{}]+)\}\}", t.read_text())}
    assert used and sorted(used - set(numbers())) == []


def test_every_v3_citation_is_in_a_bibliography():
    cited = {k.strip() for p in V3.glob("*.tex")
             for group in re.findall(r"\\cite[pt]?\*?(?:\[[^]]*\])*\{([^}]+)\}", _tex(p)) for k in group.split(",")}
    bibs = (V3.parent / "references.bib").read_text() + (V3 / "references_v3.bib").read_text()
    entries = set(re.findall(r"^@\w+\{([^,\s]+),", bibs, re.M))
    assert cited and sorted(cited - entries) == []
