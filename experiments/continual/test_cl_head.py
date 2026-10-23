import numpy as np
import pytest

from cl_head import Head, STRATEGIES, run_sequence, softmax, stages, toy_tasks


def test_ewc_gradient_matches_finite_differences():
    rng = np.random.default_rng(0)
    X = rng.normal(size=(30, 5))
    y = rng.integers(0, 3, 30)
    h = Head(5, 3)
    h.W = rng.normal(size=(5, 3)) * 0.3
    h.b = rng.normal(size=3) * 0.1
    ewc = {"lam": 7.0, "F": rng.random((5, 3)), "W_star": rng.normal(size=(5, 3)) * 0.3}

    def loss(W, b):
        P = softmax(X @ W + b)
        nll = -np.log(P[np.arange(30), y]).mean()
        return nll + 0.5 * ewc["lam"] * (ewc["F"] * (W - ewc["W_star"]) ** 2).sum()

    gW, gb = h.grad(X, y, ewc)
    eps = 1e-6
    for i, j in [(0, 0), (2, 1), (4, 2)]:
        Wp, Wm = h.W.copy(), h.W.copy()
        Wp[i, j] += eps
        Wm[i, j] -= eps
        assert abs((loss(Wp, h.b) - loss(Wm, h.b)) / (2 * eps) - gW[i, j]) < 1e-6
    for j in range(3):
        bp, bm = h.b.copy(), h.b.copy()
        bp[j] += eps
        bm[j] -= eps
        assert abs((loss(h.W, bp) - loss(h.W, bm)) / (2 * eps) - gb[j]) < 1e-6


def test_fisher_is_nonnegative_and_shaped():
    rng = np.random.default_rng(1)
    h = Head(4, 3)
    F = h.fisher_diag(rng.normal(size=(50, 4)), rng.integers(0, 3, 50))
    assert F.shape == (4, 3) and (F >= 0).all()


def test_training_is_reproducible_by_seed_and_the_seed_matters():
    rng = np.random.default_rng(2)
    X, y = rng.normal(size=(200, 6)), rng.integers(0, 4, 200)

    def trained(seed):
        h = Head(6, 4)
        h.fit(X, y, epochs=3, lr=0.5, batch=16, seed=seed)
        return h.W

    assert np.array_equal(trained(1), trained(1))
    assert not np.allclose(trained(1), trained(2)), "minibatch order is the seeded part"


def test_stages_are_pairs_of_domains_in_seeded_orders():
    domains = [f"d{i}" for i in range(10)]
    canonical = stages(domains, None)
    assert canonical == [["d0", "d1"], ["d2", "d3"], ["d4", "d5"], ["d6", "d7"], ["d8", "d9"]]
    a, b = stages(domains, 1), stages(domains, 2)
    assert a == stages(list(reversed(domains)), 1), "depends on the names, not their order"
    assert a != b and sorted(sum(a, [])) == domains
    with pytest.raises(ValueError):
        stages(domains[:9], None)


def test_replay_keeps_a_fixed_number_per_class_of_every_past_stage():
    train, evals = toy_tasks(n_tasks=3, classes_per_task=3, seed=0)
    out = run_sequence("replay", train, evals, d=20, k=9, cfg={"lr": 0.5, "epochs": 5, "per_class": 4}, seed=0)
    assert out["buffer_items"] == 4 * 3 * 2, "per class, for the 2 stages before the last"
    assert out["buffer_bytes"] == out["buffer_items"] * (20 * 4 + 8)


def test_the_matrix_and_the_toy_smoke_check():
    # Pipeline check on toy class-incremental data, NOT a finding (A.14's smoke run: naive ~0.10).
    # Naive forgets; replay much less; joint is the ceiling; the stage-0 model never learns later
    # tasks. The toy Fisher is ~1e-5, so EWC needs lam ~1e3 to act at all (and lr * lam * max F
    # must stay below 2, or SGD diverges): the sweep's lam grid comes from the real Fisher's scale.
    train, evals = toy_tasks(n_tasks=3, classes_per_task=3, seed=0)
    cfg = {"lr": 0.5, "epochs": 10, "per_class": 10, "lam": 1000.0}
    res = {s: run_sequence(s, train, evals, d=20, k=9, cfg=cfg, seed=0) for s in STRATEGIES}
    for s, out in res.items():
        R = np.asarray(out["R"])
        assert R.shape == (3, 3) and ((R >= 0) & (R <= 1)).all(), s
        assert out["seconds"] >= 0
    naive = res["naive"]["avg_forgetting"]
    assert naive > 0.05
    assert res["replay"]["avg_forgetting"] <= naive / 2
    assert res["ewc"]["avg_forgetting"] < naive
    assert res["joint"]["avg_accuracy"] >= res["replay"]["avg_accuracy"] - 0.05
    assert res["stage0"]["R"][2][2] < 0.5 and res["stage0"]["avg_forgetting"] == 0.0
    tiny = run_sequence("ewc", train, evals, d=20, k=9, cfg=cfg | {"lam": 10.0}, seed=0)
    assert tiny["R"] == res["naive"]["R"] or abs(tiny["avg_forgetting"] - naive) < 0.01
    with pytest.raises(KeyError):
        run_sequence("magic", train, evals, d=20, k=9, cfg=cfg, seed=0)


def test_an_unstable_ewc_step_raises_instead_of_scoring_garbage():
    # SGD on (lam/2) F (W - W*)^2 is stable only if lr * lam * F < 2; past that the weights grow
    # geometrically (finite for a while, meaningless at once). The NLL gradient is bounded, so a
    # huge lr alone does not diverge.
    train, evals = toy_tasks(n_tasks=3, classes_per_task=3, seed=0)
    with pytest.raises(FloatingPointError, match="unstable"):
        run_sequence("ewc", train, evals, d=20, k=9, cfg={"lr": 0.5, "epochs": 3, "lam": 1e7}, seed=0)
    ok = run_sequence("ewc", train, evals, d=20, k=9, cfg={"lr": 0.5, "epochs": 3, "lam": 1e3}, seed=0)
    assert ok["avg_accuracy"] > 0.5
