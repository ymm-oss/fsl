<!-- SPDX-License-Identifier: Apache-2.0 -->

# `fslc document`'s dialect boundary (v1: explicit rejection, not adapters)

Status: accepted partial slice of issue #334. The issue remains open for the
cross-layer views, adapters, and per-dialect coverage rows this slice does not build.

## Why v1 is a boundary, not a feature

Issue #334 asks for two things: a `business`/`design` cross-layer view
("Phase 4"), and specialized adapters for `domain`/`dbsystem`/`ai_component`
("Phase 5"). Both are already, independently, out of reach of a small,
honest v1:

- **"design" is not a dialect.** The complete dialect-keyword registry
  (`fsl_syntax::DIALECT_KEYWORDS`, a `frontends!` macro invocation in
  `rust/fsl-syntax/src/dispatch.rs`) is exactly `spec`, `refinement`,
  `compose`, `business`, `governance`, `requirements`, `domain`, `dbsystem`,
  `ai_component`, `agent` — ten keywords, no `design`. `docs/LANGUAGE.md`'s
  own three-layer framing ("business ⊒ requirements ⊒ design ⊒
  implementation") names *design* as a position in the refinement chain, not
  a grammar: `examples/agentic_rag/agentic_rag_design.fsl`, a genuine design-
  layer file, declares `spec AgenticRagDesign { ... }`. So `--view
  business`/`--view design` cannot mean "parse a business/design-dialect
  file" — it can only mean a cross-layer *presentation* over already-accepted
  `spec`/`requirements` documents plus a *second file's* `refinement`/
  `implements` metadata. Today's `fslc document generate <file>` CLI
  contract takes exactly one spec path; expressing "render this file's
  claims annotated with what an separate refinement mapping says about it"
  is a multi-input CLI-contract design of its own, unspecified by the issue
  and not decidable as a byproduct of this one.
- **Liveness re-verification tracking would require a live verify pass.**
  The only truthful source for "was the higher layer's progress property
  re-checked at this layer" is `fsl_verifier::refinement::
  check_refinement_progress`'s result — which requires *running*
  `fslc refine`, not reading anything already sitting in the checked Kernel.
  `fslc document generate`'s determinism contract (issue #326: the same
  spec always renders byte-identical Markdown) already collided with this
  exact problem once, for evidence overlays — issue #332's resolution was
  "v1 is evidence-file-only: `generate` never runs a live verify pass
  itself." The same resolution applies here in spirit (a saved-refine-result
  input, mirroring `--evidence`), but is a new design of its own, not a
  restatement of #332's.
- **A specialized dialect adapter needs coverage-registry machinery that
  does not exist.** `RCIR_TARGET_KIND_REGISTRY` (issue #328,
  `rust/fsl-tools/src/document_coverage.rs`) is keyed **only** by
  target-kind string (`action`, `property:invariant`, …) — there is no
  dialect axis anywhere in it. A `domain` saga/effect, a `dbsystem`
  migration step, or an `ai_component` evidence-boundary construct would each
  need their own semantic-coverage classification added to
  `document_project.rs` and a new registry row (or a dialect-scoped table
  alongside the existing one) — exactly the work issue #334's own acceptance
  criterion 3 says must exist **before** a dialect activates. Shipping one
  adapter without that machinery, or shipping the machinery without a real
  adapter behind it, both produce the thing the issue's invariant forbids:
  a plausible-looking document whose source-only declarations are silently
  unaccounted for.
- **`compose` needs nothing new.** It is already rejected at two independent
  layers: the Public Kernel export contract (`docs/DESIGN-kernel-contract.md`:
  "Compose lowering currently loses the component filename... Public Kernel
  v1 therefore rejects `compose` input explicitly... A future schema version
  may add multi-source provenance once it is retained by the checked
  Kernel.") and, separately, the RCIR document-projection layer (the same
  generic unsupported-dialect path this issue hardens, below). The issue's
  "confirm truthful provenance before enabling" instruction describes the
  *already-correct* current posture — reject — not a gap.

Between the issue's own two sanctioned terminal states for an
under-supported dialect — "(a) explicitly reject as unsupported" or "(b)
generate only the Kernel-projected portion, enumerating every source-only
declaration in `unsupported_targets`" — only (a) is dischargeable with v1
machinery. Option (b)'s enumeration obligation **is** the per-dialect
coverage-row work acceptance criterion 3 gates activation on. So v1 takes
option (a), for every remaining dialect, and does it properly: a *typed,
coded, fully tested* rejection, replacing what was previously an accidental
side effect of a generic catch-all.

## What v1 actually changes

**Before this issue**, an unsupported dialect (`business`, `governance`,
`domain`, `dbsystem`, `ai_component`, `compose`, `refinement`, `agent`) was
already rejected by `rust/fsl-tools/src/document_project.rs`'s RCIR
projector — this is pre-existing, not new — but the rejection surfaced as a
plain `String`, turned by the CLI into a generic `kind: "semantics"` error,
indistinguishable from an actual parse/type defect in the input. Only
`business` had a dedicated negative-control test.

**After this issue**, the projector returns a typed
`DocumentProjectionError`:

```rust
pub const RCIR_SUPPORTED_DIALECTS: &[&str] = &["requirements", "spec"];

pub enum DocumentProjectionError {
    UnsupportedDialect { dialect: &'static str },
    Other(String),
}
```

`project_requirement_claims_from_source`'s signature moves from
`Result<RequirementClaimSet, String>` to
`Result<RequirementClaimSet, DocumentProjectionError>`. `Display` on
`UnsupportedDialect` is byte-identical to the pre-existing message
(`"document projection does not support dialect '{dialect}' in RCIR v1"`) —
that sentence is a documented contract
(`docs/DESIGN-document-requirement-claim-ir.md`) and does not move.

`fslc`'s single interception point — `load_document_claims_with_label`
(`rust/fslc/src/main.rs`), shared by `document generate`, `document claims`,
`document check`, and `approval create --kind requirements_document`/
`document generate --approval`'s re-render path — maps
`DocumentProjectionError::UnsupportedDialect` to a new coded envelope instead
of the generic semantic-error path:

```json
{
  "result": "error",
  "kind": "document",
  "code": "FSL-DOC-DIALECT-UNSUPPORTED",
  "message": "document projection does not support dialect 'domain' in RCIR v1",
  "dialect": "domain",
  "supported_dialects": ["requirements", "spec"]
}
```

`dialect`/`supported_dialects` are machine-readable fields — a caller
distinguishes "RCIR has no adapter for this dialect yet" (retry never, this
input is out of scope) from a genuine parse/semantic error (fix the input)
without parsing the message string. `DocumentProjectionError::Other` keeps
today's exact `semantic_error_output` behavior, including its existing
heuristics — zero behavior change for any already-supported input.

`--view business`/`--view design` remain `kind: "usage"` errors (unchanged
exit-code/kind contract, `generate_rejects_an_unknown_view` already pins
this); only the message text changed, to name the reservation instead of
implying `requirements` is the only concept that will ever exist:
`"--view must be requirements ('business'/'design' are reserved until
docs/DESIGN-document-dialect-adapters.md's activation contract is met, issue
#334)"`.

## The dialect-activation contract (binding on future work)

This is the part of the issue v1 does not implement but must not leave
undefined, since a future PR will read this section to know what "done"
means for one adapter:

1. **Cross-layer relations come from explicit metadata only — never name
   similarity** (acceptance criterion 2). The reuse point is the *typed*
   correspondence model, not the bare strings RCIR already discards into
   `unsupported_targets` today (`document_project.rs`'s
   `implements_names: Vec<String>`, which becomes a
   `refinement:{name}`-targeted `UnsupportedEntry` and nothing richer).
   `fsl_core::refinement::{Refinement, ImplementsContract}`
   (`state_maps`, `action_correspondences`, `progress: Vec<ProgressMap>`)
   is the richer model, already consumed by two independent existing
   features — `fslc refine`/`refine chain`, and `fslc analyze --projection
   refinement_graph` (`rust/fsl-tools/src/refinement_analysis.rs`). A
   cross-layer document view must project through this same typed model or
   its existing graph projection, never re-derive correspondence from
   identifier text.
2. **Liveness re-verification is a separate, explicit fact, never implied.**
   `fslc refine` already models the three-way distinction this needs: no
   `preserve progress` declared → no `progress` key at all in its result; a
   declared `preserve progress` that holds → a `progress` object with
   `checked_to_depth`; a declared `preserve progress` that fails → a
   distinct `progress_lost`/`faithfulness_class: "liveness_not_refined"`
   shape. A document-layer feature must source this same three-way fact
   (via a saved refine-evidence input, not a live verify pass inside
   `generate` — see above) and render it as its own, clearly labeled line,
   never folded into or implied by a rendered progress-rule claim.
3. **No dialect activates without both its adapter and its coverage-gate
   row.** `rust/fsl-tools/tests/document_coverage.rs`'s
   `rcir_supported_dialects_are_exactly_spec_and_requirements` is a scope-change
   tripwire: it asserts `RCIR_SUPPORTED_DIALECTS == {"requirements", "spec"}`
   and that the remaining `fsl_syntax::DIALECT_KEYWORDS` are exactly the eight
   rejected dialects. This is necessary but not sufficient for acceptance
   criterion 3; the future activation change must add a real adapter and a
   dialect-scoped coverage row/test in the same change.
   The same test also fails if the language ever grows an eleventh dialect
   keyword, forcing an explicit RCIR posture decision for the newcomer
   rather than a silent default.
4. **`compose` stays rejected until the checked Kernel retains truthful
   multi-source provenance** (tracked at the Kernel-contract level,
   `docs/DESIGN-kernel-contract.md`), and under `--strict`, partial/
   untruthful provenance is a hard rejection, never a warning, when that
   work eventually happens.

## What v1 does not do

- No `--view business`/`--view design` (remain reserved usage errors).
- No `domain`/`dbsystem`/`ai_component` document adapter.
- No per-dialect coverage-registry axis or rows.
- No cross-layer traceability rendering.
- No liveness-re-verification display inside `fslc document`.
- No change to `compose`'s already-correct rejected status.
- No Python (`src/fslc`) change — frozen reference, out of scope per
  `AGENTS.md`.

Each of the above is a candidate for its own future issue, scoped and
gated exactly as this section specifies.

## Acceptance criteria — how this v1 maps to them

1. **"Feeding an unsupported dialect produces an explicit unsupported report,
   not an apparent document." Satisfied, and strengthened.** Rejection
   happens before RCIR projection ever starts (right after `parse_document`,
   before `parse_kernel_source`), so no partial artifact is constructible.
   The report is now typed and coded (`FSL-DOC-DIALECT-UNSUPPORTED` +
   `dialect` + `supported_dialects`), not a generic semantics error.
   Negative controls now cover all eight rejected dialects (previously only
   `business`), at both the library level
   (`rust/fsl-tools/tests/document.rs`) and the CLI level
   (`rust/fslc/tests/document_cli.rs`).
2. **"business/design view shows cross-layer relations from explicit
   metadata only." Not exercised in v1 — no cross-layer view ships — and
   that is the honest statement, not a claimed pass.** It is preserved as a
   *binding* constraint on whichever future change adds the view: see "The
   dialect-activation contract" §1, which names the exact reuse surface and
   forbids name-similarity inference.
3. **"Per-dialect: the coverage gate must pass before `--view`/that dialect
   activates." Partially guarded, not satisfied.**
   `rcir_supported_dialects_are_exactly_spec_and_requirements` detects a change
   to the supported set, but no dialect coverage axis or adapter row exists yet.
   Issue #334 remains open until those executable gates accompany each adapter.

## Verification evidence

`rust/fsl-tools/tests/document.rs`'s `rejects_every_unsupported_dialect_fail_closed`
(table-driven over all eight rejected dialects, asserting both the typed
`DocumentProjectionError::UnsupportedDialect` value and its exact `Display`
text); `rust/fsl-tools/tests/document_coverage.rs`'s
`rcir_supported_dialects_are_exactly_spec_and_requirements` (the activation
tripwire); `rust/fslc/tests/document_cli.rs`'s
`generate_rejects_an_unsupported_dialect_with_a_coded_error` and
`claims_rejects_an_unsupported_dialect_with_a_coded_error` (the coded
envelope end-to-end, confirming no `content`/`claims` payload accompanies the
error on either subcommand).
