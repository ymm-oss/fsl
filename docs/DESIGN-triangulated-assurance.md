<!-- SPDX-License-Identifier: Apache-2.0 -->

# Triangulated assurance

Status: accepted. Implemented by issue #670.

## Decision

Soundness-critical conformance anchors may register a CI-internal
`TriangulatedClaim`. A claim is not a new verifier or a vote. It records one
pre-classification observation, a model observer, a semantically independent
observer, the three executable agreement edges between them, positive and
negative calibration, and an explicit scope.

The registry is federated: P1/P2/P3 and future semantic owners construct their
own claims. The `fslc-rust` integration-test aggregator owns only the required
claim IDs and fail-closed validation. This follows the C3 Semantic Assurance
Matrix's `Claim`/`Citation` pattern without turning one central table into the
owner of every semantic decision.

`triangulated` is an internal evidence method. It never changes or promotes the
public `proved`, `bounded`, `replay-observed`, `statistical`, or `not_run`
assurance classes, and it never changes a process exit code.

## Claim contract

Every registered claim contains:

- a stable ID and accepted contract citation;
- a common observation whose kind is raw source bytes, raw process bytes, or a
  raw witness trace;
- model and independent observers, each with an executable citation, semantic
  owner, and non-empty decision lineage;
- executable model↔world, oracle↔world, and model↔oracle edges;
- an executable accepting control and rejecting control;
- command/feature/domain/backend/platform/corpus-revision scope;
- optionally, a calibrated common-mode control.

Every executable reference has an explicit state. Only `executable` is valid;
`skipped` and `unknown` fail validation. Every citation is re-read from the
working tree and must still contain its declaration anchor. Empty fields,
missing/stale claims, stale citations, pre-classified observations, shared
semantic owners, or intersecting semantic decision lineages fail the native
test gate.

The aggregator mechanically rejects obvious common ownership. This is not a
proof of organizational or semantic independence. Review remains responsible
for indirect shared decisions; the declared lineages make that residual trust
boundary visible.

## Common observation

Classification must follow observation. The accepted raw shapes are:

- `raw_source`: `source_bytes` and `source_revision`;
- `raw_process`: `stdout_bytes`, `stderr_bytes`, `process_exit`, and
  `binary_revision`;
- `raw_trace`: `trace`, `step`, `state`, `violation_kind`, and
  `failed_location`.

A production `success`/`failure` or assurance label by itself is explicitly a
`preclassified_verdict` and is rejected. Sharing process spawning, byte
capture, JSON decoding, or fixture loading is allowed because those mechanisms
do not decide the semantic result. Sharing a classifier, lowering decision,
projection, result join, or semantic registry is not independent observation.

## Executable three-edge semantics

The three edge citations point to tests run by the Rust workspace gate:

1. model↔world: the model interpretation accepts the unmodified observation;
2. oracle↔world: the independent interpretation accepts that same observation;
3. model↔oracle: their scoped identities agree exactly.

An edge failure is reported by edge name. There is no majority rule. A claim
with one broken edge is not triangulated even if the other two agree.
`cargo test` establishes that cited test assertions execute; the aggregator's
citation recheck establishes that the named executable has not disappeared or
been silently replaced by an empty registry cell.

## Calibration

Each claim cites a legitimate accepting control and a known-bad rejecting
control. The registry itself has controls proving that missing fields/edges,
fabricated citations, skipped evidence, shared owners, pre-classified input,
and missing calibration are rejected.

At least one registered claim must cite a common-mode control. P1 owns the
implementation fault operator that replaces the registered C7 independent
mapping with the production outcome classifier and declares that shared
semantic lineage. Its primary detector executes the substituted registered
path before the triangulated registry independence check; a parser diagnostic
remains the blind detector. The no-op and stale-seam controls in the existing
fault-operator harness continue to calibrate the harness itself.

## P1 — compound outcome conservation

- world: one real native process observation retaining stdout bytes, stderr
  bytes, exit status, parsed JSON, and the build fingerprint;
- model observer: `examples/self/fslc_session.fsl`, `fslc_monitor.fsl`, and
  `fslc_fold.fsl` replayed by the native CLI;
- independent observer: `self_conformance.rs` mappings that deliberately do
  not import the production outcome classifier;
- calibration: result/exit contradiction, failure-to-success, missing
  evidence, and fold-finalization controls, plus the common-mode classifier
  substitution fault.

The migration is additive to C7. It retains all existing result, exit,
evidence, failure-stickiness, proof, vacuity, and mutation checks.

## P2 — symbolic witness / concrete replay agreement

- world: the BMC witness's complete `TraceStep` sequence plus violation step,
  kind, name, and property source span;
- model observer: native symbolic BMC;
- independent observer: solver-free explicit BFS and `Monitor::replay_trace`;
- edges: the symbolic trace replays, the explicit trace replays, and violation
  identity plus property span agree.

The rejecting control mutates state, trace step, violation kind, and failed
location independently. The concrete oracle replays the trace and recomputes
the violation identity/location; every mutation must cut an edge. Shared
concrete evaluation between Monitor replay and explicit BFS is declared in the
lineage and is not counted as a third observer.

## P3 — token-based dialect dispatch

- world: identical raw source bytes observed by the syntax library, native CLI,
  and LSP index acceptance/diagnostic path;
- model observer: the finite-state dispatch contract in this design and the
  accepted `DESIGN-dialect-dispatch.md` significant-token state order;
- independent observer: a hand-written fixture manifest that does not call the
  production lexer, parser, or frontend registry;
- calibration: annotation-argument keyword, leading BOM/trivia, unknown keyword
  and span, duplicate registry key, plus a legacy raw-prefix classifier mutant.

CLI/library/LSP parity is an oracle↔world edge, not another independent
observer: those consumers intentionally share `parse_document`.

## Scope and ownership

The initial required registry is exactly P1 compound outcome conservation, P2
symbolic/concrete witness agreement, and P3 token dialect dispatch. Adding a
soundness-critical claim requires adding its ID and owner module together; an
unregistered or stale registration fails completeness in both directions.

The frozen Python package remains compatibility evidence only and is not cited
by a triangulated claim. The full product gate remains
`./tools/check-native-integration.sh`, including the Rust workspace, delivery
boundaries, WASM surface, and calibrated implementation fault operators.

## Non-goals

- proving FSL's absolute soundness or implementing three verifiers;
- treating three consumers of one parser/classifier as independent;
- converting every C3 cell in one change;
- covering algorithmic complexity, performance, or memory safety in the same
  claim;
- exposing philosophical terminology or registry metadata in public CLI/JSON.
