from results import render

LAT = {"n": 20, "mean": 400.0, "median": 300.0, "p95": 900.0}
SYS = {"accuracy": 0.95, "macro_f1": 0.9, "coverage": 1.0, "accuracy_answered": 0.95,
       "compound_accuracy": None, "latency_ms": LAT, "peak_rss_mib": 3700.0, "tier4_share": 0.4}
E1 = {"clinc150": {"n_items": 5500, "n_latency_items": 1100, "delta": 0.0263, "clusters": None,
                   "systems": {"R0": SYS, "R0 untuned": SYS, "R1": SYS, "R3": SYS},
                   "h1": {"accuracy": {"diff": 0.001, "ci95": [-0.004, 0.006]},
                          "latency": {"diff_ms": -600.0, "ci95": [-650.0, -550.0]},
                          "outcome": "supported", "p_iut": 0.0}}}
FAMILY = {"members": [{"id": "H5", "p": 6.1e-5, "p_holm": 0.0008, "outcome": "supported", "source": "E5 CLINC150"},
                      {"id": "H4", "p": None, "p_holm": None, "outcome": "pending"}]}


def test_the_report_has_the_primary_result_table_r1_and_the_family():
    md = render(E1, {}, {}, None, FAMILY)
    assert "H1 (primary, CLINC150): **supported**" in md
    assert "| R3 | 0.950 |" in md and "3700" in md
    assert "| H5 | 6.1e-05 | 0.0008 | supported | E5 CLINC150 |" in md
    assert "| H4 | — | — | pending |" in md
    assert "not run yet" in md  # E2, E3, E5 sections without logs say so
