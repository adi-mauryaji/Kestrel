import pytest

from rss import command, summarize

CFG = {"R1": {"policy": "R1", "config_sha256": "aaa", "status": "CANDIDATE"},
       "R3": {"policy": "R3", "config_sha256": "ccc", "status": "CANDIDATE"}}


def meta(policy, sha, rss=812.5):
    return {"systems": [{"policy": policy, "config_sha256": sha}], "tiers_loaded": [3], "items": 5, "requests": 5,
            "load_s": 1.2, "peak_rss_mib": rss, "rss_anon_mib_at_end": None if rss is None else rss - 10,
            "rss_file_mib_at_end": 9.0}


def test_summary_per_system():
    s = summarize("intents_v0", {"R1": meta("R1", "aaa"), "R3": meta("R3", "ccc", 3600.0)}, CFG)
    assert s["dataset"] == "intents_v0"
    assert s["systems"]["R1"] == {"config_sha256": "aaa", "status": "CANDIDATE", "peak_rss_mib": 812.5,
                                  "rss_anon_mib_at_end": 802.5, "rss_file_mib_at_end": 9.0, "load_s": 1.2,
                                  "tiers_loaded": [3]}
    assert s["systems"]["R3"]["peak_rss_mib"] == 3600.0


def test_a_process_must_have_run_only_its_own_config():
    with pytest.raises(ValueError, match="R1"):
        summarize("intents_v0", {"R1": meta("R1", "other")}, CFG)
    two = meta("R1", "aaa") | {"systems": [{"policy": "R1", "config_sha256": "aaa"},
                                           {"policy": "R3", "config_sha256": "ccc"}]}
    with pytest.raises(ValueError, match="R1"):
        summarize("intents_v0", {"R1": two}, CFG)
    with pytest.raises(ValueError, match="peak"):
        summarize("intents_v0", {"R1": meta("R1", "aaa", rss=None)}, CFG)


def test_the_command_pins_one_cpu_per_core_and_runs_one_config():
    cmd = command([0, 2, 4], "/tmp/items.jsonl", "/tmp/out/R1.jsonl", "experiments/configs/router_intents_v0_r1.json")
    assert cmd[:3] == ["taskset", "-c", "0,2,4"]
    assert cmd[4:] == ["bench-items", "/tmp/items.jsonl", "/tmp/out/R1.jsonl",
                       "experiments/configs/router_intents_v0_r1.json"]
