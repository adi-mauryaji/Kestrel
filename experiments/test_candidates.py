import json
import shutil

from candidates import ROOT, build, check


def test_the_manifest_of_the_repository_as_it_is_checks_clean():
    m = build(write=False)
    assert check(m) == []
    # ADR-031: PREREG v1 froze E1, E2, E3, E5; E4 and E6 are frozen later by amendment. Their datasets were
    # reviewed on 2026-10-26: E4 is a candidate, E6 still waits for delta3 and its PREREG items.
    assert {e: v["status"].split(" ")[0] for e, v in m["experiments"].items()} == {
        "E1": "FROZEN", "E2": "FROZEN", "E3": "FROZEN", "E4": "CANDIDATE", "E5": "FROZEN", "E6": "BLOCKED"}
    assert json.loads((ROOT / "experiments/configs/e5.json").read_text())["status"].startswith("FROZEN")
    assert m["experiments"]["E6"]["blocked_by"]
    assert len(m["experiments"]["E6"]["grid"]) == 6, "3 shortlisted SLMs x {Q4, Q8}"
    # ADR-030: E4's protocol is accepted and its dataset reviewed, so nothing blocks it; E6 scores the LM-only planner.
    assert m["experiments"]["E4"]["blocked_by"] == []
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


def test_a_frozen_experiment_with_an_unfrozen_router_config_is_reported(tmp_path):
    m = build(write=False)
    root = tmp_path / "repo"
    for rel in m["files"]:
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(ROOT / rel, root / rel)
    path = root / "experiments/configs/router_clinc150_r1.json"
    cfg = json.loads(path.read_text())
    cfg["status"] = "CANDIDATE: edited back"
    path.write_text(json.dumps(cfg, indent=1) + "\n")
    assert any("router_clinc150_r1.json" in p and "not FROZEN" in p for p in check(m, root))


def test_a_reviewed_dataset_no_longer_blocks_its_experiment():
    from candidates import review_blocker
    assert review_blocker("Kestrel-Tasks-v0", "draft") == ["Kestrel-Tasks-v0 review and freeze (it is a draft)"]
    assert review_blocker("Kestrel-Tasks-v0", "reviewed") == []
