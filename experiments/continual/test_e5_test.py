from e5_test import jobs_for, summarize_test

SELECTED = {"naive": {"lr": 10.0, "epochs": 10}, "replay": {"per_class": 50, "lr": 3.0, "epochs": 10},
            "ewc": {"lam": 100.0, "lr": 3.0, "epochs": 10}, "joint": {"lr": 10.0, "epochs": 10},
            "stage0": {"lr": 10.0, "epochs": 10}}


def test_every_selected_configuration_runs_on_every_order_and_seed():
    jobs = jobs_for(SELECTED, orders=("1", "2", "3"), seeds=(1, 2, 3, 4, 5), d=384, k=150)
    assert len(jobs) == 5 * 3 * 5
    assert jobs[0] == ("naive", 0, SELECTED["naive"], "1", 1, 384, 150)
    assert {j[0] for j in jobs} == set(SELECTED) and all(j[2] == SELECTED[j[0]] for j in jobs)


def test_summary_per_method():
    runs = [{"method": "naive", "config": 0, "cfg": SELECTED["naive"], "order": o, "seed": s,
             "avg_accuracy": 0.2, "avg_forgetting": 0.9, "bwt": -0.9, "seconds": 1.0, "buffer_bytes": 0}
            for o in ("1", "2") for s in (1, 2)]
    out = summarize_test(runs, SELECTED)
    assert out["naive"]["runs"] == 4 and out["naive"]["avg_forgetting"]["mean"] == 0.9
    assert out["replay"] is None  # no runs
