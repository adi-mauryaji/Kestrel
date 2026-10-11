from bench_e0 import busy_pct, cpu_mask, freq_summary, one_cpu_per_core


def test_busy_pct_counts_only_non_idle_time():
    # 100 jiffies elapsed, 25 of them busy -> 25 %
    assert busy_pct((1000, 4000), (1025, 4100)) == 25.0


def test_busy_pct_is_zero_when_no_time_elapsed():
    assert busy_pct((10, 50), (10, 50)) == 0.0


def test_freq_summary_reports_mean_and_minimum():
    assert freq_summary([3000.0, 2000.0, 4000.0]) == (3000.0, 2000.0)


def test_freq_summary_handles_runs_too_short_to_sample():
    assert freq_summary([]) == (None, None)


def test_one_cpu_per_core_with_adjacent_smt_siblings():
    # This laptop (Ryzen 5 5500U): logical CPUs 2k and 2k+1 share physical core k.
    topo = {cpu: ("0", str(cpu // 2)) for cpu in range(12)}
    assert one_cpu_per_core(topo) == [0, 2, 4, 6, 8, 10]
    assert cpu_mask(one_cpu_per_core(topo)) == "555"


def test_one_cpu_per_core_with_split_smt_siblings():
    # Common Intel layout: CPU k and k+6 are siblings.
    topo = {cpu: ("0", str(cpu % 6)) for cpu in range(12)}
    assert one_cpu_per_core(topo) == [0, 1, 2, 3, 4, 5]
    assert cpu_mask(one_cpu_per_core(topo)) == "3f"


def test_one_cpu_per_core_keeps_cores_of_different_packages_apart():
    topo = {0: ("0", "0"), 1: ("1", "0"), 2: ("0", "0"), 3: ("1", "0")}
    assert one_cpu_per_core(topo) == [0, 1]
