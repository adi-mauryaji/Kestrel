import hashlib

import pytest

from repro import STEPS, compare, copy_verified, e5_numbers


def test_every_analysis_output_is_compared():
    outs = [o for _, outs in STEPS for o in outs]
    assert "experiments/RESULTS.md" in outs and "experiments/logs/family_v1.json" in outs
    assert len(outs) == len(set(outs))


def test_compare_names_the_files_that_differ(tmp_path):
    a, b = tmp_path / "a", tmp_path / "b"
    for root in (a, b):
        (root / "x").mkdir(parents=True)
        (root / "x" / "same.json").write_text("{}\n")
    (a / "x" / "diff.json").write_text("1\n")
    (b / "x" / "diff.json").write_text("2\n")
    assert compare(a, b, ["x/same.json", "x/diff.json", "x/missing.json"]) == {
        "x/same.json": "identical", "x/diff.json": "differs", "x/missing.json": "missing"}


def test_raw_files_are_copied_only_if_their_hash_matches(tmp_path):
    src, dst = tmp_path / "src", tmp_path / "dst"
    (src / "clinc150").mkdir(parents=True)
    (src / "clinc150" / "d.json").write_text("data")
    good = {"file": "clinc150/d.json", "sha256": hashlib.sha256(b"data").hexdigest()}
    copy_verified([good], src, dst)
    assert (dst / "clinc150" / "d.json").read_text() == "data"
    with pytest.raises(ValueError, match="d.json"):
        copy_verified([good | {"sha256": "0" * 64}], src, tmp_path / "other")


def test_e5_numbers_leave_out_the_timings():
    rep = {"table_r6": {"naive": {"avg_accuracy": {"mean": 0.2}, "seconds": {"mean": 3.6}, "runs": 15}},
           "h5": {"diff": -0.9, "ci95": [-0.95, -0.94], "p": 6e-5}}
    assert e5_numbers(rep) == {"naive": {"avg_accuracy": {"mean": 0.2}, "runs": 15},
                               "h5": {"diff": -0.9, "ci95": [-0.95, -0.94], "p": 6e-5}}
