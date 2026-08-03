<!-- SPDX-License-Identifier: Apache-2.0 -->

# FSL Logic Test

Status: accepted. Implemented by issue #673.

## Decision

FSL continuously generates valid finite direct-spec models and requires
agreement between its concrete and symbolic semantics. The test is a detector
calibration and exploration layer, not a proof by volume: it never promotes a
public assurance class and never changes a CLI/Worker result or process exit.

`rust/fslc/tests/typed_agreement/inventory.v1.json` is the machine-readable
scope. It inventories finite-domain, structural, property, expression/type,
partial-operation, dialect, semantic-lineage, required-edge, excluded-field,
metamorphic-relation, and tier postures. Its test derives dialect rows from
`fsl_syntax::DIALECT_KEYWORDS`, couples expression/type coverage to the enum
registries, and requires every companion-sweep and R1-R7 test/control anchor to
remain live. A new registry row or removed control therefore fails closed
instead of silently leaving the generation space.

## Valid generation and replay

`generator.rs::logic_case_at_depth(seed, index, depth)` uses a specified
deterministic SplitMix64 mapping and a stable ID
`fsl-logic-v1-s<seed>-c<index>-d<depth>`. Every case
enters the ordinary `parse_kernel_source` and `build_model` path before any
engine observes it. Invalid output is a generator defect, not a discarded
sample.

Failures record the complete source, seed/index/depth and generation
coordinates, named comparison edge and field, and one `FSL_LOGIC_CASE=...`
command that replays only that case. They also retain the structurally
minimized source, coordinates, stable depth-bearing ID, and its replay command.
Iteration order uses no wall clock, hash iteration, or external randomness.

A single-case replay is diagnostic evidence, not a PR or scheduled tier. If a
fix makes that case agree, the command exits successfully but leaves its report
`complete:false`; one replay can never claim full inventory coverage.

## Agreement contract and lineages

The concrete lineage is an independent test-side finite-frontier driver over
`Monitor`, solver-free Monitor BFS, and explicit verification; these share the
concrete evaluator and are consistency edges, not three independent oracles.
Native-Z3 BMC is the symbolic lineage. No
majority vote is permitted. An engine error, missing result, comparator
disagreement, replay failure, or symbolic rejection of a concrete successor
fails the case.

The required named edges are:

- `monitor_bfs`, `bfs_explicit`, and `explicit_bmc` for normalized verdict;
- `generated_expectation` for the generator-owned expected violation step,
  including both initial and action-bearing delayed violations;
- `depth_completeness` for requested-depth preservation plus Monitor/explicit
  depth-reached and concrete-closure agreement;
- exact reachable-property step and action-coverage agreement on clean bounded
  runs;
- `trace_explicit_bmc` for exact state/action/change witness equality when
  both engines return a violation; the edge counts as exercised only for a
  nontrivial trace whose last step names an action;
- `property_location` for resolution of an agreed violation identity to its
  exact checked-model property span;
- `replay` for ordinary concrete replay of explicit and BMC evidence; and
- `successor_admission` for symbolic admission of sampled concrete steps.

Violation identity is `(kind, name, step)`; its property origin is retained in
the report. `leadsTo` is explicitly BMC-only because BFS does not evaluate it
and explicit verification rejects it. `deadlock_step` and raw state counts are
not equated because their engine contracts differ. Monitor and explicit
concrete closure are equated; BMC frontier progress is retained beside them but
not equated because it belongs to a different symbolic bounded proof domain.
Reachable and action-coverage observations after a violation are likewise
excluded because the engines stop auxiliary observation at different points.
The inventory records each reason. These are explicit scope boundaries, not
normalization or majority voting.

## Metamorphic relations, shrinking, and corpus

The existing R1-R7 relations and their negative controls remain the accepted
metamorphic set; the inventory points to executable anchors rather than
copying their contract prose. The comparator has a deliberate corruption
control which must fail on the exact `explicit_bmc:verdict` signature.

The structural shrinker removes explicit dimensions from the same case—depth,
control violation, fairness, guards, actions, state variables, property form,
domain kind, and domain size—through the ordinary parse/typecheck gate while
preserving the named failure signature. Its calibration proves that the final
source is structurally smaller and still reproduces the same signature. Real
disagreements are promoted as ordinary `.fsl` regressions and
inventoried in `typed_agreement/regressions/manifest.v1.json`; the manifest
deduplicates semantic signatures and requires raw observation and replay
commands. The injected control is not a product regression.

The first scheduled calibration discovered that BFS records a reachable-at-init
witness before reporting an initial invariant violation while explicit/BMC
stop auxiliary observation first. The minimized ordinary regression preserves
that raw difference and requires scoped verdict/trace agreement without
reintroducing an invalid post-violation reachable comparison.

## Tiers and completeness

`tools/check-native-integration.sh fsl-logic pr` runs 16 cases at the accepted
PR seed/depth. `fsl-logic scheduled` runs a 3-seed × 3-depth matrix with 128
cases per configuration (1,152 total) on pushes, schedules, manual runs, and
promotion pull requests. An ad-hoc `FSL_LOGIC_SEED` override remains diagnostic
and cannot emit `complete:true`. The v1 report begins with `complete:false`,
records each case as it executes, and becomes complete only when the expected
count ran and every required edge and selected axis was observed. The wrapper
independently requires
`expected == executed == cases.length` and `complete:true`; a timeout, missing
artifact, invalid schema, or partial shard cannot read green.

The report schema is
`schemas/fslc/assurance/fsl-logic-report.v1.schema.json`. CI retains it as an
artifact and the product-gate aggregator requires the applicable FSL Logic
lane. The test invokes only the native Rust product.
