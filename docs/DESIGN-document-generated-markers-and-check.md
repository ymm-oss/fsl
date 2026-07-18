<!-- SPDX-License-Identifier: Apache-2.0 -->

# Generated block markers and `fslc document check`

Status: accepted. Implements issue #329.

This design adds structural markers and frontmatter to the artifact `fslc document
generate` (issue #327) already emits, and a purely structural drift checker,
`fslc document check`. It adds no new semantics to RCIR (#325) or the renderer
templates (#326): those still produce exactly the same normative text; this issue
only wraps that text in a fixed, machine-checkable envelope. It interprets no
natural language, and it adds no reverse-sync from Markdown back to FSL — that is
explicitly out of scope in v1, per the issue: a generated document is a build
artifact, and a correction always goes back to the FSL source. The glossary
sidecar (#330), evidence overlay (#332), and approval-target integration
(#333) each later extended this same frontmatter/drift-reason mechanism
(a new digest key plus a `*_changed` reason) rather than replacing it — see
`docs/DESIGN-document-glossary.md`, `docs/DESIGN-document-evidence-overlay.md`,
and `docs/DESIGN-approval.md`.

## Why a claim block needs its own digest

RCIR's `Claim::claim_digest` (issue #325) is `framed_digest(CLAIM_DIGEST_ALGORITHM,
core)` over the claim's normalized checked-kernel-AST projection — pure semantics,
computed once at projection time, before any rendering happens. It is exactly right
for detecting "did the FSL construct this claim represents change?", and it is
stable across a comment/whitespace-only FSL edit by construction (the AST projection
never sees comments or whitespace). It has no way to detect "did someone edit the
*rendered sentence* for this claim?", because it never looks at rendered text at all.
A one-character edit to the Japanese or English prose inside a claim block is
therefore invisible to `claim_digest`, which fails acceptance criterion 1 outright.

This issue adds a second digest, `CLAIM_BLOCK_DIGEST_ALGORITHM =
"fsl-doc-claim-block-v1+sha256"`, computed by a new `framed_text_digest(algorithm,
text)` (`document_digest.rs`) — the same `algorithm ++ 0x00 ++ payload` framing
`framed_digest` already uses, but over raw UTF-8 text bytes rather than
canonicalized JSON, since the input here is prose, not an AST value. It hashes
exactly the claim's rendered body — the string a per-claim-kind template function
returns, before markers are attached — so it changes on any prose edit, is
reproducible by re-rendering the same claim under the same locale, and is unaffected
by a spec change that touches neither the claim's semantics nor its rendered text.
It does not fold the claim id into the hash: `fslc document check` always
cross-validates a block's `id=` attribute against a fresh render's own claim-id
sequence as a separate structural check (see below), so a second identity binding
inside the digest would be redundant.

`claim.claim_digest` and the new block digest are not redundant with each other and
neither subsumes the other: the block digest cannot tell you the FSL changed if the
rendered text happens to read the same either way (rare, but the spec/claim-set
digests in frontmatter cover that); `claim.claim_digest` cannot tell you the prose
was edited. `fslc document check` uses the block digest for per-claim comparison and
the frontmatter's `spec_digest`/`claim_set_digest` (RCIR's own digests, read from
`RequirementClaimSet::spec`) for whole-spec comparison — both are checked, at
different granularities.

## Frontmatter

`render_requirements_document` now emits a fixed-key `---`-delimited block before
the title, built by `document_markers::render_frontmatter` from data the function
already had — no new parameters:

```yaml
---
fsl_document_schema: fsl-requirements-document-v1
view: requirements
lang: ja
source: examples/pm/cancel_system.fsl
renderer: fslc-document-renderer
renderer_version: 1.2.0
normative_scope: generated-claim-blocks-only
spec_digest: sha256:...
claim_set_digest: sha256:...
---
```

`fsl_document_schema` versions the *artifact format* (this frontmatter key set plus
the marker grammar) — a schema deliberately distinct from `RCIR_SCHEMA_VERSION`,
which versions the claims JSON, not the Markdown envelope around it; a future
grammar change (e.g. #330's glossary reference) bumps this schema, not RCIR's.
`view`/`lang` are today's only-legal `--view requirements`/`--lang ja|en` values,
recorded so `fslc document check` can re-render without being told a locale on its
own command line — the artifact is self-describing. `source` is `claims.spec.source`
(omitted when `None`); `spec_digest`/`claim_set_digest` are `claims.spec`'s own
fields. `renderer`/`renderer_version` are new constants (`document_markers.rs`):
`renderer_version` is hand-bumped whenever a template string or section skeleton
changes, and is deliberately *not* `env!("CARGO_PKG_VERSION")` — keying it to the
crate version would mark every previously generated document `renderer_changed` on
every unrelated `fslc` release, not only when a template actually changed. The
failure mode of a forgotten bump is fail-closed either way: block text still
diverges from a fresh render, so `check` still reports drift, just as
`claim_changed` rather than the more informative `renderer_changed`.

No YAML dependency was added: the key set is fixed and flat, hand-emitted as
`key: value` lines and hand-parsed by splitting on the first `": "` per line. An
unknown key, or a missing required key, is a hard parse failure — frontmatter is not
extensible ad hoc. Key *order* is not checked: every key is compared individually
against a fresh render's own frontmatter, so reordering the raw lines is invisible
to any comparison and enforcing an order would be pure ceremony.

## The `background` slot

Every section `render_requirements_document` already produced is derived from
checked FSL — none of them is free text. Reframing an existing generated section as
"editable" would let a human silently rewrite text a reader believes is
machine-derived, which is exactly backwards from the issue's own rationale for
separating normative and editable content. Instead, v1 introduces exactly one new
section: `background`, positioned immediately after the title (before the existing
"position of this document" boilerplate), wrapped in `<!-- fsl:slot begin
name="background" normative="false" --> ... <!-- fsl:slot end -->` and seeded with a
locale-appropriate heading plus a one-sentence placeholder explaining that the
section is free-form and non-normative. `SLOT_NAMES: &[&str] = &["background"]` is
the complete, closed set v1 recognizes — a slot name outside it is drift, not a
feature to auto-support.

`fslc document check` treats a slot's *markers* with the same structural rigor as a
claim block's markers (must be well-formed, present exactly once, name in the closed
set) — because the markers are the boundary of the only region edits are legal in,
and a weak check on that boundary would let edits leak past it. A slot's *contents*
are fully opaque: never digested, never compared, with one exception — a line
starting with `<!-- fsl:` anywhere inside a slot (other than the closing marker) is
rejected as `marker_malformed`. Markers are invisible HTML comments in rendered
Markdown, so without this check, a copy of a claim block pasted inside the
"editable" slot would look exactly like a normative block to anyone reading the
raw source, while `check` would ignore it entirely (it lives inside opaque slot
content). This closes that spoofing gap for a small, fixed rule.

## Where claim markers get attached

`document_render.rs`'s `render_claim` is the single dispatch point every one of the
nine per-claim-kind template functions returns through, and it already owns the
first-encounter/back-reference decision (`ctx.rendered_claims`). Marker wrapping
attaches at exactly this point — the nine template functions are unchanged:

```rust
fn render_claim(claim: &Claim, ctx: &mut Ctx<'_>) -> String {
    if ctx.rendered_claims.contains(&claim.id) {
        return back_reference(claim, ctx.locale);       // unchanged: no markers
    }
    ctx.rendered_claims.insert(claim.id.clone());
    let body = match claim.kind { /* existing 9-arm dispatch, unchanged */ };
    document_markers::wrap_claim_block(&claim.id, &body)  // new
}
```

A back-reference (the short "see the `<first-requirement>` section" pointer for a
claim shared by more than one requirement) carries no marker. Three reasons: every
canonical claim id must appear in exactly one marker pair (`fslc document check`'s
own invariant would break if a back-reference reused the same `id=`); the
back-reference text is itself fixed generated skeleton, already protected end-to-end
by the residue comparison below; and a distinct `fsl:claim-ref` marker would grow
the grammar for no additional detection power in v1.

## `fslc document check <spec.fsl> <document.md>`

Two positional arguments, no flags — `check` reads `lang`/`view`/`source` back from
the artifact's own frontmatter rather than accepting them on its command line, so a
verdict never depends on the invoker happening to retype the right flags. Parsing
(`document_markers.rs`) is a hand-rolled line scanner, not a `regex`/YAML dependency:
the marker grammar is a fixed set of whole-line patterns over an alphabet (kernel
identifiers, hex digests) that can never contain `"` or a newline, so a full parser
is unneeded. `\r\n` is normalized to `\n` before scanning, so a line-ending change
from an editor is not itself drift.

**Source-label parity.** Every rendered `出典`/`Source:` line embeds the exact
label string `fslc document generate` was invoked with — `SpecInfo.source` and every
`Claim`/`Requirement` source reference derive from the same `source_path` argument
`project_requirement_claims_from_source` received. If `check` re-projected under its
own command-line spelling of the spec path, a cosmetic difference (a relative path
from a different working directory, for instance) would shift every source-ref line
and manufacture false whole-document drift. `check` therefore re-projects under the
label the artifact's frontmatter recorded (`load_document_claims_with_label`), not
under its own argv spelling; the spec *content* still comes from the path check's
own caller passed in.

**Comparison algorithm.** After the frontmatter's `fsl_document_schema`/`view`/`lang`
are confirmed supported (any of the three failing is a hard error, exit 2 — the
artifact is not a document this build can process at all, not a drifted one):

1. `renderer`/`renderer_version` mismatch -> `renderer_changed` reason
   (`FSL-DOC-RENDERER-CHANGED`); subsequent per-claim *text* comparison and the
   residue comparison are skipped for the rest of this run (comparing prose across
   renderer versions would bury the one true cause under a flood of noise; id-level
   structural checks are renderer-independent and still run).
2. Frontmatter `spec_digest`/`claim_set_digest` vs. the fresh re-projection's own
   `claims.spec.spec_digest`/`claim_set_digest` -> `spec_digest_mismatch` /
   `claim_set_digest_mismatch` (`FSL-DOC-SPEC-DRIFT`).
3. Claim-id structure: any artifact id appearing more than once ->
   `claim_duplicate` (`FSL-DOC-BLOCK-DUPLICATE`); a fresh id absent from the
   artifact -> `claim_missing` (`FSL-DOC-BLOCK-MISSING`); an artifact id absent from
   the fresh set -> `claim_unknown` (`FSL-DOC-BLOCK-UNKNOWN`); when (and only when)
   the id *sets* match exactly with no duplicates, a different relative order ->
   `claim_reordered` (`FSL-DOC-BLOCK-REORDERED`).
4. Slot structure: the same duplicate/missing/unknown checks over `SLOT_NAMES`,
   reported as `slot_duplicate`/`slot_missing`/`slot_unknown`
   (`FSL-DOC-MARKER-MALFORMED` — folded into the same code as a malformed marker
   line, since both are "the immutable/editable boundary is not well-formed";
   distinguishing a deleted slot from a corrupted marker did not seem worth a
   fifth marker-integrity code).
5. Per matched claim (present in both sides, renderer not changed): recompute
   `framed_text_digest` of the artifact's own block body; if it disagrees with the
   block's own `digest=` attribute, the artifact is internally inconsistent
   (`claim_changed`, `detail: "artifact_edited"` — someone edited the body without
   updating the digest, or edited the digest itself); otherwise, if the body
   disagrees with the fresh render's body, the FSL or renderer moved and the
   document needs regenerating (`claim_changed`, `detail: "render_diverged"`). Both
   report `FSL-DOC-BLOCK-DRIFT`, distinguished only by `detail`.
6. Residue (everything outside claim/slot bodies): compared position-by-position
   only when the artifact's and the fresh render's segment sequences are the same
   length with the same claim id / slot name at every marker position — i.e. only
   once every check above has already come back clean. Comparing residue against a
   structurally *misaligned* sequence would just add noise on top of a mismatch
   already reported; a byte difference between two structurally aligned residues is
   the only remaining thing it could be -> `edit_outside_slot`
   (`FSL-DOC-EDIT-OUTSIDE-SLOT`).
7. A body-grammar violation on the artifact side (an unclosed claim/slot, a
   malformed marker line, a marker-like line inside a slot) is not a hard error —
   the artifact is still a recognizable generated document, just internally
   inconsistent — so it becomes a single `marker_malformed` reason
   (`FSL-DOC-MARKER-MALFORMED`) instead.

A conformant result never has document text but no reasons; a drifted result's
`reasons` is never empty. Both carry the *fresh* `spec_digest`/`claim_set_digest` and
the artifact path, matching the issue's own JSON shapes with `fsl: "1.0"` added
(the same envelope convention `generate`/`claims` already use).

## Exit codes

`document_conformant` -> 0; `document_drifted` -> 1 (a `reasons`-bearing result,
*not* routed through `normalized_exit_status`'s `kind == "internal"` -> 3 special
case, since its `result` is neither `"error"` nor internal); a frontmatter/schema
failure, an I/O error, or a spec parse/type/semantics error -> 2, using the same
`error_output`/`semantic_error_output` helpers `generate`/`claims` already call; an
unexpected internal failure -> 3 via the existing `normalized_exit_status` path. No
new exit-code plumbing was needed.

## What v1 does not do

- **Line-shift sensitivity.** A comment/whitespace-only FSL edit that does not shift
  any claim's source line leaves both the semantic digests and the rendered text
  unchanged, and `check` reports it conformant, exactly matching the issue's
  required test. A comment inserted *above* a claim's declaration, however, shifts
  that claim's `出典`/`Source:` line number, which is part of the rendered block —
  `spec_digest`/`claim_set_digest` still match (locations are stripped before
  digesting), but the affected claim(s) report `claim_changed`/`render_diverged`.
  This is a deliberate position, not an oversight: a provenance pointer is generated
  content, and when it goes stale the document needs regenerating; the alternative
  (excluding source-ref lines from the digest and comparison) would let anyone edit
  a citation undetected and would need a line-classification rule inside the digest
  formula. A human reviewing a `render_diverged` reason sees immediately that the
  fix is "regenerate," not "something is semantically wrong."
- **Frontmatter key order** is not checked (see above).
- **`--strict`/`--strict-rendering` provenance** is irrelevant to `check` and is not
  recorded in frontmatter: those flags gate `generate`'s coverage/rendering
  *policy*, and change zero output bytes either way. A document legitimately
  generated without `--strict` checks exactly the same as one generated with it.
- **Reverse-sync** from an edited document back into the FSL source is out of scope
  in v1, per the issue.

## Verification evidence

`rust/fsl-tools/tests/document_markers.rs`: the frontmatter block's key set/values;
every claim renders inside exactly one marker pair with a unique id matching RCIR's
own claim id set; a claim shared by two requirements produces exactly one marker
pair (the back-reference carries none); a marker's digest equals
`framed_text_digest(CLAIM_BLOCK_DIGEST_ALGORITHM, body)`; the `background` slot is
present exactly once under its fixed name; the renderer's own output always parses
under its own grammar (both locales); missing frontmatter, an unknown frontmatter
key, and a marker-like line inside a slot are all rejected.

`rust/fsl-tools/tests/document_render.rs`: the existing `req2_ja`/`req2_en` golden
tests were updated to include the new marker lines (the underlying rendered content
is byte-identical to before this issue — only the wrapper changed) — the full
expected literal was regenerated rather than stripping markers before comparison,
since the goldens' entire purpose is pinning the artifact's exact bytes and markers
are now part of that contract.

`rust/fslc/tests/document_check_cli.rs`: a freshly generated document is conformant
in both locales; editing the `background` slot passes (acceptance criterion 2); a
one-character change inside a claim block fails with `claim_changed`/
`artifact_edited` (acceptance criterion 1); deleting, duplicating, and reordering a
claim block; an old `renderer_version`; editing generated prose outside any slot; a
marker-like line forged inside a slot; a requirement-text change drifting
`claim_set_digest`; missing/unsupported frontmatter and a missing artifact file both
exiting 2; a missing spec argument as a usage error. `rust/fslc/tests/document_cli.rs`
(issue #327) was updated where it asserted the raw markdown started with the title
heading — it now starts with the frontmatter block instead, by design.
