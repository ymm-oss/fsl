<!-- SPDX-License-Identifier: Apache-2.0 -->

# Typed declaration annotations

Status: accepted. Typed IR implemented by issue #237; declaration-level
`@...` syntax for the shared spec/business/requirements/compose frontend
implemented by issue #241.

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

Issue #247 subsequently added the document-level `@...` subset needed by
token-based dialect dispatch. `DESIGN-dialect-dispatch.md` is authoritative for
annotations immediately before a top-level document. Issue #241 (see
"Declaration-level syntax" below) added the same `@...` grammar immediately
before a declaration nested inside a document, for the shared
spec/business/requirements/compose frontend; `domain`/`dbsystem`/`ai_component`
nested declarations are a tracked follow-up (#281), not yet supported.

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

Every declaration this carrier covers accepts two coexisting source forms: the
legacy single-string slot (still one optional slot per declaration) and the
`@...` syntax below (any number of stacked annotations). Native lowering
adapts the legacy forms into the same typed carrier:

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

## Declaration-level syntax (issue #241)

`@name(args...)` parsing is one shared implementation
(`rust/fsl-syntax/src/annotation_parse.rs`), used both by `dispatch.rs` for the
one top-level annotation group a document's dialect keyword carries and by
`parser.rs` for annotation groups nested inside a dialect body. There is no
per-dialect copy of the grammar.

`parser.rs`'s `Parser` carries one `pending_annotations: Annotations` field.
Every loop whose body can contain an annotatable declaration
(`spec`/`compose`/`requirements` item loops, a `requirement` block's inner
loop, a `process`'s inner loop) calls `take_leading_annotations()` before
dispatching to that iteration's declaration parser; whichever declaration
constructor is reached next drains the field with `take_annotations()`
(validating the group immediately, matching the top-level `leading_annotations`
behavior). A declaration kind that does not accept annotations never drains
the field, so the next loop iteration's `expect_no_pending_annotations()` (or,
for an annotation immediately before `}`/EOF, `take_leading_annotations()`
itself) reports `FSL-ANNOTATION-TARGET` at the first pending annotation's span.
A block that itself does not accept annotations but opens its own
annotation-aware inner loop (a business/requirements `process`) asserts no
annotation is pending before opening that inner loop, so a stray annotation
before `process` cannot leak onto the first `transition` inside it.

Attachment reuses the existing stable target vocabulary
(`action_target`, `property_target`, `INIT_TARGET`) — no new target-key
functions were needed. `model.rs::collect_declaration_annotations` (renamed
from `collect_legacy_annotations`) is the single place that extends the
`AnnotationRegistry` with a declaration's parsed `annotations` field, right
next to the existing legacy-`meta` adapter for the same target; both channels
simply union (`until` extends both of its generated targets the same way it
binds legacy meta to both). Requirement-block annotations fan out to every
contained action/property target exactly the way the block's own
`Requirement{id,text}` already does; acceptance/forbidden block annotations
extend the trace case's own `Requirement{id,text}` relation
(`dialect.rs::trace_case_annotations`).

`python_ast()` never projects the new `annotations` field (destructured with
`..` in every projection match arm) — the frozen Python oracle has no
annotation concept, `fslc::approval::spec_digest` hashes `python_ast`, and
Public Kernel v1/v2 stay closed contracts. A file that uses `@...` on a
nested declaration is therefore unparseable by the frozen Python reference by
construction (confirmed native-only via `tests/dialect_registry.py`'s
`MONITOR_EXCLUSIONS`) and is excluded from the Python-parity corpus, not
silently mismatched.

`domain`/`dbsystem`/`ai_component` nested declarations (aggregate
command/decide/evolve/effect/saga steps, migration/compatibility rules, AI
tool authority/approval rules) have no metadata channel — legacy or `@...` —
today; only their one top-level document annotation works, via the same
dispatch-level mechanism every dialect keyword gets. Extending `@...` there is
a tracked follow-up (#281), not a gap in this issue's scope: those dialects need a
first-ever nested annotation channel from scratch (and, for `ai_component`'s
authority rules, restructuring a flat `Vec<String>` rule list into
span-carrying nodes), which is independent design work with no legacy-compat
constraint to preserve.

Within the shared frontend itself, `governance` and `refinement` declarations
(`authority`, `control`, `delegates`, `preservation`, `impl`/`abs`/`map`/action
mapping/`preserve_progress`) are also not attachment points — the issue's
declaration list names `init`/`action`/`property`/process `transition`/
`requirement`/`acceptance`/`forbidden`, none of which those two dialects
declare. A leading `@...` there still reports a precisely-spanned diagnostic
(the pre-existing generic `FSL-PARSE` "expected governance/refinement
declaration" error, since those two dialects' item loops were not changed to
hook the shared annotation-parsing mechanism), just not the more specific
`FSL-ANNOTATION-TARGET` code the hooked dialects use for the same kind of
mistake.

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

- `@requirement`, `@undecided`, `@kind`, or arbitrary `@namespace` syntax on a
  `domain`/`dbsystem`/`ai_component` nested declaration (tracked by follow-up
  issue #281; their one top-level document annotation already works, same as
  every other dialect keyword);
- formatter or source migrator behavior;
- macro execution or verifier semantics selected by an annotation;
- publishing annotations by mutating Public Kernel v1/v2.
