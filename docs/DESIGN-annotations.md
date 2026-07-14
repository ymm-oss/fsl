<!-- SPDX-License-Identifier: Apache-2.0 -->

# Typed declaration annotations

Status: accepted. Implemented by issue #237.

## Decision

FSL frontends share one typed annotation IR:

- `Requirement { id, text, span }` is a traceability relation;
- `Undecided { reason, span }` records an intentionally deferred choice;
- `Kind { id, text, span }` classifies a declaration without changing it;
- `Custom { namespace, arguments, span }` preserves an extension whose semantics
  are not owned by the verifier.

`AnnotationValue` is closed over string, integer, Boolean, and namespaced-symbol
values. `SymbolPath` rejects empty namespace segments. Every annotation retains
its own source span; declaration provenance remains in `OriginRegistry` and never
uses a requirement ID as source identity.

## Carrier and declaration coverage

`Annotations` preserves source order for formatter/comment fidelity while its
semantic queries are deterministic. `AnnotationRegistry` is a target-keyed
sidecar carried by `KernelSpec` into `KernelModel`. A sidecar avoids adding a
dialect-specific metadata field to every business, requirements, domain, DB, and
AI syntax node while allowing their declarations—including generated action and
property targets for command/decide/evolve, effects, saga steps, migrations, and
authority rules—to receive the same `Vec<Annotation>`-equivalent carrier.

Checked actions, safety/reachability properties, progress properties, and init
also expose their resolved `Annotations` directly. Acceptance and forbidden
trace cases carry the same typed `Requirement` relation even though those cases
remain outside executable `KernelModel` state.

The stable target vocabulary follows the origin graph (`spec`, `init`,
`action:<name>`, `property:<kind>:<name>`, and dialect-generated targets). `until`
binds its annotations to both generated semantic targets: the transition-safety
property and the `leadsTo` property.

## Adapters and compatibility

Declarations retain their optional legacy string slot. Issue #247 additionally
allows typed `@...` metadata immediately before a top-level document declaration;
`DESIGN-token-registry.md` supersedes the earlier parser-syntax non-goal only for
that document boundary. Native lowering adapts all existing sources into the
typed carrier:

- declaration `"REQ-3: text"` becomes `Requirement`;
- declaration `"undecided: reason"` becomes `Undecided`;
- the spec-level badge becomes `Kind`;
- requirements-process `covers` and requirement blocks become `Requirement`;
- acceptance and forbidden IDs become `Requirement` on their trace cases.

When a requirement block supplies a requirement and an inner declaration carries
legacy metadata, lowering merges both annotations instead of overwriting the
inner one. Thus an outer requirement and inner `undecided` marker coexist today
without adding `@...` syntax.

Explicit requirement syntax retains the span of the complete annotation header
(`covers ID "text"` or `requirement ID "text"`), rather than borrowing the
enclosing declaration span.

`MetaTag`, singular `requirement` diagnostics, and `KernelModel::requirement_for`
remain compatibility projections. The authoritative APIs are
`KernelModel::annotations_for` and `KernelModel::requirements_for`. A singular
projection chooses the lexically first normalized requirement, never the first
source annotation.

Outputs that can evolve add a deterministic `requirements` array while retaining
their singular `requirement`, `meta`, or `tag` field as that compatibility
projection. Strict-tag matching, TSG requirement edges, scenarios, verifier
diagnostics, and the audit ledger consume the full typed relation.

Public Kernel v1 and v2 are closed contracts and remain byte-compatible. This
issue does not add an `annotations` field to either schema. Publishing the full
carrier requires a negotiated future schema major.

## Validation and semantic isolation

- Empty requirement IDs, empty undecided reasons, empty kind IDs, and invalid
  custom namespaces are checked-model errors.
- `undecided` is reserved for the built-in marker and is rejected as an explicit
  requirement ID.
- Identical `(id, text)` requirement relations are de-duplicated.
- One requirement ID with conflicting text is an error at the conflicting
  annotation span.
- Requirement queries are lexically normalized, so changing annotation order
  cannot change checked semantics.
- `Kind` and `Custom` never alter guards, actions, property kinds, verification,
  or lowering behavior. A dialect-specific modifier such as DB `destructive`
  remains separate from this common custom-annotation carrier.

## Superseded limit

`DESIGN-undecided.md` originally listed multi-tag declarations as a non-goal for
issue #189. This design supersedes only that carrier limit. The existing
undecided semantics—metadata rather than a proof condition, no finding
suppression, and exact declaration matching—remain unchanged.

## Non-goals

- declaration-local `@...` syntax below the document boundary;
- formatter or source migrator behavior;
- macro execution or verifier semantics selected by an annotation;
- publishing annotations by mutating Public Kernel v1/v2.
