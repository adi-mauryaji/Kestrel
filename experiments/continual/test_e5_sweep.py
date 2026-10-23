import pytest

from cl_head import toy_tasks
from e5_sweep import GRID, METHODS, run_all, select


def test_every_method_gets_twelve_configs_and_the_baseline_is_never_tuned_less():
    assert {m: len(GRID[m]) for m in METHODS} == {"naive": 12, "replay": 12, "ewc": 12}
    pairs = lambda m: {(c["lr"], c["epochs"]) for c in GRID[m]}  # noqa: E731
    assert pairs("replay") <= pairs("naive") and pairs("ewc") <= pairs("naive")


def _summary(acc_by_config, ok=None):
    return [{"config": i, "n": 15, "n_ok": (ok or {}).get(i, 15), "avg_accuracy": {"mean": a}}
            for i, a in enumerate(acc_by_config)]


def test_selection_is_the_best_mean_accuracy_among_complete_configs_ties_to_the_earlier():
    assert select(_summary([0.2, 0.5, 0.4])) == 1
    assert select(_summary([0.5, 0.2, 0.5])) == 0, "a tie goes to the earlier config"
    assert select(_summary([0.2, 0.9, 0.4], ok={1: 14})) == 2, "a config with a failed run is not eligible"
    with pytest.raises(ValueError):
        select(_summary([0.1], ok={0: 0}))


def test_a_small_sweep_end_to_end_on_toy_stages():
    train, val = toy_tasks(n_tasks=3, classes_per_task=3, seed=0)
    sets = {1: (train, val), 2: (train[::-1], val[::-1])}
    grid = {"naive": [{"lr": 0.5, "epochs": 2}, {"lr": 0.5, "epochs": 5}],
            "replay": [{"lr": 0.5, "epochs": 2, "per_class": 5}],
            "ewc": [{"lr": 0.5, "epochs": 2, "lam": 1e3}, {"lr": 0.5, "epochs": 2, "lam": 1e7}]}
    out = run_all(sets, d=20, k=9, grid=grid, seeds=(1, 2), jobs=1)
    assert len(out["runs"]) == (2 + 1 + 2) * 2 * 2 + 2 * 2 * 2, "configs x orders x seeds, plus 2 controls"
    unstable = [r for r in out["runs"] if r["method"] == "ewc" and r["config"] == 1]
    assert unstable and all(r["error"].startswith("unstable") for r in unstable)
    assert out["selected"]["ewc"]["config"] == 0, "the unstable config is never selected"
    assert set(out["selected"]) == {"naive", "replay", "ewc", "joint", "stage0"}
    assert out["selected"]["joint"]["cfg"] == out["selected"]["naive"]["cfg"], "controls use naive's lr/epochs"
    assert out["eps2_forgetting"] >= 0 and out["eps2_runs"] == 4
    ok = next(r for r in out["runs"] if "R" in r)
    assert len(ok["R"]) == 3 and {"avg_accuracy", "avg_forgetting", "bwt", "seconds", "buffer_bytes"} <= set(ok)


def test_the_process_pool_gives_the_same_matrices_as_one_process():
    train, val = toy_tasks(n_tasks=3, classes_per_task=3, seed=1)
    sets = {1: (train, val)}
    grid = {"naive": [{"lr": 0.5, "epochs": 2}], "replay": [{"lr": 0.5, "epochs": 2, "per_class": 5}],
            "ewc": [{"lr": 0.5, "epochs": 2, "lam": 100.0}]}  # 1e3 is past lr * lam * max F < 2 on seed 2
    one = run_all(sets, d=20, k=9, grid=grid, seeds=(1, 2), jobs=1)
    two = run_all(sets, d=20, k=9, grid=grid, seeds=(1, 2), jobs=2)
    key = lambda r: (r["method"], r["config"], r["order"], r["seed"])  # noqa: E731
    assert [(key(r), r["R"]) for r in one["runs"]] == [(key(r), r["R"]) for r in two["runs"]]
