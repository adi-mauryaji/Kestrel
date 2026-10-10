from bench_e0 import busy_pct, freq_summary


def test_busy_pct_counts_only_non_idle_time():
    # 100 jiffies elapsed, 25 of them busy -> 25 %
    assert busy_pct((1000, 4000), (1025, 4100)) == 25.0


def test_busy_pct_is_zero_when_no_time_elapsed():
    assert busy_pct((10, 50), (10, 50)) == 0.0


def test_freq_summary_reports_mean_and_minimum():
    assert freq_summary([3000.0, 2000.0, 4000.0]) == (3000.0, 2000.0)


def test_freq_summary_handles_runs_too_short_to_sample():
    assert freq_summary([]) == (None, None)
