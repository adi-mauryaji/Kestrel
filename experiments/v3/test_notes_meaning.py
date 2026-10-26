from notes_meaning import choose_theta, chunks, lexical, policy


def test_a_note_is_cut_into_its_title_with_each_paragraph():
    text = "tags: money\nMonthly budget\n\nRent 12000.\n\nCut subscriptions.\n"
    assert chunks(text) == ["Monthly budget. Rent 12000.", "Monthly budget. Cut subscriptions."]
    assert chunks("Home router\n") == ["Home router"]


def test_lexical_needs_every_term_and_ranks_by_count():
    notes = {"a.md": "budget budget plan", "b.md": "budget only", "c.md": "nothing"}
    assert lexical("budget", notes) == ["a.md", "b.md"]
    assert lexical("budget plan", notes) == ["a.md"]
    assert lexical("money", notes) == []


def test_meaning_fills_after_the_keyword_hits_up_to_k_and_only_above_theta():
    scores = {"a.md": 0.95, "b.md": 0.90, "c.md": 0.80}
    assert policy(["c.md"], scores, theta=0.85, k=3) == (["c.md", "a.md", "b.md"], {"a.md", "b.md"})
    assert policy([], scores, theta=0.92, k=3) == (["a.md"], {"a.md"})
    assert policy(["x.md", "y.md", "z.md"], scores, theta=0.5, k=3) == (["x.md", "y.md", "z.md"], set())


def test_theta_maximizes_found_minus_noise_and_ties_go_up():
    # (found rate, noise rate) per theta
    table = {0.80: (1.0, 0.5), 0.85: (0.9, 0.0), 0.90: (0.9, 0.0), 0.95: (0.5, 0.0)}
    assert choose_theta(table) == 0.90


def test_a_query_with_any_hindi_word_is_translated_first():
    from notes_meaning import needs_translation
    assert needs_translation("har mahine paise bachana")
    assert needs_translation("maa ke liye tohfa")
    assert not needs_translation("present for my mother")
    assert not needs_translation("how to cook lentils")
