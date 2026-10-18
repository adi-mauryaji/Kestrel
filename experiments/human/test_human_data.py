import json

import pytest

from human_data import (
    KAPPA_LABELS,
    TASKS,
    cohen_kappa,
    gold_kappa_label,
    kappa_sample,
    load_intents_v0,
    parse_submission,
)


def test_kappa_sample_is_a_fifth_stratified_by_label_and_language():
    rows = load_intents_v0()
    sample = kappa_sample(rows, frac=0.2, seed=0)
    assert len(sample) >= 0.2 * len(rows)
    strata = {(gold_kappa_label(r), r["lang"]) for r in rows}
    got = {(gold_kappa_label(r), r["lang"]) for r in sample}
    assert got == strata  # every label x language cell is represented
    assert kappa_sample(rows, frac=0.2, seed=0) == sample  # fixed
    assert len({r["id"] for r in sample}) == len(sample)


def test_kappa_sample_leaves_out_excluded_texts():
    rows = load_intents_v0()
    banned = {rows[0]["text"], rows[1]["text"]}
    sample = kappa_sample(rows, frac=0.2, seed=0, exclude_texts=banned)
    assert not banned & {r["text"] for r in sample}


def test_gold_label_maps_compounds_to_one_category():
    assert gold_kappa_label({"label": "create_task"}) == "create_task"
    assert gold_kappa_label({"label": "summarize_notes|create_task"}) == "compound"
    assert "compound" in KAPPA_LABELS and len(KAPPA_LABELS) == 10


def test_cohen_kappa_known_values():
    assert cohen_kappa(["a", "b", "a", "b"], ["a", "b", "a", "b"]) == pytest.approx(1.0)
    # 2x2 table [[20, 5], [10, 15]]: po = 0.7, pe = 0.5 -> kappa = 0.4
    a = ["y"] * 25 + ["n"] * 25
    b = ["y"] * 20 + ["n"] * 5 + ["y"] * 10 + ["n"] * 15
    assert cohen_kappa(a, b) == pytest.approx(0.4)
    with pytest.raises(ValueError):
        cohen_kappa(["a"], ["a", "b"])


def test_parse_submission_accepts_an_annotation_and_rejects_bad_ones():
    ok = {"kind": "kestrel-annotation", "version": 1, "code": "AB",
          "answers": {"chat-01/en/0/0": "chitchat", "x/y": "compound"}}
    sub = parse_submission(json.dumps(ok), known_ids={"chat-01/en/0/0", "x/y"})
    assert sub["answers"]["x/y"] == "compound"
    bad_label = dict(ok, answers={"x/y": "weather"})
    with pytest.raises(ValueError, match="label"):
        parse_submission(json.dumps(bad_label), known_ids={"x/y"})
    with pytest.raises(ValueError, match="id"):
        parse_submission(json.dumps(ok), known_ids={"x/y"})
    with pytest.raises(ValueError, match="kind"):
        parse_submission(json.dumps(dict(ok, kind="other")), known_ids={"x/y", "chat-01/en/0/0"})


def test_parse_submission_accepts_written_requests():
    task = TASKS[0]["id"]
    sub = {"kind": "kestrel-requests", "version": 1, "code": "RK", "consent": True,
           "requests": [{"task": task, "lang": "hi-en", "text": "  Kal subah 7 baje yaad dilana  "}]}
    got = parse_submission(json.dumps(sub), known_ids=set())
    assert got["requests"][0]["text"] == "Kal subah 7 baje yaad dilana"
    with pytest.raises(ValueError, match="consent"):
        parse_submission(json.dumps(dict(sub, consent=False)), known_ids=set())
    with pytest.raises(ValueError, match="task"):
        parse_submission(json.dumps(dict(sub, requests=[{"task": "nope", "lang": "en", "text": "x y"}])),
                         known_ids=set())


def test_guide_covers_every_label_and_its_examples_are_not_dataset_items():
    from human_data import GUIDE, RULES
    from prep import norm_text

    assert [g[0] for g in GUIDE] == KAPPA_LABELS
    items = {norm_text(r["text"]) for r in load_intents_v0()}
    examples = [norm_text(e) for _, _, exs in GUIDE for e in exs]
    assert not set(examples) & items, set(examples) & items
    long_items = [t for t in items if len(t.split()) >= 3]
    text = " ".join(examples + [norm_text(r) for r in RULES])
    assert not [t for t in long_items if t in text]


def test_task_prompts_contain_no_dataset_items():
    from prep import norm_text

    asks = " ".join(norm_text(t["ask"]) for t in TASKS)
    long_items = [norm_text(r["text"]) for r in load_intents_v0() if len(r["text"].split()) >= 3]
    assert not [t for t in long_items if t in asks]
    assert {t["intent"] for t in TASKS} == set(KAPPA_LABELS)


def test_page_data_is_blind():
    from build_page import page_data

    data = page_data(kappa_sample(load_intents_v0()))
    assert all(set(item) == {"id", "text"} for item in data["items"])  # no gold label
    assert all(set(task) == {"id", "ask"} for task in data["tasks"])  # no intent name
    assert [g["label"] for g in data["guide"]] == KAPPA_LABELS


def test_committed_page_and_sample_are_current(tmp_path):
    from build_page import PAGE, SAMPLE, build

    build(tmp_path / "page.html", tmp_path / "sample.json")
    assert (tmp_path / "page.html").read_text() == PAGE.read_text()
    assert (tmp_path / "sample.json").read_text() == SAMPLE.read_text()


def test_ingest_writes_validated_files_and_reports_kappa(tmp_path):
    from ingest import ingest, kappa_report

    rows = load_intents_v0()
    sample = kappa_sample(rows)
    gold = {r["id"]: gold_kappa_label(r) for r in rows}
    inbox = tmp_path / "inbox"
    inbox.mkdir()
    # A pasted message: chat text around the JSON, as it arrives from WhatsApp.
    answers = {r["id"]: gold[r["id"]] for r in sample[:50]}
    answers[sample[0]["id"]] = "chitchat" if gold[sample[0]["id"]] != "chitchat" else "unsupported"
    (inbox / "ab.txt").write_text("here you go: " + json.dumps(
        {"kind": "kestrel-annotation", "version": 1, "code": "AB", "answers": answers}) + " thanks!")
    # A document exported from the page's store (ArtifactData list with out_dir).
    store = tmp_path / "store" / "requests"
    store.mkdir(parents=True)
    (store / "u_x.json").write_text(json.dumps({"id": "u_x", "data": {
        "kind": "kestrel-requests", "version": 1, "code": "RK", "consent": True,
        "requests": [{"task": "remind", "lang": "hi-en", "text": "kal 6 baje uthana"}]}}))
    out = tmp_path / "out"
    written = ingest([inbox, tmp_path / "store"], out)
    assert sorted(p.name for p in written) == ["annotator_AB.json", "requests_RK.jsonl"]
    rep = kappa_report(json.loads((out / "annotator_AB.json").read_text()))
    assert rep["n"] == 50 and rep["agreement"] == pytest.approx(49 / 50)
    assert 0.9 < rep["kappa"] < 1.0
    assert set(rep["by_lang"]) <= {"en", "hi-en"}


def test_ingest_refuses_invalid_submissions(tmp_path):
    from ingest import ingest

    inbox = tmp_path / "inbox"
    inbox.mkdir()
    (inbox / "bad.txt").write_text(json.dumps({"kind": "kestrel-annotation", "version": 1, "code": "Z",
                                               "answers": {"no-such-id": "chitchat"}}))
    with pytest.raises(ValueError, match="bad.txt"):
        ingest([inbox], tmp_path / "out")
