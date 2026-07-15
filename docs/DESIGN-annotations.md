<!-- SPDX-License-Identifier: Apache-2.0 -->

# Typed declaration annotations

Status: accepted. Typed IR implemented by issue #237; declaration-level
`@...` syntax for the shared spec/business/requirements/compose frontend
implemented by issue #241; `domain`/`dbsystem`/`ai_component` nested
declaration coverage implemented by issue #281.

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
spec/business/requirements/compose frontend. Issue #281 (see "domain/db/ai
nested declaration syntax" below) extended the same grammar to the three
specialized frontends that do not share that parser: `domain`, `dbsystem`,
and `ai_component`.

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
tool authority/approval rules) gained the same `@...` grammar in issue #281;
see "domain/db/ai nested declaration syntax" below.

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

## domain/db/ai nested declaration syntax (issue #281)

`domain` (`rust/fsl-syntax/src/domain.rs`), `dbsystem`
(`rust/fsl-syntax/src/db.rs`), and `ai_component` (`rust/fsl-syntax/src/ai.rs`)
are specialized frontends with their own hand-rolled, token-cursor `*Parser`
structs — none of them share `parser.rs`'s `Parser`. Each therefore carries
its own `pending_annotations: Annotations` field and duplicates the same three
helpers (`take_leading_annotations`/`take_annotations`/
`expect_no_pending_annotations`) rather than reusing `parser.rs`'s `Parser`
directly. All three still call the one shared `annotation_parse::annotation`
free function (`pub(crate)`, same crate) to parse an individual `@name(...)`
group, so there is still no per-dialect copy of the actual `@...` token
grammar — only of the leading/pending-buffer bookkeeping around it.
Unifying the four duplicated pending-annotation state machines behind one
trait is deferred as acknowledged design debt; none of the four parser
structs share a common base to hang it from without a larger refactor this
issue did not need.

Accepted declarations, one attachment point per dialect concept:

| Dialect | Accepts `@...` on |
| --- | --- |
| `domain` | aggregate `command`, `decide`, `evolve`, `invariant`, `projection` (both aggregate-nested and top-level); `effect`; saga `step` |
| `ai_component` | `tool` (including the `tools [a, b];` shorthand, broadcast per name); `authority` block and each of its `may_suggest`/`may_execute`/`requires_human_approval`/`forbidden` rule lines; `fallback` block and each `when` item; `check hard` block and each `rule` line |
| `dbsystem` | `migration`; each `check compatibility { rule ...; }` line |

`ai_component`'s `check hard { rule ...; }` and `dbsystem`'s `check compatibility { rule ...; }` are structurally the same "named-rule list inside a check block" shape and both accept an annotation on each individual `rule` line — matching `authority`'s per-rule-line granularity — but not symmetrically at the block level: `ai_component`'s `check hard { ... }` block itself also accepts a leading annotation (mirroring `authority`/`fallback`, which likewise accept one on their own block), while `dbsystem`'s `check compatibility { ... }` block itself does not (a leading annotation there still reports `FSL-ANNOTATION-TARGET`; only `migration` accepts one at the `dbsystem` top level). Per-rule annotations on either block do not currently reach any Kernel target through the rule name alone (`ai_component` has no real Kernel lowering at all; a `dbsystem` compatibility rule's annotations reach the checked model only when that rule is enabled and generates at least one invariant — see below), so the per-rule granularity here is primarily about API/traceability completeness (a user can name and independently track any individual rule the same way they can any individual tool or authority rule), not about an existing consumption path.

Everywhere else in these three dialects a leading `@...` reports the coded
`FSL-ANNOTATION-TARGET` diagnostic, the same as the shared frontend: a
migration's individual `add`/`drop`/`rename`/... operation line, a `dbsystem`
scalar declaration (`database`/`artifact`/`environment`), an `ai_component`
scalar declaration (`model`/`prompt`/`retriever`/`temperature`/`input`/
`output`), and a domain declaration outside the accepted list (`type`/`enum`/
`value_object`/`state`/`event`/`error`/`on_stale`/`await`/saga
`starts_on`/`compensation`/`outbox`/`inbox`) all reject a stray annotation at
its own precise span rather than silently dropping it or falling through to a
generic parse error.

Union and broadcast follow the same "one generated Kernel target can be fed
by more than one surface declaration" pattern issue #241 established for
`until`'s dual bind:

- A `command`/`decide` pair sharing one command name, plus every `evolve`
  block matching an event that `decide` emits, union into the one synthesized
  `{aggregate}_{command}` action (`domain_lowering.rs::lower_aggregate_actions`).
- One `effect`'s annotations broadcast to every action it generates: each
  `{effect}_complete_{event}` outcome action, the `{effect}_retry` action (if
  `retry.max_attempts` is set), and the `{effect}_SuccessSticky` progress
  property. An outcome-event `evolve` block's own annotations additionally
  union into that event's `complete` action, and into a saga's
  `saga_{saga}_observe_{event}` action when a saga step awaits the same
  event — the same "the annotation follows whichever generated action
  actually executes this declaration" rule the aggregate case uses.
- One saga `step`'s annotations broadcast to both the `saga_{saga}_{step}`
  action and its `_timeout` variant (if the step declares one).
- An aggregate or saga `invariant`'s annotations flow to its one generated
  `SpecItem::Invariant`. Domain-level `invariant` was not in issue #281's own
  declaration list, but is the same kind of aggregate-direct declaration as
  `command`/`decide`/`evolve`; leaving it out would reproduce the exact
  policy/goal asymmetry issue #241's coupled-change review caught, so it was
  included from the start here instead of as a follow-up gap.
- `DomainProjection` and `ai_component`'s `tool`/`authority`/`fallback`/`check`
  nodes are not lowered into any executable Kernel target at all (confirmed:
  `lower_domain_surface` never reads `domain.projections`, and
  `lower_ai_component` is a catalog-sentinel stub with no per-declaration
  content). Their annotations are preserved on the frontend AST and reachable
  through `python_ast()`/direct AST inspection, with no `AnnotationRegistry`
  binding to reach — there is no Kernel target for one to attach to.

`ai_component`'s `AiAuthority` restructuring: `may_suggest`/`may_execute`/
`requires_human_approval`/`forbidden` change from `Vec<String>` to
`Vec<AiAuthorityRule>`, where `AiAuthorityRule { name, annotations, loc }`.
`AiAuthority::python_ast()` reconstructs the original plain `Vec<String>` JSON
shape (`rule.name` only) so the public JSON projection is unchanged; the
typed carrier is additive. Every Rust call site that used to compare a
`&String` name directly (`rust/fslc/src/main.rs`'s specialized-document
validation, `rust/fsl-tools/src/ai.rs`'s `check_ai`/`replay_ai`) was updated
to compare `rule.name` instead.

`dbsystem`'s existing `DbMigration`/`DbMigrationOp` already declare a field
named `annotations: Vec<String>` — a pre-#281, unrelated legacy mechanism
(the `destructive`/`irreversible`/`rollbackable`/`lossy`/`lossless` migration
keyword flags, still projected into `python_ast()` under the same JSON key).
The new typed carrier could not reuse that name, so `DbMigration` gained a
second, distinctly named field: `decl_annotations: Annotations`. Compatibility
rules changed shape instead of gaining a same-named field:
`DbCheck.rules: Vec<String>` became `Vec<DbCheckRule>`, where
`DbCheckRule { name, annotations, span }`; `DbCheck::python_ast()`
reconstructs the original plain `Vec<String>` rule-name list.

`db_kernel_source` (`rust/fsl-core/src/db.rs`) still synthesizes the
dbsystem's executable kernel as FSL source text re-parsed through the shared
spec grammar (`fsl_syntax::parse_surface_spec`) — issue #281 did not replace
that architecture, which would have been a much larger change unrelated to
annotations. What changed is *what floats through the text*: a migration's
`decl_annotations` and a matching `DbCheckRule`'s `annotations` are rendered
back to literal `@requirement(...)`/`@undecided(...)`/`@kind(...)`/custom
`@ns.path(...)` source lines (`Annotation::render_source()`,
`rust/fsl-syntax/src/annotation.rs`) immediately before the corresponding
synthesized `action`/`invariant`, rather than being squeezed through the
lossy `quote_meta("ID", "text")` single-`MetaTag` string convention the
system-generated `DB-MIGRATION`/`DB-NOT-NULL`/`DB-COMPAT-READ`/
`DB-COMPAT-WRITE` labels still use unchanged. Re-parsing then binds them as
ordinary typed `Annotation`s through the exact same
`collect_declaration_annotations` path every other dialect uses, so they
reach `AnnotationRegistry`/`KernelModel`/TSG/the audit ledger with no new
plumbing. Because the lexer has no string-escape syntax
(`rust/fsl-syntax/src/lexer.rs::lex_string` stops at the first `"` or
newline), `Annotation::render_source()` sanitizes backslash, `"`, and
newline characters out of a `String` argument value before quoting it — the
same character class `quote_meta` already sanitizes, extended to cover
newlines too since this is a fresh code path rather than an existing one.
This sanitization only affects a `String` value containing those characters;
Integer, Boolean, and Symbol arguments round-trip exactly.

`tools/check_rust_surface_parity.py`'s `SUPPORTED_SPECIALIZED_FRONTENDS`
already includes `ai-component`/`db`/`domain`; since no new field was
projected into any of these three dialects' `python_ast()`, corpus parity is
unaffected by construction (the tool is not CI-wired — see the Python
compatibility gate note in the repository's CLAUDE.md — but was still run
manually against the corpus to confirm).

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

- `@requirement`/`@undecided`/`@kind`/custom `@namespace` syntax on
  `governance`/`refinement` declarations, or on a `domain`/`dbsystem`
  declaration outside the accepted list above (`DomainType`/`DomainAward`/
  `DomainAwait`/`DomainSagaCompensation`, a `DbMigrationOp`, dbsystem
  `database`/`artifact`/`environment`) — these still report the coded
  `FSL-ANNOTATION-TARGET` diagnostic rather than silently accepting or
  dropping the annotation;
- unifying the four duplicated `pending_annotations` parser state machines
  (`parser.rs`, `domain.rs`, `ai.rs`, `db.rs`) behind one shared trait;
- replacing `db_kernel_source`'s text-synthesis-and-reparse architecture with
  direct `KernelSpec`/`SpecItem` construction;
- formatter or source migrator behavior;
- macro execution or verifier semantics selected by an annotation;
- publishing annotations by mutating Public Kernel v1/v2.
