import numpy as np
import pytest

from e5_data import DOMAINS, by_stage, check_domains, load, no_runs

RAW = DOMAINS.parent / "data_full.json"


def test_domains_must_cover_the_intents_exactly():
    domains = {"a": ["x", "y"], "b": ["z"]}
    assert check_domains(domains, ["z", "y", "x"]) == domains
    for bad, intents in [({"a": ["x"], "b": ["x"]}, ["x"]),      # an intent in two domains
                         ({"a": ["x"]}, ["x", "y"]),             # an intent without a domain
                         ({"a": ["x", "q"]}, ["x"])]:            # a domain intent not in the data
        with pytest.raises(ValueError):
            check_domains(bad, intents)


def test_a_cache_miss_is_an_error_not_a_model_run():
    with pytest.raises(RuntimeError, match="cache"):
        no_runs()


def test_by_stage_groups_items_by_their_intents_domain():
    data = {"classes": ["a1", "a2", "b1", "c1"], "domains": {"A": ["a1", "a2"], "B": ["b1"], "C": ["c1"]},
            "x": {"val": np.arange(10.0).reshape(5, 2)}, "y": {"val": np.array([0, 2, 1, 3, 2])}}
    parts = by_stage(data, "val", [["A"], ["B", "C"]])
    assert [p[1].tolist() for p in parts] == [[0, 1], [2, 3, 2]]
    assert parts[1][0].tolist() == [[2.0, 3.0], [6.0, 7.0], [8.0, 9.0]]


@pytest.mark.skipif(not (RAW.exists() and DOMAINS.exists()), reason="needs datasets/raw (scripts/fetch_datasets.py)")
def test_the_real_sequence_loads_from_the_cache():
    try:
        data = load()
    except RuntimeError as e:  # no Week-5 cache on this machine
        pytest.skip(str(e))
    assert len(data["classes"]) == 150 and len(data["domains"]) == 10
    assert data["x"]["train"].shape == (15000, 384)
    assert set(np.unique(data["y"]["val"])) == set(range(150)), "every intent has val items"
    assert all(len(v) == 15 for v in data["domains"].values())
