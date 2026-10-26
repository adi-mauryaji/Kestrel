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
