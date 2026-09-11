# DESIGN: conformance test plan (issue #844)

## Purpose

`fslc conformance` emits a bounded exhaustive oracle. Until this design, no
test-generation path consumed its rejecting half: on `specs/job_pipeline.fsl`
at depth 4, 271 of 406 vectors were `requires_failed` and unused. Each is a
fully decided "this action instance is invalid in this state" fact.

`fslc testplan` selects those vectors — and the accepting/rejecting pair at
each parameter guard boundary — into a closed, versioned artifact that an
emitter or an external consumer can render into tests.

## Why a subcommand and not an external tool

`DESIGN-kernel-contract.md` designs the Public Kernel as a boundary for
external compilers, generators and analyzers, and an external planner would be
consistent with it. It is rejected here for one measured reason: **there is no
semantic digest binding a Kernel JSON to a conformance JSON.** An external tool
can compare only the spec name and the schema version, so a caller that hands
it two documents produced from different snapshots gets a plan built from a
mismatched pair, undetected.

`rust/fslc` builds both documents from **one checked `KernelModel`** in a single
process, which removes the pairing question instead of answering it.

## Boundaries

```text
source read once
  -> checked KernelModel
  -> Public Kernel v1 JSON
  -> conformance v1 JSON
  -> fsl-tools JSON-only planner
  -> target-neutral test-plan.v1
```

The `fsl-tools` planner receives neither `KernelModel`, private AST, `Monitor`,
nor solver. The `fsl-runtime` dependency boundary is unchanged, and the planner
cannot re-derive an outcome: `oracle.evaluator_reimplemented` is a schema
`const false`, and the planner has nothing to reimplement it with.

`fslc testgen`'s output, bytes and baked walk are unchanged; existing goldens
are untouched.

## A plan is a selection, never a verdict

`test-plan.v1` **requires** `formal_result: "not_run"`,
`assurance_effect: "none"`, and a `do_not_assume` array of at least four
entries. The emitted entries are:

- not proof of implementation correctness
- not exhaustive beyond the declared depth and finite scope
- selection coverage is not completeness
- does not replace `verify`, induction, `replay`, or refinement

This follows `DESIGN-triangulated-assurance.md`: producing more evidence must
not promote assurance or exit status. `validate_test_plan_rejects_assurance_escalation`
is the rejecting control.

`selection_coverage` is a descriptive record, not a check. It reports
`vectors_available`, `vectors_selected`, and the ids left **uncovered**, so a
plan cannot hide what it skipped by reporting only a ratio.

## Layer selection (#968 / #643)

A plan is only meaningful against an implementation at the spec's own layer.
`layer_selection.requirement` and the CLI help both state: pass the spec at the
same FSL layer granularity as the implementation you are checking; from an
upper layer reuse `forbidden` (negative) scenarios only.

This is **stated, not enforced**. Slice 1 records no source layer, model digest
or mapping provenance, so nothing mechanically prevents passing an upper-layer
spec. Cross-layer provenance is deferred (below), and until it exists the
wording is the only guard.

## Boundary case selection

For each `requires` guard of the direct form *parameter op integer-literal*
(`<=`, `<`, `>=`, `>`, in either operand order), the planner selects the
accepting vector at the boundary and the rejecting vector one step past it,
named `boundary_accept_<param>` and `boundary_reject_<param>`.

Guards outside that shape — `q + 1 <= 2`, `queue.size() < QCAP`, state-shaped
guards such as `jobs[j].st == New` — yield no boundary case. That is why
`specs/job_pipeline.fsl` produces none. The omission is visible only through
`selection_coverage.uncovered`; widening the recognized shape is future work,
not a defect of this slice.

## Frontier successors are not an error

`conformance` enumerates states up to the requested depth but still emits every
vector leaving the frontier, so at the boundary depth an `ok` vector's successor
is legitimately absent from `states[]` (at `--depth 0`, `states[]` is the
initial state alone). Shortest-path construction skips such an edge: an edge to
a non-enumerated state can never lie on a shortest path *to* an enumerated
state, so no setup path is lost. Snapshot disagreement is caught by the spec
name and `kernel_schema_version` comparisons and by outcome-kind validation, not
by this adjacency pass.

## Slice 1 scope

A single spec argument, one snapshot, plan JSON to stdout. Not in this slice:

- `--target` / `-o` rendering through the existing emitters
- cross-layer provenance: source layer, model digest, mapping
- integration with requirements-layer `forbidden` declarations
- recognizing non-direct guard shapes as boundaries
