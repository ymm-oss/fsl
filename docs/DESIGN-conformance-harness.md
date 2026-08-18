# FSL — dialect corpus conformance harness (Monitor / oracle / agreement safety net)

## Goal

Every `.fsl` under `specs/` and `examples/` is either (a) driven through the full
dual-evaluator safety net — `parse → desugar → build_spec → Monitor load →
BMC/Monitor expression agreement → verify-vs-oracle verdict agreement` — or (b)
excluded **loudly**, with a documented reason that the harness re-asserts on every
run. A new dialect (or a new example directory) that nobody registers is a harness
failure, not a silent skip — see "Cost and CI wiring" below for what actually
invokes this harness today (no CI workflow does; it is a manual/reference check).

## The gap (issue #167)

The 2026-07-08 audit found 15 of 18 `examples/db/*.fsl` failing Monitor load
(`_check_deterministic_init` was type-blind for per-key map init; fixed in
`470c75c`) while `pytest -q` stayed green: `tests/test_oracle_agreement.py`
scans only `specs/*.fsl` + `examples/gallery/{valid,errors}`, and
`tests/test_evaluator_agreement.py` only `specs/*.fsl`. Both `pytest.skip` when
`can_monitor()` fails. So an entire dialect corpus sat outside the core
correctness invariant and nothing said so. Skips are the bug this design removes.

## Nested semantic constructs are a separate coverage unit (#681)

Top-level dialect coverage does not establish that every accepted construct inside
that dialect has executable semantics. The native migration in #207 demonstrated
the distinction:

- the phase-0 inventory reported 7 business files and exact surface-AST parity,
  but the measured corpus contained `biz_policy_eventually` and no
  `biz_policy_precedence` node;
- full-corpus `check`/`verify` parity therefore exercised business dispatch while
  never entering `BusinessPolicyBody::Precedence`;
- the Python reference's dedicated precedence tests remained green independently,
  but were not native product evidence and were not ported into the Rust workspace;
- Rust accepted the syntax while `lower_business` matched it with an empty arm, so
  parser, corpus-count, and bare-`check` gates all stayed green as the policy was
  discarded.

The coverage unit for a semantic sum type is consequently each behavior-bearing
variant, not merely its enclosing frontend. A migration or new variant must bind
every accepted variant to one of two observable postures:

1. executable native semantics with an accepting control and a rejecting control;
2. an explicit fail-closed diagnostic with a rejecting test.

Where all variants lower to the same output category, the lowering should be
structured as a total expression returning that category. This makes an empty
unit arm a compile error rather than a silent feature omission. For business
policies, `lower_business` now returns one `SpecItem` from every
`BusinessPolicyBody` arm before appending it.

`examples/gallery/adversarial/business_precedence_bypass.fsl` is the maintained
rejecting control for precedence. Its own header declares the expected native
command, `violated` result, and `invariant` kind, so
`corpus_expectation_manifest.rs` executes the claim instead of inferring a
contract from current output. The focused Rust tests additionally establish the
history state/update and induction-positive structure, and select every current
`BusinessPolicyBody` variant against its own native rejecting control. Removing
the lowering can no longer leave the native corpus green.

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
path exists). Regression for the harness itself: reverting `470c75c` locally makes
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
indefinitely — see issue #786, which found this harness itself red on two
cases precisely because nothing was watching it.

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

## Refinement mapping manifest (#537 C4, issue #593)

The corpus sweeps above are `check`-shaped, and `check` is structurally blind to a
refinement mapping: a mapping file has no `state` block, so `fslc check` answers
`semantics`/"spec has no state block" for one whether or not the mapping is sound.
A green corpus therefore said nothing about `fslc refine`. Until this manifest only
6 of the 28 corpus mappings had ever been run through `refine`, by a script
(`tools/check_rust_refinement_parity.py`) that no workflow and no
`tools/check-native-integration.sh` lane invoked. The other 22 were executed by
nothing.

`rust/fslc/tests/refine_corpus_parity.rs` owns the mapping corpus the way
`tests/dialect_registry.py` owns the dialect corpus, on three rules:

- **The roster is derived, never listed.** The test walks `specs/` + `examples/`
  for `refinement`-dialect files and requires each to hold a manifest row or an
  exclusion. Adding a mapping fails the test until it is registered, and a
  registered path that no longer exists fails as a stale entry. A hard-coded list
  is the shape #577 retired 28 stale instances of.
- **Expectations are transcribed from declarations, not from output.** Each row
  carries `declared_by`, the `path:line` of the README row, documented command
  comment, or fixture header that states the expected verdict. Recording what the
  binary prints would pin a defect as the contract the moment one exists — which
  is exactly the state `examples/layers/return_impl_refines.fsl` is in
  (issue #615). Where no declaration exists, the correct move is to write one, not
  to transcribe a measurement. `depth` is deliberately *not* part of that contract:
  it bounds the search rather than declaring the verdict, so it is taken from the
  documented command line where one exists.
- **Both channels, every row.** `result`, `kind`, and the process exit code are all
  compared (#537 C4). An envelope that disagrees with its exit status is how #554
  and #600 escaped.
- **The citation is checked, not trusted.** Where a mapping declares its own
  expectation in the gallery `expected-command`/`expected-result`/`expected-kind`
  header convention, the row must agree with it — including the `--depth` inside
  `expected-command`. Otherwise `declared_by` is prose that can drift from the file
  it names, which is the same "the citation looked fine" failure the manifest
  exists to prevent. It caught one on introduction: the `refinement_failed_map.fsl`
  row ran at depth 4 against a header declaring `--depth 3`.

  22 of the 26 rows additionally carry a `declaration` anchor, and the harness
  requires the cited file to still contain a line stating this row's verdict.
  That is what keeps each declaration single-owner. `governance_semantic_mapping`
  is declared by the broken *implementation* it maps
  (`governance_semantic_after.fsl:2`, `// expected-result: error`) — the file that
  owns the error — so the citation is verified where the declaration already
  lives rather than copied onto the mapping as a second `expected-result` header
  to keep in sync. The anchor is a text match, not a line number: an edit above
  the declaration must not fail the test, but deleting it or changing its verdict
  must. The 4 unanchored rows (the `agentic_rag` and `multi_agent_system` positive
  pairs) are declared by multi-line statements of the mapping's *purpose* rather
  than a verdict on one line; they are the manifest's residual trust.

Depth is the one field the corpus is allowed to leave open, and where it is
declared the declaration wins. 24 of the 26 rows run at the depth their README
command line or fixture header states. The two that do not —
`specs/bank_refines.fsl` and `specs/seat_refines.fsl` — had no documented command
at all; each now carries one in its abstraction's header (`specs/bank.fsl:2-3`,
`specs/seat_booking.fsl:2-4`), promoting the `refines` those headers already
asserted into a runnable claim. Their depth 6 is the manifest's own and says so in
the row: it subsumes the depth-4 `refines` that `tests/test_refine_oracle.py`
asserted, since a counterexample within 4 steps is also within 6.

Exclusions are self-retiring in the #568 sense: each records the *measured* fact
that blocks a live row, and the harness re-measures it. `Blocked` re-runs `refine`
and fails when the recorded failure stops reproducing; `UndeclaredImplOperand`
fails when the corpus starts declaring the operand the mapping names. Both failure
messages name the row that must replace the exclusion. Two entries exist:
`examples/layers/return_impl_refines.fsl` (issue #615 — the README declares
`refines`, `da003eb` tightened `typecheck.rs` without migrating the corpus to the
`convert`/`abstract` requirement) and
`examples/causal/evidence/incident-log-mapping.fsl` (not a `refine` input at all:
its `impl` operand is a production observation log consumed by
`fslc causal observe-expectations --mapping`, owned by `causal_cli.rs`).

Cost: ~2m15s wall, the rows being run on a worker pool. Two `agentic_rag` rows are
~100s and ~126s on their own in a debug build, so the sweep is bounded by its
slowest row rather than by their sum.

## Corpus expectation ownership (#537 C4)

C4 requires every `specs/`+`examples/` artifact bound to the command that
actually evaluates it. Slice 1 (above) closed the refinement-mapping column;
issue #645 closes the two categories a comment in `corpus_check_sweep.rs` used
to document as an open gap. Each corpus category now has exactly one owning
test:

| Category | Owning test |
|---|---|
| kernel/dialect spec, bare `check` | `corpus_check_sweep.rs::every_corpus_spec_checks_ok_or_declares_its_error` |
| `check`/`verify`/exit conservation law | `corpus_check_sweep.rs::check_result_and_exit_status_never_contradict` |
| `ledger` vs its `verify` baseline | `corpus_check_sweep.rs::ledger_exit_status_agrees_with_its_verify_baseline` |
| refinement mapping | `refine_corpus_parity.rs` (this section, above) |
| declared `examples/gallery/{valid,errors,adversarial}` fixture | `corpus_expectation_manifest.rs` |
| `examples/gallery/injected/` (calibrated detectors) | `injection_detector_matrix.rs` |
| evidence-only document (`causal`, `agent`/`ai_component`/ai project) | `evidence_corpus_manifest.rs` |
| Worker column (native/Worker parity) | `rust/fsl-wasm/test-browser.mjs` (already C4-complete; see below) |

`corpus_expectation_manifest.rs` reads `examples/gallery/{valid,errors,
adversarial}`'s `expected-command`/`expected-result`/`expected-kind` header
convention and runs each declared command verbatim, comparing `result`,
`kind`, and the exit code the production `fslc_rust::outcome` module binds to
that result — the same three-channel discipline the refinement manifest uses,
reused here rather than re-derived. It found that 12 of the corpus's ~38
declared fixtures (three of which need a non-default flag —
`--vacuity error` / `--deadlock error` — to reproduce their own declared
verdict at all) had never been run by any native test; `tests/test_gallery.py`
was the only oracle, and it is frozen-Python, so `tools/check-native-
integration.sh` never executed it. All 38 reproduce their declaration
natively; no exclusion was needed. Every file in the three directories that
carries no header (a refine-mapping operand, or a governance fixture already
owned by `corpus_check_sweep.rs::GOVERNANCE_FIXTURE_EXCLUSIONS`) is a
self-retiring `StructuralExclusion`, re-checked against the file that actually
owns it.

`evidence_corpus_manifest.rs` classifies the corpus the same way
`fslc ai check`'s own dispatch does — `fsl_syntax::is_causal_source`,
`fslc_rust::frontend_output::is_ai_project`, `fsl_syntax::dialect_keyword` —
rather than re-deriving a fourth string sniff, and requires every classified
document to carry a manifest row or a reasoned exclusion in both directions.
The corpus holds exactly 6 live evidence-only documents today (3 `causal`
sidecars, 3 `fsl-ai` documents split `agent`/plain `ai_component`/project) and
one exclusion (`examples/annotations/annotated_ai_component.fsl`, an issue
#281 annotation-syntax sample whose own doc comment declares plain `check`,
not `ai check`).

`corpus_check_sweep.rs`'s `every_corpus_spec_checks_ok_or_declares_its_error`
no longer asserts that a `check`-targeted declared-error fixture actually
fails under `check` — that positive assertion moved to
`corpus_expectation_manifest.rs`, which runs the exact declared command
(including non-`check` ones) rather than a hard-coded `command == "check"`
comparison. The sweep keeps its complementary half: a file declaring no error
anywhere must not fail `check` unexpectedly.

**Worker column, already satisfied.** `rust/fsl-wasm/test-browser.mjs` walks
all corpus `.fsl` files and compares native/Worker `check`/`verify` envelopes
for every one of them except a self-retiring `agent`/`causal` exclusion set
(the Worker has no verb for either document type at all). No change was
needed for C4's Worker column.

**Residual.** `span` location fidelity has no header-declared expectation
anywhere in the corpus (no fixture states "the location must be exactly
`L:C`"), so nothing in this ownership map cross-checks it. That is a real gap,
not a silent one: C4's span column is future slice work, not claimed done
here.

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

### The fault must be witnessed, not inferred (#753)

**Root cause.** `git apply` silently skips every file in a patch, and exits zero, when
the scratch checkout is not its own repository root. Git then resolves the scratch to the
*enclosing* repository, where every path under it lives beneath `rust/target/` and is
git-ignored:

```
$ git -C "$scratch" apply --verbose shared-observer-lineage.patch
Skipped patch 'rust/fslc/tests/support/self_conformance_mapping.rs'.
Skipped patch 'rust/fslc/tests/triangulated/p1_compound_outcome.rs'.
$ echo $?
0
```

`apply_operator_patch` saw success, the scratch compiled **unfaulted**, the detector
passed because there was nothing to detect, and the harness recorded that as "the primary
detector still passed under the fault" — a detector gap, when the fault had never been
applied at all.

`sync_scratch` had guarded this with `[ -e "$scratch/.git" ]`, which tests the wrong
property: *something existing* at that path is not *git resolving the scratch as its own
root*. An empty or partial `.git` left behind by a restored CI cache satisfies `-e` and
suppresses the `git init` that would have repaired it. That is why the failure appeared
only in CI, never locally — where the scratch's `.git` persists between runs — and why the
same revision returned different verdicts on different runs, depending on what the cache
happened to carry. The guard now requires `git rev-parse --show-toplevel`, run inside the
scratch, to equal the scratch itself, and rebuilds `.git` when it does not. `apply_patch`
additionally runs `git apply --verbose` and turns a `Skipped patch` line into a nonzero
status, so the silent-skip path cannot return success again through some other route.

Reproduced and calibrated locally by breaking the marker exactly as the cache did — an
empty `$scratch/.git` directory — and running the same shard both ways: under the previous
`[ -e ]` guard the run fails with the source witness naming
`rust/fslc/tests/support/self_conformance_mapping.rs`; under the repaired guard all six
operators calibrate and the run exits 0.


A `primary still passed under the fault` verdict has two possible causes, and they
belong to different owners: the detector genuinely does not cover the seam (a real,
reportable gap), or the detector never saw the fault at all (a defect in this
harness). Until #753 the harness could not tell them apart. It inferred that the
fault had arrived from two weaker facts — `git apply` exited zero, and the scratch
compiled — and reported the first cause whenever the second was true. The
observable symptom was the same operator returning different verdicts on different
runs of the same revision, which made an unrelated pull request unmergeable through
the `semantic mutation` required context.

Two fail-closed witnesses now stand between the patch and the verdict, both in
`tools/run-fault-operators.sh`:

- **Source.** After the patch applies, every file it names must differ,
  byte-for-byte, from the pristine working-tree copy. `git apply` exiting zero says
  the patch was *accepted*, not that the bytes the compiler will read changed.
- **Binary.** Of the two artifacts a detector can execute — the test harness
  binary, read back from cargo's own `Executable <target> (<path>)` line so it is
  the binary that ran rather than an inference about it, and the `fslc` executable
  a detector may spawn through `env!("CARGO_BIN_EXE_fslc")` — at least one must
  differ from the digest recorded for it under the no-op control, the one point in
  a run where the scratch is known to carry no fault. A byte-identical pair is
  unambiguous: no compilation nondeterminism can make a genuinely faulted artifact
  equal an unfaulted one, so this fires only on real artifact reuse, never on a
  flaky digest.

  **Both artifacts, not just the test binary.** An operator's fault normally
  reaches exactly one of them: a patch under `rust/fslc/tests/**`
  (`shared-observer-lineage`) changes the test harness binary and leaves `fslc`
  untouched, while a patch under `rust/fslc/src/**` (`failure-verdict-exits-zero`,
  whose detector spawns the CLI) changes `fslc` and leaves the test binary
  untouched. The first version of this witness hashed only the test binary and so
  called the second shape a harness defect on every shard. It passed locally
  because local rebuilds happened to produce differing test binaries anyway — a
  vacuous green — and CI caught it. Requiring *both* to be unchanged before firing
  is what makes the witness sound in both directions.

Both witnesses report through the harness's own failure path and name the cause, so
a harness defect can no longer be recorded as a detector gap.

The source witness has its own negative control,
`controls/identical-after-apply.patch`, alongside the stale-seam and no-op controls:
a hunk that removes a line and adds the identical line back applies cleanly and
leaves the file unchanged, and the harness requires the witness to refuse it. The
binary witness has no fixture of its own — a fault that reaches the source but not
the linked binary cannot be constructed on demand — and is calibrated by live
mutation instead. That asymmetry is recorded here rather than left implicit.

Rebuild cost keeps this out of the ordinary Rust workspace lane, but M13 makes
it part of the dedicated semantic-mutation lanes on every pull request and
product-gate run — round-robin sharded three ways across the
`semantic-mutation-operators` matrix (docs/DESIGN-ci.md, "Sharded pre-merge
Linux evidence"; docs/DESIGN-semantic-mutation-gate.md, "CI scheduling: two
lanes, one aggregator"). Operators patch `rust/fslc` where possible, so the
rebuild is that crate plus a relink rather than the workspace.

Patches are applied with `git apply`, never the system `patch`. The first CI run
of this matrix failed (#613) because BSD `patch` on macOS accepted the no-op
control's hunk — which ends at end-of-file — while GNU `patch` on
`ubuntu-latest` rejected the identical hunk against identical bytes. The matrix
was green on the author's machine and red on the runner, so for a moment its
verdicts were a property of the machine rather than of the fault. That is the
same class as the scratch-fidelity defect above and the reason both controls
exist: a calibration harness whose result depends on where it runs calibrates
nothing. `git apply` is one implementation wherever git is, applies zero fuzz by
default, and tolerates the prose preamble each patch file carries.

The harness is `tools/run-fault-operators.sh`, reached directly as the legacy
`fault-operators` phase, through `tools/check-native-integration.sh
semantic-mutation` for a complete unsharded local run, and — with its own
`--shard K/N` — as the curated half of `tools/run-semantic-mutation-gate.sh
--lane operators` in CI's sharded `semantic-mutation-operators` matrix. It is
deliberately not in the ordinary `rust` phase. CI requires the aggregate
`semantic-mutation` context on every event: pull requests run the curated
matrix and generic mutants intersecting the PR diff, while other events run
the complete accepted P2 pilot scope; the aggregator requires both the
operator-shard matrix and the unsharded generic-mutants job to succeed and
enforces that the shards' union covers every operator. Operators are rows in
`rust/fslc/tests/fault_operators/operators.txt`, each naming a patch file, a
primary detector, and a blind detector; the two controls are
`controls/no-op.patch` and `controls/stale-seam.patch`. Adding an operator is a
patch file and a table row, both data.

The generic half, its fail-closed classifications, exact decision anchors,
reviewed-equivalence rules, and raw evidence contract are accepted in
[`DESIGN-semantic-mutation-gate.md`](DESIGN-semantic-mutation-gate.md).

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

## Typed generative / metamorphic agreement (#537 C6)

M13 issue #673 promotes this fixed C6 foundation into the continuously seeded
FSL Logic Test. [`DESIGN-fsl-logic-test.md`](DESIGN-fsl-logic-test.md) owns its
machine-readable inventory, stable seed/case replay, named concrete/symbolic
edges, report completeness, structural shrinking, regression corpus, and
PR/scheduled tiers. The original sweeps and R1-R7 below remain executable
members of that larger gate.

`rust/fsl-verifier/tests/expression_agreement.rs` and `explicit_engine.rs`'s
corpus sweep proved agreement on hand-written fixtures and the existing
`specs/` corpus. Neither swept a *generated* model family, and nothing
executably anchored a dialect construct's semantics against its documented
desugaring. C6 slice 1
(`rust/fslc/tests/typed_agreement.rs` + `rust/fslc/tests/typed_agreement/`)
closes both gaps: a deterministic structural generator produces checked
`KernelModel`s (never string fuzz — every model goes through
`parse_kernel_source` + `build_model`, the same on-ramp `fslc` itself uses),
compared across the three engines that do not depend on Z3js
(`fsl_runtime::bfs` "Monitor BFS", `fsl_runtime::verify_explicit`, and
`fsl_verifier::verify_bounded` "BMC", native Z3), plus seven metamorphic
relations each with a positive test and a negative control.

### Generation space

`generator.rs::domain_sweep` crosses `docs/LANGUAGE.md` S2's four scalar
domain kinds (`range`/`entity`/`number`/`enum`) at fifteen `(kind, size)`
pairs against a structural variant selected by index (state-variable count
1-2, action count 1-3, guard present/absent, fairness, and one of the five
checkable property kinds — invariant/reachable/leadsTo/trans/terminal —
so each kind is exercised at least once). `generator.rs::operation_sweep`
covers `divide`/`remainder` in both property and (guarded, safe) action
context; `head`/`pop`/`at`/index and the unguarded `divide`/`remainder`
boundary live as dedicated `relations.rs` R6 tests instead (see "Two
confirmed findings" below for why). Both floors are asserted in
`typed_agreement.rs` (`DOMAIN_SWEEP_FLOOR = 15`, `OPERATION_SWEEP_FLOOR = 4`).

Slice 2 adds `generator.rs::expression_sweep`: 25 deterministic models
designate every evaluator-reachable `Expr` variant, with separate models for
all four `AggregateKind` values. Each model carries the same finite schema
containing all nine `TypeRef` and all three `TypeDef` rows. The test confirms
the designated expression node in the checked model, derives and checks the
12-row type inventory, then runs the same Monitor BFS / explicit / BMC
agreement, replay, and successor-admission path as the earlier families.
The source-coupled row declarations generate both their exhaustive enum match
and enumeration witnesses; the generated-model labels must equal those row
sets exactly. Each positive model must return a clean verdict, after which its
known-true invariant is negated and all three engines must detect the same
step-zero violation. The `unique` and `exactlyOne` representatives cover zero,
one, and multiple matching bindings rather than only their empty case.
`Expr::EnumMember` is a typed generated form rather than a retained
direct-spec token, so its family starts from a parsed model, replaces the
token in the typed surface tree, and re-runs `build_surface_model` before any
evaluator sees it.

The whole suite — generation, all engines, replay, successor sampling, and
all R1-R7 relations — runs in well under a second; the ~2-minute budget
the C6 brief set was never approached, so the generation space was not
narrowed to fit it.

### Metamorphic relations

Each relation cites the `docs/LANGUAGE.md` contract sentence it tests, not
observed CLI output:

| # | Relation | Contract citation | Negative control |
|---|---|---|---|
| R1 | Alpha rename (whole-token substitution over every position `reserved.rs::check_reserved_names` walks) leaves the verdict unchanged | Pure syntactic substitution over an already-typechecked program (no direct citation needed; the rename fixture cross-checks its own position coverage against `check_reserved_names`'s source text) | Renaming onto an existing name is a duplicate declaration, rejected by `build_model` |
| R2 | Leading BOM + trivia leaves the verdict unchanged | `lexer.rs::skip_trivia`: a BOM is trivia only at `offset == 0` | A BOM inside an identifier is a parse error |
| R3 | Inline `state { x: T = e }` init reaches the same states as an equivalent explicit `init` block | LANGUAGE.md S2:499-508 ("normalized to an ordinary root assignment...same semantics as an equivalent `init` block") | Assigning the same root both inline and in `init` is `build_model`'s named semantic error |
| R4 | Disjoint simultaneous assignments reach the same states regardless of source order | LANGUAGE.md S5:644-661 ("all right-hand sides...read the old state...frame condition is automatic") | Assigning the same variable twice on one path is the named semantic error |
| R5 | A domain-bound-coincident invariant (`x <= hi`, textually equal to the type's own `hi`) holds at every declared size by construction | LANGUAGE.md S6 "Type bounds" (automatic, so a variable can never leave its own declared bound) | Widening the domain past the invariant's stale literal bound changes the verdict to `violated`; also reproduced mechanically via `enumerate_builtin_mutants`'s `type_bound_hi_plus1` |
| R6 | `/`/`%` are total in property context but `partial_op` in action context | LANGUAGE.md S3:557-570 | The same expression in action context is checked directly by raw `verify_bounded`; the six operation fixtures must agree across all four native engines |
| R7 | `entity`/`number` + `verify` reaches the same states as the hand-written lowered `type` | LANGUAGE.md S2:485-486 ("desugars to `type`") | Shifting the lowered bound by +1 past the declared size is detected |

`R4`'s `assignment_remove` and `R6`'s `equality_operator_flip` reuse of
`fsl_tools::enumerate_builtin_mutants` follow the same discipline as
`injection_detector_matrix.rs`: each candidate mutant's non-equivalence was
measured (it must change the BMC verdict or fail to build) before the
"must be a kill" assertion was written, and the first `type_bound_hi_plus1`
candidate tried against R5's own fixture turned out equivalent — the
mutation left the action's independently-derived wraparound modulus
unchanged, so the widened domain was never actually reachable. That
equivalent candidate is why R5's mutate-reuse test uses a separate,
dedicated fixture instead of R5's own; see `relations.rs`'s
`r5_mutate_kill_fixture` doc comment for the full account.

### Confirmed finding

The remaining finding is recorded as a re-measured fact, not silently
normalized away or excluded:

The former property-context Seq exclusion was resolved by #650. Reached
`pop`/`head`/`at`/index operations now use the same path-sensitive definedness
conditions in concrete and symbolic state-property evaluation. Monitor BFS,
explicit verification, and BMC return `partial_op` at the same step with
`_partial_property_<property>`; no solver-selected inactive slot can become a
property value or reachable witness. The former self-retiring exclusion is now
the positive agreement control
`relations.rs::r6_property_context_seq_head_is_uniformly_partial`.

The former raw-API gap for action-context partial operations was resolved by
#651. `fsl_verifier::verify_bounded` now applies backend-neutral symbolic
definedness at the public verifier boundary for ordered guards, reached body
expressions, and `ensures`. The CLI and Worker no longer consume `partial_op`
from their concrete boundary pre-scan; non-partial outcomes that the bounded
symbolic value cannot represent still retain that exact concrete evidence. The
R6 fixtures assert Monitor BFS, explicit,
`find_boundary_violation`, and raw BMC agree on `partial_op` for all six named
operations. Property-context `/` and `%` totalization remains the deliberate
exception and has a dedicated negative control in the #650 regression test.

### Z3js / Worker parity ownership

`fsl-solver-z3js` is `wasm-bindgen`-only and structurally unreachable from
native `cargo test` — there is no native code path that links it. That
column is not silently skipped: it is owned by `test-browser.mjs`'s Worker
parity suite, which this C6 slice does not duplicate. C6 compares the
three engines native tests *can* reach; a browser-side C6 sweep, if ever
needed, is a Worker-suite concern, not a gap in this one.

### Slice 2 implementation

`sweep_summary.rs` now aggregates `(domain kind, domain size, property kind,
state-variable count, action count, guarded, fair, expression variant,
aggregate kind, type row, operation/context)` counts. The
`expression_variant_sweep_agrees_across_all_three_engines_and_covers_all_types`
test prints a machine-readable `expression_variants={...}`,
`aggregate_kinds={...}`, and `type_rows={...}` slice after the generated run.
`assurance/expr.rs` and `assurance/types.rs` cite that exact test, completing
the planned `sweep_summary` → C3 matrix connection.

The live syntax enum has 24 variants, correcting slice 1's stale count of 22.
The 22 checked-kernel variants are swept. `Call` is expanded by
`PredicateExpander` and `Stage` by `StageResolver`; the C3 axis records both as
fail-closed unsupported on evaluator columns and owns a typed-AST injection
control proving a leaked form is rejected by the semantic build gate. Each
checked variant/type model also owns its paired invariant-negation control, so
the new agreement anchor proves it can reject a known semantic violation. No
new cross-engine disagreement was found by this family. The two existing R6/Seq
findings remain the only self-retiring exclusions and are not weakened or
reclassified by the safe Map-index / collection-method representatives used
for the variant rows.

## Coupled changes

`CONTRIBUTING.md` "Adding a language feature" gains: register any new dialect's
construct and example corpus in `tests/dialect_registry.py` (and any new example
directory is claimed automatically by the scan — the harness fails until its
construct is registered, on the manual/reference runs described in "Cost and CI
wiring" above; no CI lane currently runs it for you).

`CONTRIBUTING.md` "Guidelines for changes" gains: a fixed escaped defect gains
an entry in `rust/fslc/tests/fault_operators/` when its defect class can recur
at a sibling seam, so the regression test proves not only that the defect is
gone but that the suite would notice its return.

## Non-goals

- Consolidating `test_oracle_agreement.py` / `test_evaluator_agreement.py` into
  the harness (they keep their deeper declared-depth cases; overlap ≈ 1 min).
- Verifying declared verdicts (`expected-result: proved` etc.) — that stays in
  `test_gallery.py`; the harness checks evaluator *agreement*, not spec intent.
- Refinement-checking the mapping files (owned by the manifest in
  `rust/fslc/tests/refine_corpus_parity.rs`, above — not by this `check` sweep,
  and no longer by the frozen `tests/test_refine*.py`, which the required product
  gate does not execute).
- Making Monitor accept no-action specs or project-level fsl-ai files.

## Native FSL self-conformance (#537 C7)

`rust/fslc/tests/self_conformance.rs` is the Rust-native Adapter/replay anchor
for the verifier's own finite-state contracts. It invokes
`env!("CARGO_BIN_EXE_fslc")` with `std::process::Command`, parses the real JSON
stdout, and reads the exit status directly from `ExitStatus`. The compatibility
anchor in `tests/test_self_conformance.py` remains frozen evidence for the
Python reference; it is not the native product claim.

The native mapping table is deliberately independent of
`rust/fslc/src/outcome.rs`. Importing the production classifier would make the
check circular: a wrong result classification could determine both the CLI
answer and the action fed to its oracle. Instead, the test transcribes the
frozen session corpus and mapping from
`tests/test_self_conformance.py:39-67,85-152,314-395`, the monitor mapping from
`:426-445`, and the negative controls from `:488-504,620-636`. Exit semantics
come from `docs/LANGUAGE.md:940-961`. The compound table independently
enumerates the 65 result values registered by
`rust/fslc/src/outcome.rs:82-216`; unknown values and incomplete sibling-field
envelopes are errors, never default successes or failures. Chain uses a
command-specific adapter because a layer additionally depends on its integer
`exit_code`, nested `detail.implements.result`, and the implementation-command
`passed` / `failed` vocabulary. Compound finalization separately enumerates
each command's exact top-level result/exit pair.

Three self-specs separate the contracts:

| Self-spec | Native anchor |
|---|---|
| `examples/self/fslc_session.fsl` | Real check/verify/induction and extended subcommand observations map to session actions, then replay conformantly. |
| `examples/self/fslc_monitor.fsl` | Real cart replay observations map to `step_ok` / `step_reject` / `finish`, then replay conformantly. |
| `examples/self/fslc_fold.fsl` | Real sweep, full five-layer chain (including nested implements and implementation-command results), and analyze-batch item verdicts map to success/failure/skipped folds; the real top-level result and exact process exit select the final action. `fold_spec_has_native_proof_vacuity_and_mutation_evidence` product-gates bounded verification, induction, vacuity, and the failure-sticky finalize-guard mutants. |

The C7 properties have both an accepting observation and a rejecting control:

| C7 property | Executable evidence in `self_conformance.rs` |
|---|---|
| Failure cannot be promoted to success | `session_contract_violations_are_rejected` rejects `check_ok; verify_violated; verify_ok`; the three compound tests replace a real failing run's `finalize_fail` with `finalize_pass` and require nonconformance. |
| Result and exit status cannot contradict | `session_mapping_rejects_result_exit_contradictions` rejects synthetic `violated`/exit 0 while accepting the real `violated`/exit 1 tuple; `mutate_failure_verdict_cannot_exit_zero` covers issue #554's exact baseline path. `fold_classifier_is_fail_closed` rejects wrong nonzero exits for sweep, chain, and analyze batch, while the compound tests require their real exact exits. |
| Required trace/location/evidence cannot disappear | `session_mapping_rejects_result_exit_contradictions` accepts a real failure only with a nonempty counterexample trace, source line/column, and first violated step, then rejects synthetic omissions/corruption of each. The monitor mapping likewise requires `failed_at_event`, violation, and pre-failure state, rejects missing/noninteger/out-of-range evidence, and proves the later cart event was not folded after first rejection. |
| Every legitimate success path remains reachable | `native_session_corpus_observations_replay_conformantly`, `native_monitor_observations_replay_conformantly`, and each compound test execute real passing paths, including the intentional empty analyze batch; `fslc_fold.fsl::ReachLegitimateSuccessPath` witnesses fold-success then finalize-pass. |
| Empty/error/unknown input cannot generate `verified` | `semantics_error_input_never_maps_to_verified` drives `examples/self/no_actions.fsl` through real check and verify, requires `error`/`semantics`/exit 2, maps it to `verify_user_error`, and replays that path conformantly; the two inherited session traces reject success without the prerequisite check/verify. |

The rejecting traces are the in-repository detection-power proof. The manual C5
calibration applies
`rust/fslc/tests/fault_operators/failure-verdict-exits-zero.patch` in a separate
scratch checkout and requires
`self_conformance::mutate_failure_verdict_cannot_exit_zero` to fail; it adds no
operator row or shipped injection hook.

`tools/check-native-integration.sh rust` runs the workspace Rust tests and
therefore owns this native anchor for a complete local run; in CI the same
test runs under whichever `rust-tests` shard `cargo-nextest`'s partitioning
assigns it to (docs/DESIGN-ci.md, "Sharded pre-merge Linux evidence"). The
frozen Python test is outside that gate and remains a compatibility
reference only.

## Triangulated Assurance (#670)

`rust/fslc/tests/triangulated_assurance.rs` generalizes the strongest C7 shape
without weakening C3/C5/C7. Semantic owners register CI-internal claims under
`tests/triangulated/`; the aggregator rejects missing/stale claims, non-raw
observations, missing fields or edges, skipped/unknown evidence, stale
citations, shared semantic owners/decision lineage, and missing calibration.
The complete contract is `docs/DESIGN-triangulated-assurance.md`.

The initial federation registers P1 compound outcome conservation, P2 native
symbolic-witness versus solver-free explicit/Monitor replay identity, and P3
token-based dialect dispatch. P1 now retains raw stdout/stderr bytes, process
exit, parsed JSON, and the native build fingerprint before its independent
mapping classifies the observation. P2 recomputes violation identity and source
span and rejects state/step/kind/location mutants. P3 uses one checked-in raw
source manifest across syntax-library, CLI, and LSP consumers while a manual
fixture oracle remains independent of the production registry.

`shared-observer-lineage.patch` is the calibrated common-mode fault: it selects
the production outcome classifier in place of the registered independent P1
mapping and declares the self-spec's owner/lineage. The primary test executes
that substituted registered path before the triangulated independence detector
fails, while the blind parser detector stays green. Consumer parity is never
counted as observer independence.
