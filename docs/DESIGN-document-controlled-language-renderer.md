<!-- SPDX-License-Identifier: Apache-2.0 -->

# Controlled-language renderer (ja/en) for RCIR

Status: accepted. Implements issue #326.

This design adds `fsl_tools::render_requirements_document`, which converts an RCIR
v1 claim set (issue #325, `docs/DESIGN-document-requirement-claim-ir.md`) into
deterministic Japanese/English Markdown. Normative text comes from fixed
per-claim-kind templates; no LLM ever authors normative content. CLI wiring is
issue #327.

## Inputs and architecture

```rust
pub fn render_requirements_document(
    claims: &RequirementClaimSet,
    kernel: &KernelSpec,
    model: &KernelModel,
    trace_contract: Option<&RequirementsTraceContract>,
    locale: Locale,
) -> Result<RenderedDocument, String>
```

The renderer takes the RCIR claim set (for structure: sections, requirement
grouping, coverage, undecided, analysis scope, digests) *and* the original
checked `KernelSpec`/`KernelModel` it was projected from (for content: safe-pattern
recognition and canonical-text fallback). It does not re-derive natural
language from RCIR's JSON AST. Each claim's `semantic_targets`/`subject` field
is used to look the original `ActionDef`/`PropertyDef`/`LeadsToDef`/
`RequirementsTraceCase` back up in the model (by name, using the same
`action_target`/`property_target` vocabulary the RCIR projector used), so
expression rendering runs directly against `fsl_syntax::Expr` — the type
`fsl_core::expr_text`/`source_expr_text` already knows how to print. Given
this, `trace_contract` must be `requirements_trace_contract(source)`'s result
for the *same* source RCIR was projected from (`None` for direct `spec`,
which has no acceptance/forbidden cases). Before producing any Markdown the
renderer regenerates Public Kernel v2 and requires byte-for-byte equality with
`claims.public_kernel`; every claim subject must identify its sole semantic
target and canonical claim ID, and every semantic target must then resolve
exactly once. Acceptance and forbidden trace IDs, ordered steps, arguments,
source positions, and expectations must also equal RCIR's projected
`trace_cases` payload. A mismatched valid model, role mapping, trace payload,
or missing/duplicate target therefore fails closed instead of degrading to
metadata-only or contradictory prose. Issue #327's CLI computes these checked
inputs once and passes them to both projector and renderer.

`fsl_core::expr_text`/`source_expr_text` — the exact function `explain
--readable` already uses to print a checked expression back as readable FSL
text — moved from the `fslc` binary crate into `fsl_core` in this issue (a
pure relocation, `fslc` re-exports both names unchanged, verified by the
existing `cli_regression.rs` test and every `explain --readable` caller).
This was necessary, not cosmetic: `fsl-tools` cannot depend on `fslc` (the
dependency runs the other way), so without the move, the canonical-fallback
renderer would have needed a second ~150-line precedence-aware
expression-to-text implementation — exactly the kind of second-implementation
drift risk the project avoids elsewhere.

## Safe-pattern expression rendering

`fsl_tools::document_render_expr` recognizes a fixed whitelist of `Expr`
shapes and renders each as a natural-language proposition or noun phrase;
everything else falls back to a fenced canonical-FSL block (`source_expr_text`
output). Falling back is never a failure — an unnatural paraphrase is worse
than a visible canonical expression — and it is counted in
`RenderedDocument::formula_fallback_count` (a future `--strict-rendering` CLI
flag, #327, can turn a non-zero count into an error for organizations that
require full natural-language coverage).

Recognized atoms: enum-member equality, `Stage` equality, a bare `Bool`
term, numeric comparison (`==`/`!=`/`<`/`<=`/`>`/`>=`), set membership
(`.contains(...)`), `option` `is none`/`is some(...)`, and the `true`/`false`
literals — each with its negated form where FSL allows one (`not`, `!=`).
Connectives: a 2-operand `and`/`or` chain renders inline; a 3+ chain renders
as an introductory sentence plus a numbered list; `not (A and B and ...)`
where every conjunct is an atom (the N-AND special case) gets its own fixed
"do not all hold at the same time" template — none of these ever apply De
Morgan, factor an expression, or reorder operands. `A => B` recognizes each
side as either a bare atom or a parenthesized same-connective group of 2-3
atoms. `forall`/`exists` fuse their binder into the rendered body's
introduction ("for every `c: Sub`, ..."); a *nested* quantifier is never
recognized — only the outer binder reads naturally, and the inner body falls
back on its own, so a fallback never discards the part that *is* safe.
`count`/`sum` render as a noun phrase (a term, usable as a comparison
operand, e.g. inside `churn_count == count(...)`); `unique`/`exactlyOne`
render as a proposition. `old(x)` unwraps to "the pre-transition value of
`x`" as either a term or the subject of a comparison. A recognized atom's
embedded canonical code span is capped at 60 Unicode scalars — a longer span
(a large struct/set/seq literal, typically) bumps the *whole enclosing
proposition* to the fallback block rather than embedding an unreadably long
inline code span.

This scope is intentionally the set of shapes issue #326 named plus what the
three test fixtures below actually exercise; anything else (a `Method` call
other than `contains`, a named relation helper such as `rel_acyclic`, a
struct/map/set equality, `Not` of a non-atom other than the N-AND case) falls
back rather than guessing a new template.

## Per-claim-kind templates

Each of RCIR's nine claim kinds has one fixed Markdown template (ja/en), with
the semantic-fidelity rules issue #326 requires baked in as *structure*, not
just wording discipline:

- `operation`: enablement (all guards, `requires`/`let` in declaration order),
  effects (simultaneous commit, pre-state reads — a struct-literal assignment
  additionally expands into its field-level updates, sorted the same way
  `expr_text` sorts struct-literal fields, so the canonical literal and its
  expansion never disagree), postconditions (rollback on failure), and a
  fairness clause that always calls weak fairness "a scheduling assumption"
  and never "immediately" or "every time".
- `state_rule`/`transition_rule`/`deadline_rule`: distinct fixed lead-ins
  ("after initialization and after each commit" vs. "pre/post-transition
  state" vs. "a deadline rule, distinct from a progress rule's liveness").
- `progress_rule`: always states scope/trigger/consequence/premise/rank-hint
  as a declaration ("the FSL declares this as a demand") and never as
  established evidence — RCIR v1 carries no verification evidence at all
  (that is issue #332); a future evidence overlay only needs to replace the
  fixed "verification status" line, not this template's structure.
- `reachability_goal`: a goal ("at least one execution must exist"), never
  phrased as an invariant that must hold everywhere.
- `acceptance_trace`/`forbidden_trace`: Given/When/Then over the concrete
  step sequence from `requirements_trace_contract`, always closing with a
  fixed disclaimer that the example does not generalize to every input,
  ordering, or state (this is the exact wording issue #326's acceptance
  criterion 3 tests for).
- `terminal_rule`: "no further operations are intended," explicitly not a
  reachability demand.

`undecided` items are excluded from every claim template and rendered only in
their own section with a fixed "not a verification condition" disclaimer.
`analysis_scope` (`instances`/`values`) renders in its own section with a
fixed "these are analysis bounds, not operational capacity" disclaimer;
numeric bounds are read from RCIR's normalized-AST `["num", N]` shape and
shown as the plain number (any other shape — e.g. a `const` reference — falls
back to a compact JSON dump rather than fabricating a value).

A claim referenced by more than one requirement (RCIR's many-to-many
relation) renders in full only the first time it is encountered while
walking `requirements` in order; every later reference is a short "see the
`<first-requirement>` section" back-reference, so the same normative text is
never duplicated with a risk of silently drifting between copies.

## Determinism

Requirements render in RCIR's own array order; a requirement's claims render
in its own `claim_ids` order; guards/statements/postconditions stay in
declaration order. There is no `HashMap`/iteration-order-dependent state
anywhere in the renderer. Two runs over the same `RequirementClaimSet` and
`KernelModel` produce byte-identical Markdown (tested directly).

## Verification evidence

`rust/fsl-tools/tests/document_render.rs` (24 tests): an exact byte-for-byte
golden match of `examples/pm/cancel_system.fsl`'s REQ-2 in both locales
(issue #326's acceptance criterion 1 — two guards, a struct-literal update,
and fairness, all present); requirement text and formalized meaning kept in
separate sections; the acceptance/forbidden non-generalization disclaimer
(with a negative check that no absolute-guarantee phrasing appears);
analysis-scope's not-a-capacity disclaimer and correct plain-number
rendering; byte-identical repeated renders; `requires`-as-enablement and
`not`-is-preserved meaning-fidelity checks; the transition-rule N-AND
special case together with `old(...)`; the deadline/progress/reachability
distinctions; undecided exclusion from claims; a forbidden case with an
empty step prefix; and the shared-claim back-reference behavior.
