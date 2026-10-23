import pytest

from freeze import FROZEN, freeze_config, retarget_e3
from make_config import config_hash, verify


def cfg(status="CANDIDATE: tuned on val (ADR-019); FROZEN at the PREREG freeze (Week 13)"):
    c = {"format": 1, "status": status, "policy": "R1", "accept": [None, 0.9, None]}
    return c | {"config_sha256": config_hash(c)}


def test_the_freeze_changes_only_the_status_and_the_hash():
    old = cfg()
    new = freeze_config(old)
    assert new["status"] == f"{FROZEN}; tuned on val (ADR-019)"
    long = cfg("CANDIDATE: chosen on val at tolerance 0.0 (ADR-021; delta is only the test margin, PREREG §5); "
               "FROZEN at the PREREG freeze (Week 13)")
    assert freeze_config(long)["status"] == (f"{FROZEN}; chosen on val at tolerance 0.0 (ADR-021; delta is only "
                                             "the test margin, PREREG §5)")
    assert {k for k in old if old[k] != new[k]} == {"status", "config_sha256"}
    assert verify(new) and new["config_sha256"] != old["config_sha256"]
    assert freeze_config(new) == new  # idempotent


def test_e3_points_follow_their_router_config_to_its_frozen_hash():
    e3 = {"datasets": {"intents_v0": {"router_config_sha256": "old-a", "sets": {}},
                       "clinc150": {"router_config_sha256": "old-b", "sets": {}}}}
    out = retarget_e3(e3, {"old-a": "new-a", "old-b": "new-b"})
    assert [v["router_config_sha256"] for v in out["datasets"].values()] == ["new-a", "new-b"]
    assert e3["datasets"]["intents_v0"]["router_config_sha256"] == "old-a"  # not mutated
    with pytest.raises(ValueError, match="clinc150"):  # points chosen for some other config
        retarget_e3(e3, {"old-a": "new-a"})
