---
name: new-spec
description: Author a new .fsl spec and take it through the repo's non-vacuity gate before calling it done — fslc check, then verify, then --engine induction, then a hollowness check via fslc mutate kill-rate / --vacuity. Use when creating or substantially changing a spec under specs/ or examples/.
---

# new-spec — author a spec that actually says something

A spec that verifies is worthless if it verifies *vacuously*. This skill is the gate a
new or changed `.fsl` should pass. Read `skills/fsl/reference.md` before writing syntax —
FSL is not in training data, so write from the reference, not memory.

## Gate

1. **Parse + types (fast loop).** `.venv/bin/python -m fslc check <spec>`. Fix errors
   here first; this is the tight iteration signal.
2. **Bounded model check.** `… verify <spec> --depth 8` (raise depth as needed). Read the
   JSON: `verified` with a witness, or a shortest counterexample to reason about.
3. **Unbounded proof where it applies.** `… verify <spec> --engine induction` for an
   infinite-depth guarantee (supply a `decreases` ranking / `invariant` if induction needs
   it; `sum(x: T of …)` is allowed in a `decreases` measure).
4. **Non-vacuity — the step people skip.** A green spec can be hollow (a dead ghost, a
   tautological invariant). `--vacuity` catches some cases but misses dead-ghost
   tautologies, so gate on **mutation kill-rate**: `… mutate <spec>`. A very low kill-rate
   (roughly <10%) means the spec barely constrains anything — strengthen the invariants
   until mutants die.

## Rules

- **Never hollow out a spec to dodge a counterexample.** Weakening an invariant to go
  green defeats the purpose. If a counterexample is real, fix the modeled behavior, not
  the property.
- If a spec lives under `specs/` or `examples/`, changing it will move the corpus snapshot
  (`tests/test_corpus_snapshot.py`). Review the diff; regenerate only if the behavior
  change is intended (`FSLC_SNAPSHOT_UPDATE=1 .venv/bin/python -m pytest tests/test_corpus_snapshot.py -q`).
- If FSL is the wrong tool for what you're modeling, say so and recommend ordinary tests
  instead of forcing a spec (see the self-check in `skills/fsl/SKILL.md`).
