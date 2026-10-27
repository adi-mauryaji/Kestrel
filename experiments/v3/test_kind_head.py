import numpy as np

from kind_head import cross_fit_logits, folds


def rows():
    # 3 strata; families of 2 rows each
    out = []
    for s, n in (("a", 6), ("b", 5), ("c", 2)):
        for g in range(n):
            out += [{"group": f"{s}{g}", "stratum": s, "label": s}] * 2
    return out


def test_folds_keep_families_whole_and_spread_each_stratum():
    rs = rows()
    f = folds(rs, k=3, seed=0)
    assert len(f) == len(rs) and set(f) <= {0, 1, 2}
    home = {}
    for r, i in zip(rs, f):
        assert home.setdefault(r["group"], i) == i, r["group"]
    for s in "ab":
        per = [len({r["group"] for r, i in zip(rs, f) if r["stratum"] == s and i == k}) for k in range(3)]
        assert max(per) - min(per) <= 1, (s, per)
    assert folds(rs, k=3, seed=0) == f, "seeded: the same every time"


def test_out_of_fold_logits_come_from_heads_that_never_saw_the_fold():
    from head import logits, train_head
    rng = np.random.default_rng(0)
    y = np.array([0, 1, 2] * 10)
    x = rng.normal(size=(30, 4)) + np.eye(3, 4)[y] * 2
    f = [(i // 3) % 3 for i in range(30)]  # every class in every fold
    z = cross_fit_logits(x, y, f, 3, 1.0)
    for k in range(3):
        m = np.array(f) == k
        w, b = train_head(x[~m], y[~m], 3, 1.0)
        assert np.allclose(z[m], logits(x[m], w, b))


def test_each_kinds_version_has_its_own_log():
    from kind_head import log_for
    assert log_for("kinds_v1").name == "final_kinds_v1.json"
    assert log_for("kinds_v2").name == "final_kinds_v2.json"
