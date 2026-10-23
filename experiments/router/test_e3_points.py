import json

import pytest

from e3_points import CONFIGS, SETS, points


def test_every_dataset_has_the_tier_sets_its_tiers_allow():
    assert SETS == {"intents_v0": [(4,), (1, 4), (3, 4), (1, 3, 4)], "clinc150": [(4,), (3, 4)]}


@pytest.mark.parametrize("dataset, full", [("intents_v0", "{1,3,4}"), ("clinc150", "{3,4}")])
def test_the_full_tier_set_reproduces_the_deployed_router(dataset, full):
    cfg = json.loads((CONFIGS / f"router_{dataset}.json").read_text())
    out = points(dataset)
    p = out["sets"][full]["point"]
    t1, t3, t4 = p["tau"]
    accept = cfg["accept"]  # [T1, T2, T3, T4]; a tier left out has no threshold
    assert t3 == pytest.approx(accept[2]) and (t4 or 0.0) == pytest.approx(accept[3])
    assert p["acc"] == pytest.approx(cfg["chosen_on"]["acc"])
    assert out["router_config_sha256"] == cfg["config_sha256"]
