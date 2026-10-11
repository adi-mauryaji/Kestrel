from intents_v0 import assign_splits, expand, load, load_labels, validate

LABELS = load_labels()
FILLERS = {"en": {"topic": ["python", "budget"], "when": ["tomorrow"]},
           "hi-en": {"topic": ["python", "budget"], "when": ["kal"]}}


def fam(id, intents, en, hi, **extra):
    return {"id": id, "intents": intents, "author": "claude-draft", "reviewed": False,
            "en": en, "hi-en": hi, **extra}


def test_the_real_template_files_are_valid():
    families, fillers = load()
    assert validate(families, fillers, LABELS) == []


def test_validate_reports_each_problem():
    bad = [
        fam("a-01", ["make_coffee"], ["make coffee"], ["coffee banao"]),
        fam("a-01", ["search_notes"], ["find {topc}"], []),
        fam("oos-01", ["unsupported"], ["what's the weather"], ["mausam kaisa hai"]),
        fam("s-01", ["search_notes"], ["find {topic}"], ["{topic} dhundo"], domain="weather"),
        fam("s-02", ["search_notes"], ["find {topic}"], ["{topic} dhoondo"]),
        fam("c-01", ["create_task", "create_task"], ["x {when}"], ["y {when}"], extra_key=1),
    ]
    errors = validate(bad, FILLERS, LABELS)
    for needle in ["unknown intent 'make_coffee'", "duplicate family id 'a-01'", "unknown slot 'topc'",
                   "no hi-en template", "needs a domain", "unsupported families only", "repeats an intent",
                   "duplicate en template 'find {topic}'", "unknown key 'extra_key'"]:
        assert any(needle in e for e in errors), (needle, errors)


def test_expand_fills_slots_per_language_and_is_deterministic():
    f = [fam("s-01", ["search_notes"], ["find notes about {topic}"], ["{topic} wale notes dhundo"])]
    rows = expand(f, FILLERS, per_template=2, seed=0)
    assert sorted(r["text"] for r in rows if r["lang"] == "en") == [
        "find notes about budget", "find notes about python"]
    assert sorted(r["text"] for r in rows if r["lang"] == "hi-en") == [
        "budget wale notes dhundo", "python wale notes dhundo"]
    assert {(r["family"], r["label"], r["group"]) for r in rows} == {("s-01", "search_notes", "s-01")}
    assert rows == expand(f, FILLERS, per_template=2, seed=0)


def test_expand_never_repeats_a_combination():
    f = [fam("s-01", ["search_notes"], ["find {topic} {when}"], ["{when} {topic} dhundo"])]
    rows = expand(f, FILLERS, per_template=5, seed=0)
    assert len(rows) == 4 and len({r["text"] for r in rows}) == 4


def test_compound_and_oos_rows_get_their_label_group_and_stratum():
    rows = expand([fam("cmp-01", ["summarize_notes", "create_task"], ["summarize and add task"],
                       ["summary do aur task banao"]),
                   fam("oos-01", ["unsupported"], ["weather {when}"], ["{when} ka mausam"], domain="weather")],
                  FILLERS)
    by_family = {r["family"]: r for r in rows}
    c, o = by_family["cmp-01"], by_family["oos-01"]
    assert (c["label"], c["intents"], c["stratum"], c["group"]) == (
        "summarize_notes|create_task", ["summarize_notes", "create_task"], "compound", "cmp-01")
    assert (o["label"], o["stratum"], o["group"]) == ("unsupported", "unsupported", "oos:weather")


def _toy_dataset():
    families = [fam(f"{i}-{n:02}", [i], [f"{i} {n} {{topic}}"], [f"{i} {n} {{topic}} karo"])
                for i in ("search_notes", "create_task") for n in range(20)]
    families += [fam(f"cmp-{n:02}", ["search_notes", "create_task"], [f"cmp {n}"], [f"cmp {n} karo"])
                 for n in range(9)]
    families += [fam(f"oos-{d}", ["unsupported"], [f"oos {d} {{topic}}"], [f"oos {d} karo"], domain=d)
                 for d in ("weather", "email", "music", "flights", "shopping", "news", "maths", "jokes",
                           "sports", "traffic")]
    return expand(families, FILLERS, per_template=2, seed=0)


def test_splits_keep_families_and_oos_domains_whole():
    splits = assign_splits(_toy_dataset(), seed=0)
    home = {}
    for name, rows in splits.items():
        for r in rows:
            assert home.setdefault(r["group"], name) == name, f"{r['group']} straddles sets"
    oos_train = {r["group"] for r in splits["train"] if r["label"] == "unsupported"}
    oos_test = {r["group"] for r in splits["test"] if r["label"] == "unsupported"}
    assert oos_train and oos_test and not oos_train & oos_test, "test OOS domains are unseen"


def test_compound_rows_are_evaluation_only():
    splits = assign_splits(_toy_dataset(), seed=0)
    where = {name for name, rows in splits.items() for r in rows if r["stratum"] == "compound"}
    assert where == {"val", "test"}
