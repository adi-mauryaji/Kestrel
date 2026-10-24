# Speaker notes · Kestrel mentor review

What to say on each slide of `kestrel_slides.pdf` (14 slides, about 15 minutes). The slides take every result
number from `paper/generated/numbers.tex`; the numbers below match it at the commit that added this file.

Build the slides: `cd paper/slides && latexmk -pdf kestrel_slides.tex`.

## 1 · Cover (about 45 s)

Kestrel is a personal assistant for notes and tasks that runs entirely on a laptop: nothing leaves the machine. This
talk is about one research question inside it. Can a cascade of cheap models answer most requests, and send only the
hard ones to a local language model, without losing accuracy? Everything I report was pre-registered: the hypotheses,
systems, scoring, measurement protocol and analysis were frozen as a git tag before any test item was read. I will show
what was supported, what was not, and why.

## 2 · The problem (about 1 min)

On this laptop, classifying one request with the 4-bit Gemma-4-E2B model takes 865 ms at the median and keeps about
3.5 GB resident. A frozen multilingual sentence encoder with a linear head answers in about 5 ms with under 300 MB. Most
requests to a notes-and-tasks assistant are simple: "search my notes for budget", "remind me to call Asha tomorrow".
So the question is whether a cascade can answer those cheaply and escalate only the hard ones.

If asked "why not just use the encoder?": it cannot handle multi-step requests or produce plans, and on our own data it
is far weaker (0.702 accuracy), as the results show.

## 3 · The system (about 1.5 min)

A request is normalized, then checked for a conjunction: and, then, aur, phir, fir. If it has one it may carry two
intents, so it goes straight to the language model, which can plan several steps. Otherwise it starts at Tier 1,
keyword rules, whose confidence is their measured precision on the calibration split. If no rule is confident, Tier 3
embeds the request with a frozen multilingual e5-small encoder and applies a linear head, temperature-scaled. If that is
below its threshold, Tier 4, Gemma-4-E2B at 4-bit, decodes under a grammar that only allows label strings; its
confidence is the probability of the chosen label tokens, temperature-scaled by binary cross-entropy. Each tier has its
own threshold tau. The operating point is the cheapest set of thresholds on validation whose accuracy matches the better
of the two tuned single models. Tier 2, a distilled micro-model, was planned as an ablation and not built.

## 4 · The gap (about 1 min)

The closest prior work calibrates model cascades, C3 and CascadeBERT, and CAUC even shares one calibrated threshold
across the cascade, with a budgeted variant tuned on validation, and proves that calibration bounds the accuracy of
accepted answers. C3 also cascades Llama-2 models, not only encoders. But they measure cost in FLOPs, GFLOPs or
parameter counts, not time and memory on a device. Arora et al. route intent detection from an encoder to a cloud LLM,
with an uncalibrated signal. Chaganti and Sadhu measure on devices, but neither has a calibrated multi-tier cascade that
mixes rules, a classifier and a generative model. Nobody in our search tested code-mixed Hinglish in a cost-aware
cascade. Because each piece exists, I make no novelty claim: the contribution is a controlled, pre-registered
evaluation in this setting.

Expected question, "isn't this just CAUC?": CAUC's tiers all emit logits over the same labels, it measures parameter
counts or GFLOPs, and it has no out-of-scope requests or abstention; our RQ2 is explicitly a replication of the
calibrated-threshold idea.

## 5 · Research questions (about 1 min)

The primary hypothesis, H1, is the one the study is powered for: at its validation-chosen operating point, the
calibrated cascade R3 is non-inferior in accuracy to always using the language model, R0, within a margin delta of
0.0263, and it is faster. It is tested on CLINC150, the public benchmark, as an intersection-union test: both parts must
hold. The secondary hypotheses ask whether the confidence signal beats random escalation at the same cost (H1b),
whether the cascade keeps up with the encoder alone (H1c), whether calibration helps (H2), whether the encoder tier
pays off (H3), whether replay prevents forgetting (H5), and whether Hinglish is served as well as English (H7).
Personalization and quantization are pre-registered but wait on the review of their synthetic datasets. Delta is twice
the seed-to-seed standard deviation of the tuned language model's accuracy in a five-seed pilot; it was set before any
test data was touched.

## 6 · Method (about 1.5 min)

Three things make the comparison fair. First, everything was frozen before test: the pre-registration is a git tag,
delta came from a pilot on validation data, and every threshold and temperature was fitted on the calibration and
validation splits only. Second, the baselines get the same tuning effort as the cascade: the language model alone and
the encoder alone each get one abstention threshold tuned on validation, and R-rand escalates at random at the
cascade's own rates, so it costs the same. Third, latency is measured on the laptop under a fixed protocol: cores
pinned, turbo off, five timed repetitions per item after warm-up, systems interleaved, and five pre-declared rules that
would invalidate a run (lost AC power, background load, frequency drift, temperature, a crash or hash mismatch). Every
test run was valid at the first attempt. Scoring: abstaining on an in-scope request counts as an error; on an
out-of-scope request, abstaining is correct. The synthetic set comes from template families, so its bootstrap
resamples whole families.

## 7 · Primary hypothesis H1 (about 1 min)

The primary hypothesis is supported. On CLINC150's 5,500 test requests the cascade was 0.048 more accurate than the
tuned language model, with a 95% confidence interval from 0.038 to 0.058: not just non-inferior but better. Mean
latency per request fell by 717 ms, CI 694 to 739. Only 17% of requests reached the language model, and it answered 6%
of all requests (it abstained on the rest); the median for requests answered below it was about 5 ms. Both parts of
the intersection-union test hold. A pre-registered sensitivity analysis that drops the 14 test items whose text also
appears in another split gives the same interval.

Why is the cascade more accurate than the model it escalates to? Because the encoder head, trained on CLINC150's
15,000 training items, is a better CLINC classifier than a few-shot prompted 2B model; the cascade only sends it what
the encoder is unsure about. That point matters on the next slide.

## 8 · Accuracy against latency (about 1.5 min)

This is the result I want the panel to see clearly. The plot shows accuracy against mean latency on a log scale;
ringed points are not dominated. On CLINC150, the encoder alone, R1, is both more accurate (0.893) and about 25 times
faster than the cascade (0.874 at 146 ms). So the cascade beats the language model, but not the encoder. H1c, which
asked whether the cascade keeps up with the encoder within delta, is inconclusive after Holm. On our synthetic
English-Hinglish set the picture flips: the language model alone is perfect, the encoder alone is weak at 0.702, and
the cascade sits in between at 0.968 for about half the latency. So each single model wins on one dataset and the
cascade wins on neither, but it is never far from the better one. And the cascade does not save memory: it keeps the
encoder loaded next to the language model, so its peak is 3,773 MiB against 3,491.

Expected question, "so why use a cascade?": you do not know in advance which model suits your traffic; the cascade is
a hedge that loses little on either kind. The next slides and the failure analysis explain the CLINC150 gap.

## 9 · Calibration (about 1 min)

RQ2 compares the same cascade with one shared threshold on raw confidences, R2, and on temperature-scaled confidences,
R2c. On CLINC150, calibration lowered selective risk at 80% coverage from 0.075 to 0.054, and the encoder's expected
calibration error fell from 0.055 to 0.008. H2 is supported there. On the synthetic set it did not replicate: the
difference is +0.005 with an interval that includes zero, and the encoder's temperature, fitted on only 100 synthetic
calibration items, barely changed its error.

Why does a temperature help at all? It never changes a tier's predicted label, but it does reorder confidences across
requests: between the encoder's raw and calibrated confidences, Kendall's tau is 0.92 on CLINC150. Within each tier,
risk at 80% coverage still barely moved, so its gain is making the tiers' confidences comparable when one threshold is
applied across them. With per-tier thresholds, as in R3, it brought no saving: in an exploratory check after the test
results, the validation search on raw confidences reached the same accuracy more cheaply, 128 against 140 ms on
CLINC150. Our pre-registration had said a temperature cannot move that frontier; that was wrong, and the paper says so.
So H2 is a statement about abstention quality. This is a replication of the calibrated-threshold idea of C3 and CAUC,
and one of the two replications failed.

## 10 · Tiers and forgetting (about 1.5 min)

Two more questions. RQ3, the tier ablation on our synthetic set: adding the encoder to rules plus the language model
cut mean cost by 289 ms per request, and accuracy stayed within delta, so H3 is supported. Interestingly, the rules
alone cost accuracy on test: rules plus the language model scored 0.976 against 1.000 for the language model alone.
The failure analysis shows why.

RQ5 asks what happens when new intents arrive and the encoder's head is updated in stages: ten CLINC150 domains in five
stages. Naive updates forget almost everything learned earlier, average forgetting 0.947. Replay of 50 stored items per
earlier class removes the forgetting and gets within a point of joint training. EWC did not help at any strength we
could use: the Fisher information was tiny and SGD's stability bound capped the penalty. H5, replay against naive, is
supported. The price is privacy as much as memory: the replay buffer keeps embeddings of half of every old class's
training items, and embeddings derived from users' requests should be treated as personal data.

## 11 · All hypotheses (about 1 min)

Here is every pre-registered test. The primary is supported. Of the 13 secondary members, six are supported, five
inconclusive, none is "not supported", meaning no interval landed wholly on the wrong side, and two are pending.
Pending members enter Holm's method with p equal to 1, which makes the adjustment conservative: a "supported" now
cannot be lost when H4 and H6 arrive, but an "inconclusive" could still become supported. H1c on CLINC150 and H7a are
individually below 0.05 (0.045 and 0.036) but not after adjustment. The Hinglish results are inconclusive mainly
because 126 requests per language give wide intervals.

Expected question, "the dataset assignment for H1b, H1c and H2 was clarified after the freeze?": yes, it is disclosed;
at that point E5 and one E2 result had been seen, and both datasets were kept in the family, which is the conservative
choice.

## 12 · Failure analysis (about 1.5 min)

Two mechanisms explain the losses; both were found after the results were read, so they are exploratory. On CLINC150,
474 of the cascade's 692 errors, 68%, are out-of-scope requests that it answered with an in-scope intent. The operating
point accepts the encoder at confidence 0.601, the cheapest point that met the accuracy floor on validation, while the
encoder alone abstains below 0.782. 149 out-of-scope test requests fall between those two thresholds, and the cascade
answers them. Out-of-scope requests are 11.8% of validation but 18.2% of the official test set, so a threshold tuned
for cost on validation under-weights them. Part of the loss is that shift in the out-of-scope rate. Whether the encoder
is also miscalibrated on out-of-scope requests is open: its good calibration on test, ECE 0.008, is measured on
in-scope items only, and out-of-scope requests have no class in its head.

On the synthetic set, 6 of the cascade's 8 errors are two-part requests joined by "also" or ", now", which the
multi-intent signal does not know, so a single-intent rule took them. Every validation compound had a signal word, so
tuning could not see the gap. Fixes are clear, but changing the system now would be a post-hoc change, so they go into
a new, pre-registered version.

## 13 · Limitations (about 1 min)

What these results do not show. Everything is from one laptop CPU with one language model and one encoder, with turbo
boost off, so absolute latencies are conservative and other hardware may change the ratios. Kestrel-Intents-v0 is
synthetic, drafted with an AI assistant and reviewed by me, and its inter-annotator agreement has not been measured
yet: the second annotator is pending. Hinglish appears only in that set, 126 requests per language. Personalization
(E4) and quantization (E6) wait on the review of their synthetic datasets. Latency excludes model loading and is
measured on a warm, single-user process. Every analysis reproduces byte for byte from a clean checkout on this machine,
but a second machine has not reproduced a result yet; I will do that next week on a Mac. The margin delta was frozen
before the smallest effect of interest was confirmed with you; if you set it above 0.0263, H1, H1c, H3 and H7a are also
reported at your margin as a sensitivity analysis, and the primary test stays at 0.0263. Energy could not be measured
without root. And the privacy checks (no network calls in the tested scenarios, a canary absent from logs and deleted
from the database) are tests, not a formal guarantee: nothing confines the process at kernel level.

## 14 · Conclusion (about 1 min, then questions)

To answer the research questions: RQ1, yes against the language model on CLINC150, more accurate and 717 ms faster per
request, but not against the encoder alone, which dominated the cascade there. RQ2, calibration lowers selective risk
on CLINC150 but did not replicate on our synthetic set. RQ3, the encoder tier saves 289 ms per request with accuracy
within the margin. RQ5, replay prevents forgetting and EWC does not. RQ7, Hinglish is inconclusive with this sample
size. The practical lesson is that the cascade's advantage depends on traffic, especially the out-of-scope share, so
the operating point should be chosen on data that matches deployment. Next steps: review the Tasks and Personas
datasets so E4 and E6 can be frozen by amendment and run, which completes the Holm family; reproduce on a second
machine next week; get a second annotator for agreement; and confirm the smallest effect of interest with you.

Likely questions and short answers:

- *Why is the cascade more accurate than R0?* The encoder head is a better CLINC classifier than the few-shot 2B
  model; the cascade escalates only uncertain items.
- *Is the synthetic data a problem?* Yes for external validity; that is why H1 is on CLINC150 and why Intents-v0
  resamples template families.
- *Did you tune on test?* No: everything was frozen as a git tag before any test item was read, and the reruns
  reproduce byte for byte.
- *Why no energy?* RAPL needs root on this machine.
