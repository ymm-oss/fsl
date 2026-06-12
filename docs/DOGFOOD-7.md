# DOGFOOD-7: Correctness Oracle Test Suite

## Summary

Added a bounded correctness suite for `fslc` without modifying `src/fslc`.

New collected tests by category:

| Category | Files | Collected |
|---|---:|---:|
| Monitor BFS oracle agreement | `tests/oracle.py`, `tests/test_oracle_agreement.py` | 37 |
| Trace and witness soundness | `tests/test_trace_soundness.py` | 105 |
| Independent refinement oracle | `tests/test_refine_oracle.py` | 11 |
| Metamorphic checks | `tests/test_metamorphic.py` | 5 |
| JSON/CLI robustness | `tests/test_robustness.py` | 3 |
| Total |  | 161 |

Observed new-test execution: `91 passed, 70 skipped in 88.02s`; wall time `88.24s`.
Full suite execution: `299 passed, 70 skipped in 255.13s`; wall time `255.34s`.
The prior suite baseline was 208 passed at about 170s, so the measured increase is about 85s.

## Oracle Scope And Limitations

`tests/oracle.py` is a pure Python bounded oracle that enumerates reachable states by driving
`fslc.runtime.Monitor.enabled()` and `Monitor.step()`. It does not import Z3 or use BMC.

Limitation: BMC encoding bugs are detectable only when they disagree with Monitor's concrete
single-step semantics. Bugs shared by BMC and Monitor step semantics are not detectable by this
oracle. LeadsTo lasso reasoning is also outside finite Monitor replay, so leadsTo traces are
explicitly skipped in trace soundness.

One deterministic corpus spec, `specs/job_pipeline.fsl`, is skipped by the BFS oracle because
`Monitor.enabled()` raises on a guarded `let queue.head()` before enumeration can proceed.

## Hypothesis

`./.venv/bin/python -c "import hypothesis"` failed with `ModuleNotFoundError`.
`tests/test_robustness.py` therefore uses a fixed-seed deterministic generator.

## Mutation Proof

### Historical `refine.py` Mutation

Mutation:

```bash
git show de9d919^:src/fslc/refine.py > src/fslc/refine.py
```

Result:

| Harness | Result |
|---|---|
| `tests/test_refine_oracle.py -q` | Failed: 5 failures. Historical refine reported `refines` for known `refinement_failed` cases, including the depth-short-of-deadlock fixtures. |
| `tests/test_oracle_agreement.py -q` | Failed: 2 failures. Gallery refinement false-negative fixtures reported `refines` instead of `refinement_failed`. |
| Other new harnesses | Not targeted for this mutation. |

Restoration: original `src/fslc/refine.py` was restored from `/private/tmp/fslc_refine_original.py`.

### Monitor Invariant-Check Mutation

Mutation: temporarily changed the Monitor invariant check in `src/fslc/runtime.py` from:

```python
if not _as_bool(cond):
```

to:

```python
if False and not _as_bool(cond):
```

Result:

| Harness | Result |
|---|---|
| `tests/test_oracle_agreement.py tests/test_trace_soundness.py -q` | Failed: 8 total failures. |
| `tests/test_oracle_agreement.py` | Failed: 4 failures where the mutated Monitor oracle missed invariant/type-bound violations reported by BMC. |
| `tests/test_trace_soundness.py` | Failed: 4 failures where BMC traces replayed through mutated Monitor as `ok`. |
| Other new harnesses | Not targeted for this mutation. |

Restoration: original `src/fslc/runtime.py` was restored from `/private/tmp/fslc_runtime_original.py`.

## Final Verification

Commands run after restoring all mutations:

```bash
/usr/bin/time -p ./.venv/bin/python -m pytest tests/oracle.py tests/test_oracle_agreement.py tests/test_trace_soundness.py tests/test_refine_oracle.py tests/test_metamorphic.py tests/test_robustness.py -q
/usr/bin/time -p ./.venv/bin/python -m pytest tests/ -q
git diff -- src/fslc
```

Final `git diff -- src/fslc` was empty.

## Bug Found By This Suite

- **BUG-020 (Monitor robustness)**: `Monitor.enabled()` raises `_PartialOp`
  on `specs/job_pipeline.fsl`, which `fslc verify` proves/verifies cleanly.
  Cause: `enabled()` eagerly evaluates `let j = queue.head()` while testing
  whether `start()` is enabled; in states reachable during enumeration the
  guard `requires queue.size() > 0` should gate it, but the let is evaluated
  before/independent of the guard, so `head()` on a (possibly empty) Seq
  raises instead of the action being treated as simply not-enabled.
  Impact: runtime Monitor / replay / testgen for any spec with a guarded
  partial-op (`head`/`pop`/`at`) inside a `let`. The BFS oracle skips this
  spec for now. Independently reproduced; not fixed in this test-only round.
