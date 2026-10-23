import json
import shutil

from candidates import ROOT, build, check


def test_the_manifest_of_the_repository_as_it_is_checks_clean():
    m = build(write=False)
    assert check(m) == []
    assert {e: v["status"] for e, v in m["experiments"].items()} == {
        "E1": "CANDIDATE", "E2": "CANDIDATE", "E3": "CANDIDATE", "E4": "BLOCKED", "E5": "CANDIDATE", "E6": "BLOCKED"}
    assert all(m["experiments"][e]["blocked_by"] for e in ("E4", "E6"))
    assert len(m["experiments"]["E6"]["grid"]) == 6, "3 shortlisted SLMs x {Q4, Q8}"
    # ADR-030: E4's protocol is accepted (only its dataset blocks it); E6 scores the LM-only planner.
    assert m["experiments"]["E4"]["blocked_by"] == ["Kestrel-Personas-v0 review and freeze (it is a draft)"]
    assert "aliases" in m["experiments"]["E4"]["test"]
    assert m["experiments"]["E6"]["planner"].startswith("LM-only")
    assert not any("planning path" in b for b in m["experiments"]["E6"]["blocked_by"])


def test_a_changed_file_or_a_stale_cross_reference_is_reported(tmp_path):
    m = build(write=False)
    root = tmp_path / "repo"
    for rel in m["files"]:
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(ROOT / rel, root / rel)
    assert check(m, root) == []
    cfg_path = root / "experiments/configs/router_intents_v0.json"
    cfg = json.loads(cfg_path.read_text())
    cfg["accept"][2] = 0.5  # thresholds edited by hand, hash left as it was
    cfg_path.write_text(json.dumps(cfg, indent=1) + "\n")
    problems = check(m, root)
    assert any("router_intents_v0.json" in p and "changed" in p for p in problems), problems
    assert any("config_sha256" in p for p in problems), problems
    (root / "experiments/configs/e5.json").unlink()
    assert any("e5.json" in p and "missing" in p for p in check(m, root))
