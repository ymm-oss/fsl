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
  eval/regress/drift/compat`, `formal_result:"not_run"`), `ai-agent`
  (`is_ai_agent_source`; structural analysis, `agent_analyzed`, not formal proof),
  and `causal` (`is_causal_source`; the causal graph never enters `KernelModel`,
  `fsl-runtime`, or `fsl-solver` — see [`DESIGN-causal.md`](DESIGN-causal.md) §1 —
  and native `check` reports `result:"causal_model_checked"` /
  `formal_result:"not_run"`. The frozen Python reference has no causal
  implementation at all, so `is_causal_source` is a keyword sniff that lives in
  `tests/dialect_registry.py` itself rather than `src/fslc`).
- `MONITOR_EXCLUSIONS: dict[str, str]` — repo-relative path → reason, for
  individual files the frozen Python Monitor legitimately rejects. Each entry
  names its active native or BMC-side coverage, and a stale entry fails the
  harness once the Monitor starts accepting it.

## Classification (automatic, in the harness)

`classify(path)` reads the source and returns one of:

1. `EXCLUDED` — `is_ai_project_source` / `is_ai_agent_source` / `is_causal_source`
   match, or path in `MONITOR_EXCLUSIONS`.
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

The injected corpus is also the executable calibration of detector boundaries:
reachability/coverage catches over-constraint, vacuity catches ineffective rules,
strict tags need an external requirement registry to detect pure omission,
mutation is meaningful as a delta from an accepted baseline, and
acceptance/forbidden traces provide the independent channel for boundary or guard
drift. No single green detector establishes intent fidelity.

This calibration is measured on the authoritative native surface, not only the
frozen Python reference (issue #485): `rust/fslc/tests/injection_detector_matrix.rs`
runs the same primary/blind matrix against the native `fslc` binary and is part
of the Rust CI-equivalent gate (`tools/check-native-integration.sh`, which does
not execute Python — AGENTS.md). `tests/test_injection_bench.py` measures the
same native binary by default too (`FSLC_BENCH_CLI=python` switches back to the
frozen reference); it regenerates the committed `examples/gallery/injected/MATRIX.json`
evidence artifact. A detector gap that is native-only and already filed (the
`unreachable-antecedent` lane's `vacuity` primary detector against a
`forall`-quantified implication, issue #486) is an explicit, documented
exclusion in both — never a silent pass.

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

This harness — the full dual-evaluator pipeline (Monitor load, BFS/expression
agreement, verdict agreement) — belongs to the frozen Python reference
implementation and is no longer run by `.github/workflows/ci.yml`. It remains
available for manual historical/reference checks; active CI coverage is
provided by the Rust workspace tests and WASM browser validation.
"Manual/reference" describes who runs it, not whether it may stay red: a
corpus registration gap here is invisible to CI precisely because nothing
else runs the full dual-evaluator pipeline over `specs/`/`examples/`, so a
failing run must still be treated as a reviewable registration diff to land
(a new dialect/example directory registered in `tests/dialect_registry.py`,
or a stale exclusion/declared-error front matter removed), not left red
indefinitely — see issue #476.

Its narrower structural obligation — every `.fsl` under `specs/`/`examples/`
either `check`s cleanly or is a declared/excluded error, so nothing rots
silently the way issue #485's 18 files did — has a native equivalent that
*is* active CI: `rust/fslc/tests/corpus_check_sweep.rs`. Unlike
`rust/fsl-lsp/tests/corpus.rs` (parse + index only, never builds a checked
model), this sweep runs `fslc check` on every file and requires each one to
either succeed, declare `// expected-result: error` for a `check`-targeted
invocation, be a `refinement`-dialect file (structurally out of scope: a
mapping file has no `state` block, so `fslc check` always reports "spec has
no state block" regardless of whether the mapping is sound — whether it is
actually exercised by `fslc refine` is a separate claim tracked by #483, not
asserted here), or the injected detector corpus (its own matrix, above), or
carry a reasoned exclusion naming the test or issue that actually owns its
expected behavior.

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

## Implementation fault operators (#537 C5)

`injection_detector_matrix.rs` calibrates detectors against defective *specs*:
`examples/gallery/injected/*.fsl` are hand-authored, and each names the detector
that must catch it and one that must stay blind. It answers "can this detector
see a bad spec?"

C5 asks a different question — "would our test suite notice if the *verifier*
started lying?" — and needs a different mechanism, because the defect lives in
Rust, not in a `.fsl` file. Every escaped defect in the 2026-07 batch was of this
kind: `wrap_specialized`'s `_ => 0` (#601), `run_db_check` folding only
`violated` (#600), `analyze batch` dropping explicit non-`.fsl` input (#496).

The operators are **patches, not code**. `rust/fslc/tests/fault_operators/` holds
one minimal diff per operator; the harness applies it to a scratch checkout,
rebuilds there, runs the test named as that operator's primary detector, and
**requires that test to fail**. Then it reverts. Nothing is injected at runtime.

No fault-injection hook may exist in the shipped binary, under a feature flag or
otherwise. For a verifier that is the worst artifact imaginable: a switch that
makes the product lie about verdicts, in the same codebase whose purpose is to
prevent exactly that. The mechanism that proves we detect false greens must not
itself be able to cause one. This is not a cost trade-off, and no build
convenience overrides it.

Each operator declares a primary detector and a blind detector, reusing
`injection_detector_matrix.rs`'s discipline: the primary must fail under the
patch, and the blind must still pass — an operator that breaks everything proves
nothing about the detector it claims to calibrate.

The harness needs its own negative control. A no-op patch must leave every named
detector passing; if it does not, the harness reports failure whatever the
operator does, and every cell in it is meaningless.

A patch that no longer applies is a **loud failure, not a skip**. It means the
seam it targeted moved, and someone must confirm the fault is still possible
there and re-target the patch. Silently skipping a stale operator is how a
detector matrix rots into decoration.

Rebuild cost puts this in the product gate (`tools/check-native-integration.sh`),
not the per-pull-request gate. Operators patch `rust/fslc` where possible, so
the rebuild is that crate plus a relink rather than the workspace.

The harness is `tools/run-fault-operators.sh`, reached as the
`fault-operators` phase of `tools/check-native-integration.sh` and included in
its `all` — deliberately not in its `rust` phase, which `.github/workflows/ci.yml`
runs on every pull request. CI runs it as its own post-merge `fault operators`
job, required by the `product gate` aggregator (`docs/DESIGN-ci.md` "Product
gate contract"): a matrix that never runs is worse than one that skips, so
"not on pull requests" must not become "nowhere". Operators are rows in
`rust/fslc/tests/fault_operators/operators.txt`, each naming a patch file, a
primary detector, and a blind detector; the two controls are
`controls/no-op.patch` and `controls/stale-seam.patch`. Adding an operator is a
patch file and a table row, both data.

Detector naming is measured, not asserted. The first calibration moved two of
the three intended primaries after the harness showed the named test could not
fail under the fault: `exit_status`'s failure row is reachable only through
`mutate_exit_status`, so `corpus_check_sweep`'s conservation law never sees it
(`issue_554_mutate_exit_status::a_violated_baseline_exits_one` does), and #600's
status-guard half (`!= 0 && != 1`) folds a status-1 kernel identically to
`== 2`, so only its verdict-fold half is what
`issue_600_db_check_folds_kernel_verdict` actually detects. Both are the kind of
mis-attribution a matrix that never runs its own negative side would have kept.

#600's status guard is deliberately **not** a fourth operator, and the reason is
itself a measurement. Reverting `run_db_check`'s guard to `== 2` and running the
whole `fslc` suite fails nothing: the two guards differ only for a kernel status
outside {0,1,2}, and inside `run_db_check` no `.fsl` input can produce one.
`check_db` calls `validate_db` first, so every input `run_verify` would reject
with status 2 is already rejected before the kernel runs; `run_verify`'s
remaining non-{0,1} exits are status 3 from `Z3Solver::new`, from
`replay_bmc_witnesses`, and from `render_explicit_output` — internal
inconsistencies, not spec properties. The guard is defensive depth against a
verifier fault, not a reachable behavior, so no fixture can calibrate it and an
operator for it would only ever report "primary did not fail". An operator whose
fault is unobservable is not a missing detector; it is a fault that does not
exist yet. The unreachability itself needs no issue: an issue asserts there is
work to do, and for that there is none — the guard is correct, defensive, and
consistent with its `run_domain_check` sibling. This paragraph is the record.

The measurement did surface a separate obligation, tracked as #612:
`run_db_check` and `run_domain_check` carry the *same* predicate in two places,
and #600 exists because someone maintained one of them and not the other. That
duplication is real work whatever the branch's reachability, and it belongs with
the rest of the outcome vocabulary in `rust/fslc/src/outcome.rs`. Its
justification is the duplication and the third site that will otherwise repeat
#600 — not that extracting it would make a fourth operator possible. Building
test machinery for a fault that cannot occur is the mistake this paragraph
exists to prevent.

This is the distinction the harness has to keep making. "No test covers this
line" and "no input can reach this line" look identical from a coverage report
and are opposite conclusions.

Tell them apart the way this one was told apart, before writing either an
operator or the test you think is missing: patch the seam to its defective form
and run the *whole* `fslc` suite, not the one test you expect to own it. A single
test staying green says only that this test does not cover the seam. The whole
suite staying green — with the pre-existing failures subtracted, since a suite
that is already red proves nothing either way — says no input reaches it, and
the honest output is a recorded measurement rather than a new operator or a
fixture nobody can build.

## Coupled changes

`CONTRIBUTING.md` "Adding a language feature" gains: register any new dialect's
construct and example corpus in `tests/dialect_registry.py` (and any new example
directory is claimed automatically by the scan — the harness fails until its
construct is registered).

`CONTRIBUTING.md` "Guidelines for changes" gains: a fixed escaped defect gains
an entry in `rust/fslc/tests/fault_operators/` when its defect class can recur
at a sibling seam, so the regression test proves not only that the defect is
gone but that the suite would notice its return.

## Non-goals

- Consolidating `test_oracle_agreement.py` / `test_evaluator_agreement.py` into
  the harness (they keep their deeper declared-depth cases; overlap ≈ 1 min).
- Verifying declared verdicts (`expected-result: proved` etc.) — that stays in
  `test_gallery.py`; the harness checks evaluator *agreement*, not spec intent.
- Refinement-checking the mapping files (covered by `test_refine*.py`).
- Making Monitor accept no-action specs or project-level fsl-ai files.
