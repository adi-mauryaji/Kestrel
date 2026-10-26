import numpy as np
import pytest

from train import choose_c, label_index


def test_label_index_maps_known_labels_and_sets_aside_the_rest():
    rows = [
        {"text": "a", "label": "create_task"},
        {"text": "b", "label": "summarize_notes|create_task"},  # compound: val/test only
        {"text": "c", "label": "oos"},  # CLINC150 out-of-scope: no head class
        {"text": "d", "label": "search_notes"},
    ]
    keep, y, other = label_index(rows, ["search_notes", "create_task"])
    assert keep == [0, 3]
    assert y.tolist() == [1, 0]
    assert other == [1, 2]


def test_label_index_refuses_unknown_labels_when_strict():
    with pytest.raises(ValueError, match="translate"):
        label_index([{"text": "x", "label": "translate"}], ["create_task"], strict=True)


def test_choose_c_takes_the_best_score_and_the_smaller_c_on_ties():
    assert choose_c({0.1: 0.80, 1.0: 0.90, 10.0: 0.85}) == 1.0
    assert choose_c({1.0: 0.90, 10.0: 0.90, 100.0: 0.89}) == 1.0  # stronger regularization wins ties
    assert choose_c({100.0: 0.5, 10.0: 0.5}) == 10.0
    assert np.isfinite(choose_c({1.0: 0.0}))


def test_per_class_counts_precision_and_recall_from_predictions():
    from train import per_class
    y = np.array([0, 0, 1, 1, 2])
    pred = np.array([0, 1, 1, 1, 0])
    assert per_class(y, pred, ["app", "chat", "email"]) == {
        "app": {"n": 2, "precision": 0.5, "recall": 0.5},
        "chat": {"n": 2, "precision": 2 / 3, "recall": 1.0},
        "email": {"n": 1, "precision": 0.0, "recall": 0.0},
    }
