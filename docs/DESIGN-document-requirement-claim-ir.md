<!-- SPDX-License-Identifier: Apache-2.0 -->

# Requirement Claim IR (RCIR) v1 and the document projector

Status: accepted. Implements issue #325.

This design adds the foundation of the `fslc document` feature family: a versioned
public contract, the Requirement Claim IR (RCIR) v1, and a deterministic projector
from the checked kernel model into that contract. CLI wiring is issue #327; the
controlled-language renderer, drift checking, the no-silent-omission gate, the
glossary sidecar, the evidence overlay, and approval integration are issues
#326/#328-#334 and are out of scope here. This issue ships a schema plus library
functions callable from Rust integration tests; nothing in the CLI changes.

## Goal and guarantee boundary

RCIR is the handoff between a checked `spec` or `requirements` dialect model and a
natural-language requirements document. The normative content of such a document is
compiled deterministically from checked semantics; an agent may only edit
non-normative slots (issue #329). Within that pipeline RCIR guarantees:

1. every claim kind it supports is projected without silent omission — every
   authored semantic target is classified into exactly one of `rendered`,
   `unattributed`, or `unsupported`, and the projector fails closed if that
   partition is not exact;
2. conditions, negation, effects, simultaneous update, and fairness remain in the
   embedded Public Kernel v2 contract; RCIR binds document roles to those nodes by
   stable semantic target, so a renderer does not accept an unpaired model;
3. digests exist at spec, claim-set, and claim granularity so drift in the
   projected meaning set is mechanically detectable;
4. unsupported semantic elements and unknown provenance are reported, never
   dropped or fabricated.

RCIR does not guarantee that the FSL captures the original business intent, that
requirements absent from the FSL do not exist, that agent-authored free text
matches the FSL, or that an implementation conforms to the FSL.

## Contract position and versioning

RCIR is a public role-and-traceability sidecar over Public Kernel, with independent
schema and versioning. The interoperability unit is one RCIR object containing an
exact Public Kernel v2 object:

- schema file: `schemas/fslc/document/requirement-claims.v1.schema.json`
- schema `$id`: `https://fsl.dev/schemas/fslc/document/requirement-claims.v1.schema.json`
- instance fields: `schema_version` (semver; consumers check the major),
  `result: "requirement_claims"`.

RCIR is explicitly not a second semantics. `public_kernel` is validated by
`kernel.v2.schema.json`; claim records contain typed subjects and stable semantic
targets, not another expression/statement AST. Trace-case arguments and expression
expectations, which do not occur in the model contract itself, reuse Public Kernel
v2's expression projection and schema definition. RCIR defines no new logical
operators or evaluator. The historical Python-shaped AST is used only as an
internal, location-neutral digest preimage and is never serialized as RCIR.

Adding an optional field or a new claim kind is a minor version change; changing a
required field, a digest preimage, assurance meaning, or a classification rule is a
major change.

## Supported dialects

v1 supports direct `spec` and `requirements` only. `business`, `governance`,
`dbsystem`, `domain`, `ai_component`, `compose`, `refinement`, and `agent` are
rejected at the entry point with `"document projection does not support dialect
'<name>' in RCIR v1"`; the projector never emits a partial artifact for them.

## Top-level model

An RCIR artifact is one JSON object with exactly these top-level members:
`$schema`, `schema_version`, `result`, `spec`, `public_kernel`, `semantics`, `requirements`,
`claims`, `trace_cases`, `undecided`, `analysis_scope`, `coverage`, `provenance`.

- `spec` names the specification, its dialect, its portable source path
  (repository-relative, or `null` when the caller does not supply one — the
  projector never guesses it), and the three digests below together with their
  algorithm-name constants (self-describing, following the approval-record
  convention of naming the algorithm beside the value).
- `public_kernel` is the exact Public Kernel v2 artifact for the checked model.
  Its schema is referenced directly by the RCIR schema, so malformed or mismatched
  semantic payload cannot be hidden behind an otherwise-valid claim sidecar.
- `semantics` states the fixed FSL execution model the renderer must phrase
  exactly once: `updates: "simultaneous"`, `reads: "pre_state"`,
  `failed_step: "rollback"`, `fairness: "weak"`. These are closed constants in v1.

## Claim records and claim kinds

A claim's ID is `<semantic_target>#<kind>` (the semantic target plus document role
is the primary identity; `claim_digest` is the separate content identity). v1 claim
kinds and their authoritative sources:

| kind | source | payload |
| --- | --- | --- |
| `operation` | `ActionDef` | `subject` (action name, `display_name`, parameters), `fairness`; guards, effects, and postconditions resolve through `semantic_targets` into Public Kernel |
| `state_rule` | `KernelModel::invariants` (except requirements-NFR-deadline-generated ones) | property subject and semantic target |
| `transition_rule` | `KernelModel::transitions` | property subject and semantic target |
| `progress_rule` | `KernelModel::leadstos` | property subject and semantic target |
| `reachability_goal` | `KernelModel::reachables` | property subject and semantic target |
| `acceptance_trace` | `requirements_trace_contract` acceptance cases | `trace` (a reference into `trace_cases`) |
| `forbidden_trace` | `requirements_trace_contract` forbidden cases | `trace` (a reference into `trace_cases`) |
| `deadline_rule` | invariants generated by requirements NFR deadline lowering | property subject and semantic target |
| `terminal_rule` | `KernelModel::terminal` | terminal subject and semantic target |

Rules:

- Source lines are never used as prose; meaning always comes from checked nodes.
  Spans are provenance only.
- Requirement source text and formal meaning stay in separate records. The
  projector never fuses them into one sentence, because the verifier has not
  proven the prose matches the formalization.
- **Deadline detection is a dialect-scoped name heuristic.** Requirements NFR
  deadline lowering (`rust/fsl-core/src/dialect.rs`, `lower_requirements`) emits an
  invariant named `_deadline_<safe_id>_<age_name>_<n>` carrying a `Requirement`
  annotation copied from the enclosing `requirement`/`deadline` declaration. The
  projector classifies an invariant as `deadline_rule` when the model's dialect is
  `requirements` *and* the name starts with `_deadline_`; a same-named invariant in
  a direct `spec` is not reclassified (user identifiers may legally start with
  `_`, so the heuristic is intentionally scoped to the one dialect that actually
  generates this prefix). A dedicated `LoweringStep` marker would remove the
  heuristic but would also regenerate Kernel v2 goldens, so it is deferred.
- Many-to-many: `claim.requirements` holds every requirement ID attached to the
  claim's target(s); a requirement's `claim_ids` holds every claim it covers. RCIR
  has no singular compatibility projection (no `requirement`/`requirement_for`
  field anywhere) — a claim with two requirement IDs keeps both, and a renderer
  links to one canonical claim anchor from each requirement's section instead of
  duplicating prose.
- An acceptance/forbidden case's own ID is itself a requirement relation: native
  lowering folds requirement blocks, process `covers`, and acceptance/forbidden IDs
  into the same typed-annotation carrier (`skills/fsl/reference.md` sec. 10), so
  `case.annotations.requirements()` already includes `{id: case.id, text:
  Some(case.text)}` even without an explicit `@requirement(...)`.

## Requirement records

Requirement IDs are aggregated across every contributing target (actions,
properties, leadsTos, terminal, and trace cases) via each target's
`requirements_for`/`case.annotations.requirements()`, keyed by requirement ID
rather than by target:

- `statements`: every distinct `(text, source)` pair recorded for the ID,
  deduplicated by exact text (including `None`); multiple different original texts
  for one ID are preserved, not an error.
- `claim_ids`: every claim covering the ID, sorted.
- `kinds`: `@kind(...)` annotation IDs collected from every contributing target's
  own annotations (`Annotation::Kind`), sorted and deduplicated. This is a v1
  interpretation — the issue's example includes a `kinds` field but does not fully
  specify its source — scoped to the declarations that actually carry the
  requirement, not the whole spec.

## Trace cases

`trace_cases` holds the executable detail of acceptance and forbidden cases from
`fsl_core::requirements_trace_contract` (a from-source reparse, independent of the
`KernelModel`/origin registry): ID, kind, source text, ordered steps (action name,
typed Public Kernel expressions for arguments, and each step's own source
position), optional expectation (`expr` or `stage` form), source position, and the
same requirement-ID list as its `acceptance_trace`/`forbidden_trace` claim. The
claim itself carries only `{"trace_case": id}` — the resolved steps live in
`trace_cases`; a renderer joins by ID. The claim's digest core, however, embeds the
*resolved* trace (steps without their coordinates) so a step edit changes the claim
digest even though the claim record itself only holds a reference.

## Undecided, analysis scope

- `undecided` reuses `fsl_tools::undecided::undecided_records` (issue #189), a new
  typed sibling of `undecided_declarations` that additionally carries the
  annotation's span; `undecided_declarations`'s existing JSON output and callers
  are unchanged (it is now a thin projection over the typed records). Undecided
  items are metadata, never claims, and never verification conditions.
- `analysis_scope` carries authored `verify { instances ...; values ... }` bounds.
  **These do not survive kernel lowering**: `entity`/`number` declarations plus
  their `verify` bounds are fully consumed by `lower_requirements` into concrete
  bounded types (an `entity Case` + `verify { instances Case = 2 }` becomes a plain
  `type Case = 0..1`), so nothing is left to scan in the lowered `KernelSpec`. The
  `_from_source` entry point instead reads `RequirementsItem::Common(SpecItem::
  VerifyBounds { .. })` directly from the parsed `SurfaceRequirements` — the same
  surface tree it already inspects for `implements` names — before lowering
  erases it, and passes the result into `DocumentInput::analysis_scope`. Direct
  `spec` dialect has no `entity`/`number`/`verify` concept at all, so its
  `analysis_scope` is always empty. Scope is disclosure of analysis coverage, not a
  system limit, and is never merged into requirement bodies.

## Provenance

Source provenance and requirement relations are distinct (the origin module
enforces that `MetaTag.id` never becomes an origin identity). Each claim carries:

- `origin_ids`: internal `OriginChain` identity strings for its semantic targets,
  stable only for the exact source revision (`identity_stability:
  "exact_source_revision"`, the same contract Kernel v2 publishes);
- `sources`: known source positions;
- `assurance`: the Public Kernel v2 vocabulary reused verbatim — `source_backed`,
  `generated_from_source`, `generated_only`, `unknown`.

**The origin registry (`OriginChain`/`OriginSite`) is sparse outside the domain
dialect today** (`docs/DESIGN-kernel-origin-v2.md`: "the carrier is currently
richest for the domain dialect"); a real `requirements`/`spec` spec's actions and
properties routinely have zero bound `OriginChain`s. Relying on the registry alone
would report `unknown` for nearly every claim in the two dialects RCIR v1 targets,
which is truthful but not useful. `provenance_for` therefore prefers the origin
registry when it has a chain for a target (it is the richer signal where present,
distinguishing `generated_from_source` from `generated_only`), and falls back to
the claim's own checked-model span (`ActionDef::span`, `PropertyDef::span`,
`LeadsToDef::span`) as `source_backed` when the registry has nothing — this is not
a guess, it is the same span the parser already attached to the authored
declaration. Only a claim with *neither* signal reports `unknown`; in v1 that is
exactly `terminal_rule` (`KernelModel::terminal` is a bare `Expr` with no span of
its own) when the registry has no `terminal` entry, and it is exactly what
`build_surface_model` (an empty-registry `KernelModel`) demonstrates. Trace claims
(`acceptance_trace`/`forbidden_trace`) always report `source_backed`: their
`line`/`column` come from a direct, independent reparse of the real source text,
never from the origin registry, so they carry a genuine position regardless of the
model's registry state.

Top-level `provenance.completeness` is `complete` when no claim is `unknown`,
`unknown` when every claim is (or there are no claims), and `partial` otherwise.

This assurance vocabulary is deliberately disjoint from the verification assurance
classes (`proved`/`bounded`/`replay-observed`/`statistical`/`not_run` in
`docs/DESIGN-assurance-classes.md`). RCIR v1 carries no verification evidence at
all; the evidence overlay (#332) layers that vocabulary separately.

## Coverage: fail-closed three-way classification

The authored-target universe of an RCIR artifact is: `init` (when an init block
exists), every `action:<name>`, `property:invariant:<name>`, `property:trans:<name>`,
`property:reachable:<name>`, `property:leadsTo:<name>`, `terminal` (when declared),
`acceptance:<id>`/`forbidden:<id>` from the trace contract, `projection:<name>` for
KPI projections, and `refinement:<name>` for each surface `implements` block.
Vocabulary declarations (types, enums, consts, state fields, `time` items) are
claim inputs, not claims, and are outside the universe.

Every universe member is classified exactly once:

- `rendered`: projected to a claim carrying at least one requirement ID;
- `unattributed`: projected to a claim with no requirement ID — the claim is still
  emitted;
- `unsupported`: not projected, with a machine-readable reason. v1 unsupported
  targets are `init` (no v1 claim kind), `projection:<name>` (KPI is a
  business-layer artifact), and `refinement:<name>` (the correspondence belongs to
  the refinement contract; RCIR still reports that the spec claims conformance to
  another layer, rather than silently omitting it).

`rendered ∪ unattributed ∪ unsupported == universe`, pairwise disjoint, is asserted
by the projector itself (not only tested): a violation is a projector bug and fails
closed with an error rather than emitting a partial artifact.

## Digests

All three digests use one framing: `sha256(<algorithm-name bytes> || 0x00 ||
<canonical JSON bytes>)`, rendered `"sha256:<64 hex>"`. Canonical JSON is compact
serialization of a recursively key-sorted value (the workspace enables serde_json
`preserve_order`, so sorting is explicit, in `fsl_tools::document_digest::
canonical_value`).

| digest | algorithm | preimage |
| --- | --- | --- |
| `spec_digest` | `fsl-kernel-ast-v1+sha256` (same algorithm name as `fslc approval`) | the location-normalized kernel AST |
| `claim_set_digest` | `fsl-rcir-claim-set-v1+sha256` | spec name/dialect, `semantics`, every requirement core (id, kinds, statement texts — sorted), every claim core (sorted by id), every trace-case core (sorted by id), undecided cores, `analysis_scope`, and the unsupported list |
| `claim_digest` | `fsl-rcir-claim-v1+sha256` | one claim's own core |

**Digest implementation is intentionally independent from `fslc::approval`**,
even though both use the identical algorithm name and byte-framing recipe. Moving
`fslc/src/approval.rs`'s private `normalized_ast`/`sha256_bytes` into a shared
location and having `approval.rs` delegate was considered (it would guarantee the
two producers can never silently diverge) but was deferred as a larger, separate
refactor of already-shipped, tested code than this issue's scope calls for;
`fsl_tools::document_digest` reimplements the same ~30-line recipe instead. A
future change that unifies them is compatible with this design (the algorithm name
and output format are the public contract, not the implementation).

A claim core is the claim record minus `source`, `subject.display_name`,
`provenance`, and `claim_digest`; `requirements` is present only as ID strings, not
text (text lives in the requirement record, so requirement-text-only edits move
`claim_set_digest` but not any `claim_digest`). Trace claim cores inline the
resolved trace core (`{id, steps: [{action, args}], expectation}`, no step
coordinates) instead of the `{"trace_case": id}` reference, so a step edit changes
the claim digest. Expression ASTs inside cores are location-normalized, so
comment-, formatting-, and line-shift-only source changes are digest-neutral,
while deleting a `not`, removing a guard, editing or retargeting an assignment,
renaming a referenced enum member, dropping `fair`, or weakening an invariant all
change exactly the digests of the claims whose cores embed the changed node — never
an unrelated claim's digest, because each `claim_digest` is computed by hashing
that claim's own core in isolation (no cross-claim byte sharing). Type-definition-
only edits that no claim references move `spec_digest` but not `claim_set_digest`
(v1 does not render a type inventory, so the claim set genuinely did not change).

## Determinism

Claims are sorted by ID; trace cases by ID; requirement statements by
`(text, source)`; requirement/claim-ID lists and coverage lists lexically; guard
and step order is declaration order (semantic order, not re-sorted). Two runs over
the same source bytes produce byte-identical artifacts.

## Implementation placement

The projector lives in the `fsl-tools` crate, not in `rust/fslc/src/document.rs` as
the issue text sketched, following the existing convention that every comparable
generator (`ledger`, `html`, `undecided`, `typestate`, `testgen`) lives in
`fsl-tools` with `fslc` as a thin argument-parsing and envelope layer. Issue #325
ships library-level functions exercised directly by Rust integration tests, with no
CLI surface (that is issue #327's scope), so there is no reason to place it in the
bin crate. Files, matching the crate's existing flat-module style (`analysis.rs` /
`analysis_export.rs` / `analysis_graph.rs` are siblings, not nested):

- `rust/fsl-tools/src/document.rs` — public types (`RequirementClaimSet`, `Claim`,
  `ClaimKind`, `Requirement`, `Coverage`, `ProvenanceSummary`, ...)
- `rust/fsl-tools/src/document_digest.rs` — canonicalization and the three digests
- `rust/fsl-tools/src/document_project.rs` — the projector
- `schemas/fslc/document/requirement-claims.v1.schema.json` — the contract

`fsl-tools` gains the workspace `sha2` dependency and a `jsonschema` (default
features disabled — no remote `$ref` resolution is needed) dev-dependency for
schema-conformance tests.

The projector reimplements none of: parser, name resolver, type checker, expression
evaluator, transition semantics, solver encoding, or assurance classification. It
classifies checked nodes and serializes them.

## Verification evidence

`rust/fsl-tools/tests/document.rs` (29 tests) against
`examples/pm/cancel_system.fsl` and two dedicated fixtures
(`document_claims_fixture.fsl` — kernel-wrapper `requirements` covering `trans`,
`reachable`, `leadsTo`, `terminal`, a deadline, `forbidden`, a multi-statement
requirement, a multi-requirement action, `@undecided`, and `entity`/`number`/
`verify` bounds; `document_kpi_fixture.fsl` — the process+data profile plus a
`kpi` projection):

- projection completeness (exact target-universe partition, all nine claim kinds,
  unattributed claims are still emitted, dialect rejection is fail-closed);
- many-to-many requirement relations (no singular compatibility projection
  anywhere in the serialized artifact), including identical statement text from
  distinct source declarations without provenance collapse;
- nine mutation-sensitivity cases (`not` removal, guard removal, assignment
  removal, assignment retarget, enum-member rename, fairness removal, invariant
  weakening, acceptance-step change, forbidden-step change), each asserting the
  targeted claim's digest changes, an unrelated claim's digest does not, and
  `claim_set_digest` moves;
- digest stability under comment/formatting-only edits and under a
  requirement-text-only edit (claim digests unchanged, `claim_set_digest` moved);
- provenance (`source_backed` in the ordinary case; `unknown` only for
  `terminal_rule` under a stripped origin registry, never for claims with a real
  declared span);
- undecided/analysis-scope separation from claims;
- schema validation of every fixture's output against
  `requirement-claims.v1.schema.json` via the `jsonschema` crate, plus one negative
  control (a truncated document fails validation).
