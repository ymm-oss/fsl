# DOGFOOD-10: Fault-Injection Benchmark — Measuring Detector Catch Rate by Type × Mechanism (2026-06-14)

issue #8. We ran the detector suite implemented in #3–#7 by **injecting known errors into correct specs and
measuring what catches what**. An effectiveness measurement that makes roadmap #1's own proposal verifiable.

Harness: `tests/test_injection_bench.py` (extends the gallery's "expectation declaration → JSON match" to multiple
detectors). Corpus: `examples/gallery/injected/` (3 domains × 7 injection kinds = 21 specs. Each spec has a
`// inject:` `// expect-detector:` `// expect-signal:` declaration header). The measured matrix is regenerated into
`examples/gallery/injected/MATRIX.json`.

Domains: `bank` (specs/bank*), `order_workflow` (specs/order_workflow),
`return_system` (the returns domain in examples/layers).

## Result: Catch-Rate Matrix (stable 3/3, exact match with prediction)

Each injection is caught **only in the predicted lane**, and all other detectors pass it through (each cell's
caught/not-caught matches across all 3 domains. `surprises: []` = zero divergence from prediction).

| Injection (type) | verify | --vacuity | --strict-tags | strict-tags +ids | mutate | forbidden/acc |
|---|---|---|---|---|---|---|
| guard over-strengthening (over-constraint 3) | **✓** | – | – | – | – | – |
| invariant with unreachable antecedent (vacuous 5) | – | **✓** | – | – | – | – |
| adding a constraint not in the NL (fabrication 7) | – | – | **✓** | – | – | – |
| dropping a requirement (omission 7) | – | – | **✗** | **✓** | – | – |
| invariant weakening (under-constraint 4) | – | – | – | – | **✓** | – |
| boundary flip `<=↔<` (mistake 6) | – | – | – | – | – | **✓** |
| guard weakening (under-constraint 4) | – | – | – | – | – | **✓** |

(✓ = caught, ✗ = the same detector but the condition is not met so not caught, – = not applicable)

## Insights

- **F18: each detector has a non-overlapping lane.** None of the 7 injections ends up in a "everything catches it"
  state; **exactly one mechanism** caught each. A design that is neither redundant nor leaving gaps was backed up by
  measurement. verify (over-constraint) / vacuity (vacuous) / strict-tags (fabrication) / mutate
  (under-constraint = invariant) / forbidden (under-constraint = guard, mistake) divide up the territory.

- **F19: a mistake (boundary flip) and guard weakening are caught only by an independent channel
  (forbidden/acceptance).** verify, vacuity, strict-tags, and mutate all pass them through. These are
  "internally perfectly consistent but different from intent" errors, and **positive/negative traces written from
  the NL by someone other than the spec's author** are the only net. The reason for #3 forbidden / D4 to exist is
  fixed numerically.

- **F20: pure omission is undetectable in principle without a requirements registry.** The dropped-requirement
  injection is **not caught** with `--strict-tags` (plain), and caught only with `strict-tags +ids` given
  `--requirements ids.txt`. The absence of a requirement the spec never once mentions becomes visible only with an
  external declaration (ids registration / an empty requirement block).

- **F21: invariant weakening is visible only in mutate's delta.** Standalone verify of the weakened spec passes
  (a weaker invariant). It is detected by `fslc mutate`'s survivor count increasing relative to baseline (this
  harness judges a survivor increase from baseline → injected as caught). A single run only says "many survivors",
  so **comparison against baseline is the condition**.

## Holes in the Detection Net (→ remaining territory)

On the matrix "every injection is caught in one lane", but per F19/F20 **it does not close with automation alone**:

1. **Mistake / guard weakening**: forbidden/acceptance is needed, and its positive/negative traces are input
   **written from the NL by a human or an independent agent**. Not auto-generated.
2. **Omission**: a maintained ids registry is the prerequisite.
3. The final bastion for these is **back-translation diff (D5, the skills/fsl workflow)** — an agent that has not
   seen the original text renders the `.fsl` into natural language and reconciles items against the requirements.
   This benchmark fixes the "territory of the automatic detectors" and demonstrates the picture in which the
   **outside** of it is borne by the independent channel / back-translation.

There is **no "uncovered type"** requiring a new issue (every injection has a corresponding lane). However, 1–3
above are not "holes in the detectors" but operating conditions of "**human / independent-channel input is the
prerequisite**", and are already recorded in skills/fsl and #2.

## Out of Scope (future)

The original idea of "measuring generation quality by skill version" (have a different AI formalize with the plain
skill / +memo / +positive-example pair and compare error rates) was separated out from this benchmark because it is
a non-deterministic live experiment (lineage of DOGFOOD-8 blind writability. A separate manual DOGFOOD in the
future).

## Reproduction

```bash
./.venv/bin/python -m pytest tests/test_injection_bench.py -q   # regenerates the matrix into MATRIX.json
```

A calibration asset that can be re-run when the model/detectors are updated. Measures 21 injections × 6 detectors in
about 60 seconds.
