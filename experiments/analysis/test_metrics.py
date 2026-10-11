import pytest

from metrics import accuracy, ece, macro_f1, per_item_correct

A = None  # an abstention


def test_abstention_is_wrong_in_scope_and_right_out_of_scope():
    gold = ["a", "b", "oos", "oos", "oos"]
    pred = ["a", A, A, "oos", "a"]
    assert per_item_correct(gold, pred, oos_label="oos") == [True, False, True, True, False]
    assert accuracy(gold, pred, oos_label="oos") == pytest.approx(3 / 5)


def test_without_an_oos_label_abstention_is_always_wrong():
    assert per_item_correct(["a", "oos"], [A, A]) == [False, False]


def test_lengths_must_match():
    with pytest.raises(ValueError):
        accuracy(["a"], ["a", "b"])


def test_macro_f1_hand_computed():
    # a: P = 1/1, R = 1/2 -> 2/3.  b: P = 2/3, R = 1 -> 4/5.  Mean = 11/15.
    assert macro_f1(["a", "a", "b", "b"], ["a", "b", "b", "b"]) == pytest.approx(11 / 15)


def test_macro_f1_averages_over_gold_classes_and_maps_abstention_to_oos():
    # Abstentions count as "oos": a gets F1 0; oos: P = 1/2, R = 1 -> 2/3. Mean = 1/3.
    assert macro_f1(["a", "oos"], [A, A], oos_label="oos") == pytest.approx(1 / 3)
    # A class that is only predicted (never gold) is not averaged in.
    assert macro_f1(["a", "a"], ["a", "z"]) == pytest.approx(2 / 3)


def test_ece_zero_when_perfectly_calibrated():
    assert ece([0.8] * 10, [True] * 8 + [False] * 2, n_bins=10) == pytest.approx(0.0, abs=1e-12)


def test_ece_of_an_overconfident_model():
    assert ece([1.0] * 4, [True, False, True, False], n_bins=15) == pytest.approx(0.5)


def test_ece_bins_are_right_closed():
    # 0.8 belongs to (0.7, 0.8] together with 0.75: |0.5 - 0.775| = 0.275.
    # Left-closed bins (A.3's code) would split them: 0.5*0.2 + 0.5*0.75 = 0.475.
    assert ece([0.8, 0.75], [True, False], n_bins=10) == pytest.approx(0.275)


def test_ece_rejects_bad_input():
    with pytest.raises(ValueError):
        ece([1.2], [True], n_bins=10)
    with pytest.raises(ValueError):
        ece([], [], n_bins=10)
