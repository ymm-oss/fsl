# FSL — dialect corpus conformance harness (Monitor / oracle / agreement CI gate)

## Goal

Every `.fsl` under `specs/` and `examples/` is either (a) driven through the full
dual-evaluator safety net — `parse → desugar → build_spec → Monitor load →
BMC/Monitor expression agreement → verify-vs-oracle verdict agreement` — or (b)
excluded **loudly**, with a documented reason that the harness re-asserts on every
run. A new dialect (or a new example directory) that nobody registers is a CI
failure, not a silent skip.

## The gap (issue #167)

The 2026-07-08 audit found 15 of 18 `examples/db/*.fsl` failing Monitor load
(`_check_deterministic_init` was type-blind for per-key map init; fixed in
`470c75c`) while `pytest -q` stayed green: `tests/test_oracle_agreement.py`
scans only `specs/*.fsl` + `examples/gallery/{valid,errors}`, and
`tests/test_evaluator_agreement.py` only `specs/*.fsl`. Both `pytest.skip` when
`can_monitor()` fails. So an entire dialect corpus sat outside the core
correctness invariant and nothing said so. Skips are the bug this design removes.

## Registry — `tests/dialect_registry.py`

Declarative, no logic. The harness scans `SCAN_ROOTS = ("specs", "examples")`
exhaustively; the registry says what may exist there.

- `DIALECTS: dict[str, Dialect]` — `Dialect(construct, min_files, depth=4)` per
  frontend: `kernel` (`spec` — the design layer writes kernel specs), `business`,
  `requirements`, `governance`, `compose`, `db` (`dbsystem`), `domain`, `ai`
  (`ai_component`). `construct` is the file's top-level keyword; `min_files` is a
  glob-rot floor (the scan must keep finding at least that many — a corpus that
  shrinks under its floor fails, so coverage cannot narrow silently); `depth`
  bounds the BFS/verify agreement stages.
- `EVIDENCE_CONSTRUCTS: dict[str, str]` — construct → reason, for whole file
  kinds that have **no kernel expansion by design**: `ai-project`
  (`is_ai_project_source`; external statistical evidence, `fslc ai
  eval/regress/drift/compat`, `formal_result:"not_run"`) and `ai-agent`
  (`is_ai_agent_source`; structural analysis, `agent_analyzed`, not formal proof).
- `MONITOR_EXCLUSIONS: dict[str, str]` — repo-relative path → reason, for
  individual files the frozen Python Monitor legitimately rejects. Each entry
  names its active native or BMC-side coverage, and a stale entry fails the
  harness once the Monitor starts accepting it.

## Classification (automatic, in the harness)

`classify(path)` reads the source and returns one of:

1. `EXCLUDED` — `is_ai_project_source` / `is_ai_agent_source` match, or path in
   `MONITOR_EXCLUSIONS`.
2. `REFINEMENT` — top-level keyword `refinement` (mapping files are not state
   machines; refine semantics are covered by `test_refine*.py` and the
   refinement fixtures in `test_oracle_agreement.py`).
3. `DECLARED_ERROR` — front matter `// expected-result: error` (gallery error /
   adversarial fixtures that must fail at parse/type/semantics/acceptance).
4. `INJECTED` — `// inject:` / `// expect-detector:` front matter
   (`examples/gallery/injected/`, the detector benchmark corpus of
   `test_injection_bench.py`).
5. `CONFORMANCE` — everything else; the top-level keyword must match a
   `DIALECTS` construct. **An unknown construct fails the run** with
   "register the new dialect in tests/dialect_registry.py".

## Pipeline stages and failure semantics — `tests/test_dialect_conformance.py`

One parametrized test per class; every obligation is an `assert`, never a skip.

| Class | Obligation |
|---|---|
| CONFORMANCE / INJECTED | full pipeline below |
| REFINEMENT | `parse_src` succeeds and `ast[0] == "refinement"` |
| DECLARED_ERROR | build or `run_verify` still yields `result:"error"` — a fixture that starts passing is a stale declaration and fails |
| EXCLUDED | the documented reason still holds (Monitor load still raises / construct still matches) — a stale exclusion fails and must be deleted |

Full pipeline per file (depth from the file's dialect entry, default 4):

1. **Load** — `Monitor(path)` (= `parse_src` → dialect desugar → `build_spec` →
   `_check_deterministic_init`), then `reset()` + `enabled()`. Any raise fails.
2. **Explore** — `bfs_oracle(path, depth, collect_phys=EXPR_STATES)`; the BFS is
   extended to also return the first `EXPR_STATES = 40` unique `_phys`
   snapshots, so one exploration feeds both agreement stages.
3. **Expression agreement** — for each snapshot, pin the symbolic
   `bmc.make_state` to the concrete values in a z3 solver (unsat pin = failure)
   and compare `bmc.eval_expr` vs `runtime.eval_concrete` on every invariant and
   reachable. Any mismatch fails (shared helpers factored into
   `tests/agreement.py`, reused by `test_evaluator_agreement.py`).
4. **Verdict agreement** — `run_verify(path, depth, deadlock_mode="warn")`
   against the oracle, same decision table as `test_oracle_agreement.py`
   (factored into `oracle.assert_verdict_agrees`): oracle violation ⇒ `violated`
   with matching kind and minimal step; unreached reachables ⇒
   `reachable_failed`; else `verified`/`proved` (finite `leadsTo`
   counterexamples excepted — the oracle has no lasso check). INJECTED files may
   additionally return `error` with kind `acceptance`/`forbidden` (declared
   detector outcomes the oracle does not model). Any *undeclared* `error` fails.

Two meta-tests close the structural hole: `test_corpus_fully_claimed` (no
UNKNOWN construct anywhere under `SCAN_ROOTS`) and `test_registry_floors`
(per-dialect scan count ≥ `min_files`; also asserts every `MONITOR_EXCLUSIONS`
path exists). Regression for the gate itself: reverting `470c75c` locally makes
the db corpus fail stage 1 loudly (verified once at PR time; the assert-not-skip
structure keeps it true).

## Cost and CI wiring

Measured on the current corpus (175 `.fsl`, 148 monitorable): BFS depth 4 ≈ 56 s
(worst file 4.5 s), verify depth 4 on the 104 previously-uncovered files ≈ 29 s;
with pinning and the covered files the whole harness projects to ≈ 3 min
single-threaded. Bounds are explicit constants (`depth` per dialect,
`EXPR_STATES`) — raising coverage is a registry diff, not a hidden loop change.

This harness belongs to the frozen Python reference implementation and is no
longer run by `.github/workflows/ci.yml`. It remains available for manual
historical/reference checks; active CI coverage is provided by the Rust
workspace tests and WASM browser validation.

The external-compiler conformance surface introduced by issue #208 is separate
from this historical corpus gate. Native `fslc conformance` emits versioned,
language-neutral Monitor vectors from any checked/lowered model, including
disabled and rollback-failure outcomes. Its schema and golden corpus are defined
in [`DESIGN-kernel-contract.md`](DESIGN-kernel-contract.md) and are active Rust CI
inputs.

## Exclusion policy

- No `pytest.skip` anywhere in the harness. Every non-conformance file is a
  *classified* parametrized case whose classification is itself asserted.
- Path exclusions carry a reason string that appears in the test id; adding one
  is a reviewable registry diff, and a stale one (the file starts loading) fails.
- External-evidence artifacts (`.jsonl`/`.json`/`.sql`/`.prisma` fixtures for
  `fslc ai`/`db import`/domain replay) are out of scope by extension — the scan
  is `*.fsl` only.

## Coupled changes

`CONTRIBUTING.md` "Adding a language feature" gains: register any new dialect's
construct and example corpus in `tests/dialect_registry.py` (and any new example
directory is claimed automatically by the scan — the harness fails until its
construct is registered).

## Non-goals

- Consolidating `test_oracle_agreement.py` / `test_evaluator_agreement.py` into
  the harness (they keep their deeper declared-depth cases; overlap ≈ 1 min).
- Verifying declared verdicts (`expected-result: proved` etc.) — that stays in
  `test_gallery.py`; the harness checks evaluator *agreement*, not spec intent.
- Refinement-checking the mapping files (covered by `test_refine*.py`).
- Making Monitor accept no-action specs or project-level fsl-ai files.
