from kind_fallback import decide, score

SURE_EMAIL = {"app": 0.1, "chat": 0.0, "email": 0.9, "rewrite": 0.0, "translate": 0.0}
UNSURE_CHAT = {"app": 0.4, "chat": 0.5, "email": 0.1, "rewrite": 0.0, "translate": 0.0}
UNSURE_APP = {"app": 0.55, "chat": 0.45, "email": 0.0, "rewrite": 0.0, "translate": 0.0}


def test_the_head_alone_decides_as_kinds_rs_does():
    assert decide(SURE_EMAIL, 0.7) == ("take", "email")
    assert decide(UNSURE_CHAT, 0.7) == ("ask", "chat")
    assert decide(UNSURE_APP, 0.7) == ("take", "app")  # app at any probability: the old path


def test_below_tau_a_sure_gemma_decides_and_an_unsure_one_leaves_the_head_s_answer():
    assert decide(SURE_EMAIL, 0.7, ("chat", 0.99), 0.8) == ("take", "email"), "the sure head is not asked"
    assert decide(UNSURE_CHAT, 0.7, ("rewrite", 0.85), 0.8) == ("take", "rewrite")
    assert decide(UNSURE_APP, 0.7, ("chat", 0.85), 0.8) == ("take", "chat")
    assert decide(UNSURE_CHAT, 0.7, ("rewrite", 0.6), 0.8) == ("ask", "chat")
    assert decide(UNSURE_APP, 0.7, ("chat", 0.6), 0.8) == ("take", "app")


def test_score_counts_right_wrong_and_asked_decisions():
    decisions = [("take", "email"), ("take", "app"), ("take", "chat"), ("take", "app"),
                 ("ask", "chat"), ("ask", "rewrite")]
    gold = ["email", "app", "app", "chat", "chat", "translate"]
    assert score(decisions, gold) == {
        "n": 6, "taken": 4, "right": 2,
        "skill_not_asked_for": 1,  # chat written for an app request
        "skill_sent_old_way": 1,  # a chat request sent down the app path
        "asked": 2, "asked_right_kind": 1,
    }


def test_the_config_carries_what_the_runtime_needs_from_the_dev_run():
    from kind_fallback import fallback_config
    meta = {"slm": "gemma", "model_file": "gemma.gguf", "prompt": "tier4_kind_prompt.txt", "prompt_sha256": "p",
            "shots": [{"text": "hi", "label": "app"}], "head_sha256": "h"}
    cfg = fallback_config(meta, 1.5, 0.9, "You are the router.")
    assert {k: cfg[k] for k in ("model_file", "prompt", "prompt_sha256", "system", "shots", "temperature", "tau_g")} == {
        "model_file": "gemma.gguf", "prompt": "tier4_kind_prompt.txt", "prompt_sha256": "p",
        "system": "You are the router.", "shots": [{"text": "hi", "label": "app"}], "temperature": 1.5, "tau_g": 0.9}


def test_the_committed_config_matches_the_dev_log_and_the_prompt():
    import hashlib
    import json
    from kind_fallback import CONFIG, ROOT, SUMMARY
    cfg, log = json.loads(CONFIG.read_text()), json.loads(SUMMARY.read_text())
    prompt = ROOT / "datasets" / "contracts" / cfg["prompt"]
    assert cfg["prompt_sha256"] == hashlib.sha256(prompt.read_bytes()).hexdigest()
    assert cfg["system"] == prompt.read_text(encoding="utf-8"), "the runtime uses the dev run's prompt"
    assert cfg["temperature"] == log["temperature_gemma"] and cfg["shots"] == log["meta"]["shots"]
    assert str(cfg["tau_g"]) in log["val_head_gemma"]


def test_each_dataset_has_its_own_head_prompt_logs_and_config():
    from kind_fallback import CONFIG, OUTPUTS, ROOT, SUMMARY, paths
    v0, v1 = paths("kinds_v0"), paths("kinds_v1")
    assert (v0["config"], v0["outputs"], v0["summary"]) == (CONFIG, OUTPUTS, SUMMARY)
    assert v0["labels"].name == "kinds.txt" and v1["labels"].name == "kinds_v1.txt"
    assert v1["prompt"].name == "tier4_kind_prompt_v1.txt"
    assert v1["config"] == ROOT / "experiments" / "configs" / "kinds_fallback_v1.json"
    assert v1["summary"].name == "fallback_dev_v1.json" and v1["outputs"].name == "fallback_outputs_v1.jsonl"
    assert v1["head"].name.startswith("kinds_v1-")


def test_the_config_names_its_labels_and_dev_log():
    from kind_fallback import fallback_config, paths
    meta = {"slm": "gemma", "model_file": "gemma.gguf", "prompt": "p.txt", "prompt_sha256": "p",
            "shots": [], "head_sha256": "h"}
    cfg = fallback_config(meta, 1.5, 0.9, "s", paths("kinds_v1"))
    assert cfg["labels"] == "kinds_v1.txt" and cfg["dev_log"].endswith("fallback_dev_v1.json")
    assert fallback_config(meta, 1.5, 0.9, "s")["labels"] == "kinds.txt"


def test_tau_and_tau_g_are_chosen_by_fewest_wrong_then_asks_then_fewer_gemma_calls():
    from kind_fallback import choose
    s = lambda bad, old, asked: {"skill_not_asked_for": bad, "skill_sent_old_way": old, "asked": asked}
    grid = {(0.7, 0.9): s(1, 1, 3), (0.8, 0.9): s(1, 0, 5), (0.6, 0.8): s(0, 1, 5), (0.9, 0.9): s(0, 1, 9)}
    assert choose(grid) == (0.6, 0.8)  # 1 wrong, 5 asks; lower tau wins over (0.8, 0.9)
    grid[(0.6, 0.9)] = s(1, 0, 5)
    assert choose(grid) == (0.6, 0.9)  # same wrong, asks and tau: the higher tau_g


def test_the_test_run_is_refused_once_its_log_exists(tmp_path):
    import pytest
    from kind_fallback import refuse_second_test_run
    log = tmp_path / "fallback_test_v1.json"
    refuse_second_test_run(log)
    log.write_text("{}")
    with pytest.raises(SystemExit, match="once"):
        refuse_second_test_run(log)


def test_kinds_v2_has_its_own_prompt_logs_and_config_and_a_fresh_test_part():
    from kind_fallback import fresh, paths
    v2 = paths("kinds_v2")
    assert v2["prompt"].name == "tier4_kind_prompt_v2.txt" and v2["labels"].name == "kinds_v2.txt"
    assert v2["config"].name == "kinds_fallback_v2.json" and v2["summary"].name == "fallback_dev_v2.json"
    rows = [{"source": "kinds_v2"}, {"source": "kinds_v1"}, {"source": "intents_v0"}]
    assert [fresh(r) for r in rows] == [True, False, False]
