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
