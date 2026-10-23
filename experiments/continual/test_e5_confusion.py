import pytest

from e5_confusion import shares


def cell(c, n, o):
    return {"correct": c, "newer": n, "older": o}


def test_shares_average_the_old_stages_after_the_last_and_after_the_next_stage():
    # 3 stages. Old stages are 0 and 1. After the last stage: stage 0 (0.2, 0.7, 0.1), stage 1
    # (0.6, 0.3, 0.1). Right after the next stage: C[1][0] (0.5, 0.4, 0.1), C[2][1] = stage 1 above.
    C = [[cell(1, 0, 0), cell(0, 0, 1), cell(0, 0, 1)],
         [cell(0.5, 0.4, 0.1), cell(0.9, 0, 0.1), cell(0, 0, 1)],
         [cell(0.2, 0.7, 0.1), cell(0.6, 0.3, 0.1), cell(0.9, 0, 0.1)]]
    runs = [{"method": "naive", "C": C}, {"method": "naive", "C": C}]
    out = shares(runs)["naive"]
    assert out["after_last"] == pytest.approx({"correct": 0.4, "newer": 0.5, "older": 0.1})
    assert out["after_next"] == pytest.approx({"correct": 0.55, "newer": 0.35, "older": 0.1})
    assert out["runs"] == 2
