# FSL — C3 Semantic Assurance Matrix (issue #537 C3, issue #646)

## Goal

`fslc` has two existing, narrow feature-coverage registries: the Public Kernel
`coverage.rs` matrix (`OUTCOME_FEATURE_KEYS` and friends, issue #223) and the
RCIR no-silent-omission registry. Issue #537's C3 asks for the same discipline
generalized across every semantic/product surface: rows are semantic
features, columns are verification/rendering surfaces, and every cell is one
of `exercised` / `rejecting-control` / `unsupported-fail-closed` /
`not-applicable` (with a reason), never blank. This document is the accepted
design for slices 1–3: the mechanism
(`Claim`/`Citation`/aggregator/negative controls) and the fully registered
`outcome_kind`, `violation_kind`, `properties`, `expr`, `types`, and
`dialects` axes.

This is not the assurance-*class* vocabulary in
[`DESIGN-assurance-classes.md`](DESIGN-assurance-classes.md) (`proved` /
`bounded` / `replay-observed` / `statistical` / `not_run`, a per-requirement
strength label rendered to end users in `fslc ledger`/`fslc html`). This
document's `Claim` vocabulary answers a different, CI-internal question: does
a real, currently-passing test in this repository actually demonstrate that
semantic feature X is exercised (or correctly rejected, or fail-closed
unsupported, or structurally inapplicable) on surface Y? It never appears in
product output.

`DESIGN-triangulated-assurance.md` builds a narrower, stronger method on top of
this citation discipline for selected soundness-critical claims. A C3 cell says
that one evidence item exists; a triangulated claim additionally requires a raw
common observation, two reviewably independent semantic lineages, all three
executable agreement edges, and calibrated positive/negative controls. It does
not replace this matrix or turn every cell into a triangulated claim.

## Relationship to #479 and #537's other contracts

#537 is the umbrella issue; C3 is one of its seven contracts (C1 false-green,
C2 verdict conservation, C3 this matrix, C4 corpus ownership, C5 fault
calibration, C6 typed generative agreement, C7 native self-conformance). C4
(corpus/refinement-mapping ownership, `tests/refine_corpus_parity.rs`) is
this design's direct precedent for the citation-anchor discipline: a
`Citation` here is structurally the same idea as C4's `Declaration` —
`path` + `anchor` text, re-verified against the working tree on every run,
never a cached table.

## Physical form

No new crate, no central hand-written table.

- `rust/fslc/tests/assurance_matrix.rs` — the aggregator. Declares every axis
  module via `#[path = "assurance/<name>.rs"]`, collects each axis's `Axis`
  value, and enforces two properties across all of them: every declared
  `(row, column)` cell has a `Claim`
  (`every_declared_cell_across_every_axis_has_a_claim`), and every claimed
  cell's citation rechecks against the current working tree
  (`every_claim_citation_rechecks_against_the_working_tree`). A
  `negative_controls` module demonstrates both checks actually fail: a
  fabricated/missing-file citation fails `Citation::recheck`, and an
  unclaimed required cell fails `Axis::check_complete` — proving the checkers
  are not vacuously `Ok`, with a matching positive control proving they are
  not vacuously `Err` either.
- `rust/fslc/tests/assurance/claim.rs` — the shared `Citation`/`Claim`/`Axis`
  types and their `recheck`/`check_complete`/`check_citations` methods.
- `rust/fslc/tests/assurance/outcome_kind.rs`,
  `rust/fslc/tests/assurance/violation_kind.rs`,
  `rust/fslc/tests/assurance/properties.rs`,
  `rust/fslc/tests/assurance/expr.rs`,
  `rust/fslc/tests/assurance/types.rs`, and
  `rust/fslc/tests/assurance/dialects.rs` — one module per semantic-surface
  axis. Each module owns its own `axis()` constructor and any small targeted
  test it needed to write because no existing test cleanly covered a cell.

## Cell vocabulary

```rust
pub enum Claim {
    Exercised { by: Citation },
    RejectingControl { by: Citation },
    UnsupportedFailClosed { by: Citation },
    NotApplicable { reason: &'static str, basis: Citation },
}
```

Every variant carries exactly one `Citation { path, anchor }`. `recheck()`
re-reads `path` from the working tree and asserts some line contains
`anchor` — never a line number (an unrelated edit above the citation must
not fail it, but deleting or renaming the anchored declaration must). This
mirrors `refine_corpus_parity.rs`'s `Declaration`/`declared_by` discipline
exactly, including its residual trust boundary: the citation proves the
*existence* of a real test at that anchor, not that the test's assertions
are correct — assertion correctness is `cargo test`'s job, run as one of
this feature's required gates, not the aggregator's.

"Required" is scoped per axis: each axis module declares its own `rows` and
`columns`; the aggregator only requires a `Claim` for `(row, column)` pairs
within that axis's own declared scope. A cell outside an axis's declared
columns is not part of that axis's matrix at all — it is not silently
skipped, it was never declared required. Narrowing an axis's declared
columns after this document is accepted needs a reason recorded here (see
"Slice 1 boundary" below for the two boundaries this slice already made).

## Axis: `outcome_kind`

7 rows, referenced directly from
`rust/fslc/src/coverage.rs::OUTCOME_FEATURE_KEYS` (never copied — the axis
module reads the constant, so the two can never drift). Declared columns:
`Monitor`, `CLI`.

| Kind | Monitor | CLI |
|---|---|---|
| `ok`, `requires_failed`, `partial_op`, `type_bound`, `invariant`, `trans`, `ensures` | `Exercised` — `conformance_coverage.rs::every_outcome_kind_the_corpus_emits_is_registered_and_exercised` | `Exercised` — `assurance/outcome_kind.rs::cli_conformance_command_emits_every_registered_outcome_kind` |

The `Monitor` column cites the existing bidirectional coverage test
verbatim: it already regenerates conformance vectors from the fixed fixture
manifest and asserts every registered kind is both emitted and present at
`level: "exercised"` in the coverage matrix — read in full before citing,
confirmed to do exactly this (not merely mention the word "outcome").

The `CLI` column has no existing test that decomposes by kind (the closest,
`kernel_contract.rs`'s golden-diff test for `fslc conformance`, asserts
whole-JSON equality, not a per-kind breakdown), so this axis owns one small
targeted test, `cli_conformance_command_emits_every_registered_outcome_kind`:
it runs the real `fslc conformance` subcommand as a subprocess against the
same fixture manifest `coverage.rs` scans and asserts every registered kind
appears in the CLI's own stdout JSON.

## Axis: `violation_kind` (issue #646)

### The vocabulary issue #646 under-reported

Issue #646 cites 9 `Violation.kind` values across 4 emission-site groups.
Reading `rust/fsl-verifier/src/induction.rs::prove_ranked_leadstos` in full
(not transcribing the issue text) found **12** live values, not 9: the
`RankFailure.kind` selection at what is now `induction.rs`'s dynamic
`let kind = if ... else { ... }` chain (reached only when a `helpful` clause
is declared) produces three values the issue's citation list omits entirely
— `pending_not_preserved`, `non_decreasing_helpful_action`, and
`non_helpful_action_increases_measure` — none of which had any exercising
fixture anywhere in the corpus before this slice. This is exactly the defect
class #537 C3 exists to make structurally impossible to miss again: the
registry below is the full, source-verified 12-value set, not the issue's
transcription of it.

### Registry

Single-owner constants in `rust/fsl-verifier/src/violation_kind.rs`
(`pub const ALL: &[&str]`), referenced (not copied) by the axis module.
Every one of the following emission sites now references a constant instead
of a bare string literal, with **zero byte-level change to any emitted JSON
`violation_kind` value** (proven by running the full existing `fsl-verifier`
and `fslc` verification test suites, including every golden-JSON-diff test,
unchanged before/after):

| Constant | Value | Emission site(s) |
|---|---|---|
| `LEADS_TO` | `leadsTo` | `bmc.rs::leadsto_violation`; `fslc`'s `verification_output.rs::render_leadsto_failure` (duplicate literal, now routed through the same constant) |
| `INVARIANT` | `invariant` | `induction.rs`'s `InductionCti` construction in the k-induction base/step loop |
| `TRANS` | `trans` | `induction.rs`'s `InductionCti` construction in the `k == 1` transition-property special case |
| `UNBOUNDED_BELOW` | `unbounded_below` | `induction.rs::prove_ranked_leadstos`, the unconditional pre-check |
| `PROGRESS_ACTION_NOT_FAIR` | `progress_action_not_fair` | `induction.rs::prove_ranked_leadstos`, the `helpful_fairness` check |
| `HELPFUL_ACTION_ENABLEDNESS_NOT_STICKY` | `helpful_action_enabledness_not_sticky` | `induction.rs::prove_ranked_leadstos`, the `helpful_sticky` check |
| `HELPFUL_ACTION_NOT_ENABLED` | `helpful_action_not_enabled` | `induction.rs::prove_ranked_leadstos`, the `no_deadlock` (helpful variant) check |
| `NON_DECREASING_ACTION` | `non_decreasing_action` | `induction.rs::prove_ranked_leadstos`, the no-`helpful`-declared branch, and the dynamic `kind` chain's `else` fallback |
| `PENDING_NOT_PRESERVED` | `pending_not_preserved` | `induction.rs::prove_ranked_leadstos`, the dynamic `kind` chain (`helpful`-declared branch) |
| `NON_DECREASING_HELPFUL_ACTION` | `non_decreasing_helpful_action` | same dynamic `kind` chain |
| `NON_HELPFUL_ACTION_INCREASES_MEASURE` | `non_helpful_action_increases_measure` | same dynamic `kind` chain |
| `DEADLOCK` | `deadlock` | not a `Violation`-shaped struct field — `fslc`'s `verification_output.rs::render_deadlock_failure` renders it directly from `BmcResult::deadlock_trace`; the CLI's duplicate literal in `render_leadsto_failure`'s sibling path was also routed through the constant |

Declared columns: `BMC`, `induction` — the two fsl-verifier engines capable
of populating one of these fields.

| Kind | BMC | induction |
|---|---|---|
| `leadsTo` | `Exercised` | `NotApplicable` (Slice 1 boundary) |
| `invariant` | `NotApplicable` (Slice 1 boundary) | `Exercised` |
| `trans` | `NotApplicable` (Slice 1 boundary) | `Exercised` |
| `unbounded_below` | `NotApplicable` (induction-only ranking) | `Exercised` |
| `progress_action_not_fair` | `NotApplicable` (induction-only ranking) | `Exercised` |
| `helpful_action_enabledness_not_sticky` | `NotApplicable` (induction-only ranking) | `Exercised` |
| `helpful_action_not_enabled` | `NotApplicable` (induction-only ranking) | `Exercised` |
| `non_decreasing_action` | `NotApplicable` (induction-only ranking) | `Exercised` |
| `pending_not_preserved` | `NotApplicable` (induction-only ranking) | `Exercised` |
| `non_decreasing_helpful_action` | `NotApplicable` (induction-only ranking) | `Exercised` |
| `non_helpful_action_increases_measure` | `NotApplicable` (induction-only ranking) | `Exercised` |
| `deadlock` | `Exercised` | `NotApplicable` (induction never calls the BMC deadlock probe) |

Every `Exercised` cell cites a real, individually-read test:
`leadsTo`/`deadlock` cite `issue_260_leadsto_stagnation.rs`'s
`violation_kind`-asserting tests; `invariant`/`trans` cite
`induction_suggestions.rs`'s tests (confirmed via each test's assertion on
the `--engine induction`-only fields `suggested_invariants`/`"trans"` that
`render_induction_cti` only inserts for that exact `cti.kind`, not merely a
test that happens to run the induction engine); the four ranked-liveness
kinds already exercised elsewhere cite `leadsto_helpful_ranking.rs`
(`progress_action_not_fair`, `helpful_action_enabledness_not_sticky`,
`helpful_action_not_enabled`, `non_helpful_action_increases_measure`); the
four with no prior fixture cite the new
`rust/fsl-verifier/tests/leadsto_rank_kind_vocabulary.rs`
(`unbounded_below`, `non_decreasing_action`, `pending_not_preserved`,
`non_decreasing_helpful_action`) — each fixture confirmed by running it
through the real solver and reading the resulting `RankFailure.kind`, not
derived on paper.

### Independent bidirectional check

`assurance/violation_kind.rs::every_violation_kind_the_probe_corpus_emits_is_exactly_the_registered_set`
mirrors `conformance_coverage.rs`'s own bidirectional discipline: it runs one
small, self-contained spec per registered value directly through
`fsl_verifier`'s public API (its own probe corpus, independent of every
other test's fixtures) and asserts the observed `kind` set equals
`violation_kind::ALL` exactly — catching a kind that stops firing (dead
registry entry) as loudly as a kind that fires but was never registered (the
#646 defect class this whole axis exists to close).

### Slice 1 boundary

Two file-scope decisions bound this slice, both cited by axis `NotApplicable`
cells via this section (`### Slice 1 boundary`, cited by anchor text) rather
than a source-code citation, because they are design decisions, not
observable source facts:

1. `bmc.rs`'s own `make_violation` call sites for the plain (non-induction)
   BMC engine's `"invariant"`/`"trans"`/`"ensures"` property violations are
   **not** routed through `violation_kind`. Issue #646's own citation list
   never named these sites; they mirror `fsl_runtime::Monitor`'s already-
   registered `outcome.kind` spelling (`coverage.rs::OUTCOME_FEATURE_KEYS`
   already has `invariant`/`trans`/`ensures` rows), so unifying them is that
   registry's concern, not a gap in this one. Revisiting this is in-scope
   for a future slice if the two registries are ever meant to merge.
2. `fslc`'s `main.rs:14741` (the `refine` command's progress-check
   rendering, also a `"leadsTo"` literal reading a `BmcViolation`-shaped
   value) and `verification.rs:537` (`"leadsTo_rank"`, a fixed envelope-shape
   tag with no corresponding `RankFailure.kind` value — the granular ranked
   kind renders separately, in the `rank_failure` JSON field) are outside
   this slice's file scope (`bmc.rs`/`induction.rs`/`verification_output.rs`
   only, per the accepted brief). `main.rs:14741` duplicates an
   already-registered value and is a reasonable target for the same
   constant in a later slice; `verification.rs:537` is a different
   vocabulary entirely (JSON envelope shape tags, not `Violation.kind`) and
   is out of scope for this axis regardless of slice.

## Axis: `properties`

Rows are the kernel schema's `properties.required` list — `invariants`,
`transitions`, `reachables`, `leads_to`, `terminal`
(`schemas/fslc/kernel/kernel.v1.schema.json`). A `KernelModel` struct field
cannot be caught by a compile error here, so the axis uses the same
schema-anchored sync discipline as `SEMANTICS_FEATURE_KEYS`:
`kernel_schema_property_groups_match_the_axis_rows` asserts the axis's row
list equals the schema's `required` set, and a new property group must land
in that schema to become a public-kernel contract surface at all. Declared
columns: `BMC`, `explicit`, `induction`, `replay`.

The `replay` column rests on a source-verified structural fact: every
native verify run replays *every* symbolic witness (violation trace,
leadsTo violation trace, each reachable witness trace, deadlock trace)
through the solver-independent Monitor before rendering
(`verification_output.rs::replay_bmc_witnesses`, exit 3 on failure, called
on both the BMC seam and the explicit renderer). A cited test asserting
exit 0/1 on a witnessed/violated verdict is therefore genuine replay
evidence: a Monitor rejection would have produced exit 3 and failed the
cited assertion.

| Group | BMC | explicit | induction | replay |
|---|---|---|---|---|
| `invariants` | `Exercised` — `expression_agreement.rs::bounded_verification_rejects_initial_violation_without_action_instances` | `Exercised` — `fsl-runtime/tests/explicit_engine.rs::explicit_engine_totalizes_property_context_zero_division` | `Exercised` — `induction_suggestions.rs::suggests_a_scalar_bound_without_changing_the_verdict` | `Exercised` — `explicit_engine.rs::explicit_and_bmc_agree_on_every_accepted_top_level_corpus_spec` (replays every violation trace, `replayed > 0` asserted) |
| `transitions` | `Exercised` — targeted probe (below) | `Exercised` — same probe | `Exercised` — `induction_suggestions.rs::trans_ctis_never_receive_invariant_suggestions` | `Exercised` — same probe (exit 1, not 3) |
| `reachables` | `Exercised` — `explicit_engine.rs::explicit_reachable_witness_step_matches_bmc` | `Exercised` — same test | `UnsupportedFailClosed` — targeted probe (below) | `Exercised` — same test (witness replay gates the exit) |
| `leads_to` | `Exercised` — `issue_260_leadsto_stagnation.rs::leadsto_deadlock_stagnation_is_detected_beyond_the_deadlock_step_for_plain_kernel_spec` | `UnsupportedFailClosed` — `explicit_engine.rs::explicit_cli_exit_codes_cover_bounded_proved_violated_budget_and_semantics` (exit 2, "does not support leadsTo") | `Exercised` — `induction_suggestions.rs::ranked_leadsto_failures_never_receive_suggestions` | `Exercised` — `replay_trace_contract.rs::overdue_bounded_response_is_liveness_nonconformance_after_safety` |
| `terminal` | `Exercised` — targeted probe (below) | `Exercised` — `fsl-runtime/tests/explicit_engine.rs::explicit_bfs_proves_at_state_space_closure` | `NotApplicable` — terminal's only kernel semantic is deadlock-report exclusion and the induction engine has no deadlock probe (basis: `coverage.rs`'s `terminal_deadlock` row description) | `Exercised` — `fsl-runtime/tests/monitor_regression.rs::action_cover_trace_uses_the_enabling_bool_value_and_may_end_at_terminal` |

Three cells had no existing owner anywhere in the corpus and got targeted
probes in `assurance/properties.rs`, each empirically confirmed against the
real binary before its expectation was written down:

- `bmc_and_explicit_report_and_replay_a_trans_property_violation` — no
  pre-existing test asserted an end-to-end verify run reporting
  `violation_kind: "trans"` on either engine
  (`fixtures/assurance_trans_violation.fsl`).
- `induction_rejects_a_reachable_property_fail_closed` — `--engine
  induction --property <reachable>` must be a usage error naming the
  reason, not a silent skip or vacuous proof.
- `terminal_excludes_the_final_state_from_bmc_deadlock_reporting` — with
  its negative control: the identical machine minus `terminal`
  (`fixtures/assurance_terminal_once_missing.fsl`) must deadlock under
  `--deadlock error`, proving the green verdict on the `terminal` fixture
  is the exclusion at work, not a deadlock probe that never fires.

## Negative controls

`assurance_matrix.rs::negative_controls` demonstrates, without touching any
real axis:

- `a_citation_with_a_fabricated_anchor_fails_recheck` /
  `a_citation_to_a_nonexistent_file_fails_recheck` — `Citation::recheck`
  actually rejects a broken citation.
- `an_unclaimed_required_cell_fails_the_completeness_check` —
  `Axis::check_complete` actually rejects a blank required cell.
- `a_fully_claimed_toy_axis_passes_both_checks` — the positive control:
  proves the two checks above are not vacuously `Err` either.

## Anti-Goodhart

Per issue #537's own guardrail: cell/row count is not a success metric.
The two measurements that matter are "required-blank-cell count stays 0"
(enforced every run by `every_declared_cell_across_every_axis_has_a_claim`)
and "does adding a new registry entry force a matching axis update" — for
`outcome_kind`/`violation_kind` this holds because rows are read from the
single-owner constant, not copied, so a new registered kind with no cell
fails `check_complete` immediately; for a genuinely new `KernelModel`
property group, the anchor is the kernel schema's `properties.required`
list plus `kernel_schema_property_groups_match_the_axis_rows` (see the
`properties` axis section above).

## Slices 2 and 3: expressions, types, and dialects (implemented)

### `expr` axis (slice 2)

`assurance/enum_rows.rs` derives 24 `Expr` rows and four `AggregateKind` rows
through declarations that generate both an exhaustive match on the real enum
and the witnesses used to enumerate it. The earlier plan's count of 22 was
stale: `Call` and `Stage` are live syntax variants, although neither is
allowed to reach a checked Kernel evaluator. The axis and the C6 sweep consume
this shared source-coupled inventory, so adding a variant cannot be repaired by
updating a label match while forgetting its row or generated-model posture.

Declared columns are `Monitor` and `BMC` (28 × 2 = 56 cells). The explicit
engine is deliberately not a third expression column: it drives the same
concrete evaluator as Monitor. It remains present in the C6 agreement sweep
as an independent exploration/verdict/replay control, but does not constitute
a third expression implementation.

- 52 cells are `Exercised`: the 22 evaluator-reachable `Expr` variants and
  four `AggregateKind` rows on both columns.
- Four cells are `UnsupportedFailClosed`: `Expr::Call` and `Expr::Stage` on
  both columns. `PredicateExpander` eliminates valid direct-spec calls and
  `StageResolver` eliminates valid business/requirements stage access during
  lowering. `typecheck.rs::infer_type_inner` rejects either form if it leaks
  into the public Kernel. The targeted control
  `unlowered_call_and_stage_fail_closed_before_evaluator_entry` injects each
  into a parsed typed surface tree, re-runs the ordinary semantic build gate,
  and observes the named rejection before either evaluator can run.

The exercising citation is executable, not prose:
`typed_agreement.rs::expression_variant_sweep_agrees_across_all_three_engines_and_covers_all_types`.
`generator.rs::expression_sweep` supplies 25 deterministic, in-process models:
one for each non-aggregate checked variant and one for each of the four
aggregate kinds (so `Expr::Aggregate` is observed four times). Each model is
parsed and checked, its designated variant is confirmed in the resulting
expression tree, and then Monitor BFS / explicit / BMC verdict, replay, and
sampled-successor admission are compared. Every positive model must be clean;
the test then negates its known-true `Variant` expression and requires all
three engines to report the same step-zero invariant violation. The
`unique`/`exactlyOne` models additionally exercise zero, one, and two matching
bindings. `Expr::EnumMember`, which ordinary
enum tokens do not retain as a direct-spec node, uses the documented
`build_surface_model` typed-AST mutation gate and is re-typechecked before the
same comparison. `sweep_summary.rs` records a per-row
`expression_variants={...}` / `aggregate_kinds={...}` summary; this is the
explicit C6 `sweep_summary` → C3 `Citation` connection planned in slice 1.

### `types` axis (slice 3)

The shared source-coupled declarations in `assurance/enum_rows.rs` likewise
derive all nine `TypeRef` rows and all three `TypeDef` rows through exhaustive
matches plus generated witnesses. Declared columns are `Monitor` and `BMC`
(12 × 2 = 24 cells), all `Exercised`.

The expression family deliberately carries `Int`, `Bool`, `Named`, inline
`Range`, `Map`, `Relation`, `Set`, `Seq`, and `Option` state/type positions,
plus `Domain`, `Enum`, and `Struct` definitions. The C6 test recursively reads
the checked `KernelModel` type inventory, requires all 12 rows, then passes the
same models through concrete and symbolic evaluation. The type cells therefore
cite observed value construction/comparison rather than a parser-only mention.

### `dialects` axis (slice 3)

`assurance/dialects.rs` reads its ten rows directly from
`fsl_syntax::DIALECT_KEYWORDS`, which the `frontends!` macro emits from the
same invocation that builds the dispatch table. No second ten-string row list
exists. `every_registered_dialect_has_a_corpus_representative_and_reviewed_posture`
also derives the observed corpus set with `dialect_keyword`; a frontend
addition fails an explicit rejecting default in each posture mapping until its
CLI, Worker, and corpus ownership are adjudicated.

Declared columns are `CLI check`, `Worker`, and `corpus` (10 × 3 = 30 cells):

- 27 cells are `Exercised`. Ordinary dialect checks cite the native bare-check
  sweep; Worker cells cite the all-corpus normalized-envelope parity harness;
  corpus ownership cites the C4 bare-check/gallery, refinement, or evidence
  manifest appropriate to the dialect.
- Three cells are `UnsupportedFailClosed`. Two are `refinement`: a mapping has
  no standalone Kernel `state`, so native bare `check` refuses it and the
  Worker parity corpus confirms the same refusal. Its actual semantics remain
  owned and exercised by `refine_corpus_parity.rs` under `fslc refine`. The
  third is `agent` × Worker: native runs its lenient agent analysis while the
  Worker has no agent path and stops at the Kernel lowering gate.
  `test-browser.mjs::assertAgentWorkerProbeFailsClosed` is the agent-specific,
  self-retiring error assertion. The harness's other three unsupported probes
  are causal documents, but causal intentionally bypasses `frontends!` and is
  therefore not a dialect-axis row.

### Deferred corpus/induction expansion

The earlier “also” item is not implemented in slices 2/3: the specs-only
BMC/explicit corpus sweep is not extended to `examples/`, and no induction
column is added. That work first needs execution-cost measurement and a
declared representative subset whose properties are genuinely
induction-provable; adding the whole corpus would conflate unsupported or
bounded-only properties with missing assurance. A later independent slice
should measure the expanded sweep, publish the subset rule, and only then
register those columns.
