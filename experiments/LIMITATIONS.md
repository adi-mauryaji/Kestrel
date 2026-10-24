# Limitations (Week 15; blueprint Ch. 54 "Limitations documentation")

What the results in `RESULTS.md` do **not** show, with where each point comes from.

**Data**
- **Kestrel-Intents-v0 is synthetic.** Claude drafted it from templates and the human reviewed it (v0). Its 252 test
  items come from 44 template families, so the bootstrap resamples families (ADR-029). Inter-annotator agreement (κ)
  has not been measured yet: the second annotator is pending (Request Lab), and the labels were frozen without it
  (ADR-031).
- **Hinglish exists only in Kestrel's own data.** No public code-mixed intent benchmark was used. With n = 126 per
  language, the per-language intervals are wide (H7a and H7b are inconclusive).
- **Kestrel-Tasks-v0, -Personas-v0 and -Memory-v0 are unreviewed drafts.** E4 and E6 have not run (ADR-031, ADR-033).

**Scope of the system tested**
- One machine (Ryzen 5 5500U, 6 cores, 14 GB, Linux), one SLM in the cascade (Gemma-4-E2B Q4_0) and one encoder
  (multilingual-e5-small Q8_0). Turbo boost was off, so latencies are conservative (ADR-012). The quantization
  comparison (E6) is pending.
- Latency is per request on a warm, pinned, single-user process, with model load excluded (27–131 s at start, E1
  meta). Tier 2 was not built, so H3's Tier-2 clause is untested (Ch. 52.1).
- Energy was not measured: RAPL is root-only on this machine (PREREG §3).
- `kestrel ask --notes DIR` carries requests out on a notes folder through the sandbox (since 83034fe), but that
  working application was not part of any measured experiment: the evaluations score routing, and the tool runner
  only in development runs on the Kestrel-Tasks-v0 fixtures.

**Analysis**
- The Holm family's adjusted p-values are interim until H4 and H6 have theirs. An interim "supported" is final; an
  interim "inconclusive" may still change (family.py).
- The family's datasets for H1b, H1c and H2 were fixed after the freeze (clarification v1.0.1, ADR-032). At that
  point E5 and Intents-v0's E2 had already been seen; both datasets were kept in the family, and this is disclosed.
- δ = 0.0263 was frozen before the mentor confirmed the smallest effect of interest (ADR-031). A larger SESOI would
  make it a sensitivity analysis.
- The cascade's operating point was chosen on val. Out-of-scope requests are 11.8% of val but 18.2% of CLINC150 test,
  and they account for 68% of R3's CLINC150 errors (ERROR_ANALYSIS.md, exploratory).
- The multi-intent signal knows only and / then / aur / phir / fir. Compound requests joined by "also" or ", now" are
  missed: 6 of R3's 8 Kestrel-Intents-v0 errors (ERROR_ANALYSIS.md).

**Privacy and security** (Ch. 42, `logs/privacy/`)
- "No network" is shown for the tested scenarios, on this OS, at default logging (no connect() under a network
  namespace, a canary absent from the logs, and deleted from the database). It is not a formal privacy guarantee.
  Nothing confines the process at kernel level (no Landlock or seccomp), and debug logs, swap, crash dumps and
  backups are not covered.

**Reproducibility** (`README_REPRO.md`)
- A clean checkout on this machine reproduces every analysis byte for byte, and E5's numbers. A second machine or
  person has not reproduced a result yet. The Tier-3 heads are not in git, and whether retraining gives the same
  bytes on another platform has not been checked.
