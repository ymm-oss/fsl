<!-- SPDX-License-Identifier: Apache-2.0 -->

# Glossary sidecar for `fslc document`

Status: accepted. Implements issue #330.

This design adds a presentation-only glossary sidecar to `fslc document generate`
(#327) and glossary-parity awareness to `fslc document check` (#329). It adds no new
FSL grammar: every glossary target is validated against the already-checked
`KernelModel`. A label can only change how an `action:`/`state:`/`enum:` identifier
*displays*; nothing in this design touches modality, negation, or conditional
structure, because the renderer's safe-pattern recognizer (#326) and its per-claim
templates are never given the glossary at all — only two narrow, additive display
points ever consult it (below).

## Why a label needs its own digest, and why it must not touch the semantic one

The glossary must change `artifact_digest` (the rendered bytes) but never
`claim_set_digest` (RCIR's own semantic digest, issue #325). This holds with zero
digest-computation changes, by construction: the glossary is threaded only into
`render_requirements_document` (issue #326), strictly downstream of
`project_requirement_claims`, which computes `claim_set_digest` without ever seeing
a glossary. `artifact_digest` is `approval::sha256_bytes(rendered.markdown)` —
whatever bytes come out, that digest reflects them. Recording a `glossary_digest`
key in frontmatter (below) is what makes this true even for a glossary edit whose
labels apply nowhere in the document: without that key, an inert glossary change
would leave `rendered.markdown` byte-identical and the required "artifact digest
changes" test would depend on the glossary actually mattering to some rendered
line, which is not the guarantee the issue asks for.

## Glossary file: schema, parsing, and duplicate-key detection

`rust/fsl-tools/src/document_glossary.rs` (a new file, matching the established
one-file-per-concern convention alongside `document_markers.rs`/
`document_check.rs`/`document_coverage.rs`) owns `Glossary { locale, labels:
BTreeMap<String, String> }`, `GlossaryIssue` (`Json`/`UnsupportedSchema`/
`UnsupportedLocale`/`DuplicateTarget`/`EmptyLabel`), and `parse_glossary(text: &str)
-> Result<Glossary, Vec<GlossaryIssue>>`.

The issue requires a duplicate label target to be its own diagnosed error
(`FSL-DOC-LABEL-CONFLICT`), but ordinary `serde_json` deserialization into any map
type (`Value`, `HashMap`, `BTreeMap`) silently keeps only the *last* occurrence of a
repeated JSON object key — by the time a `Value` exists, "this file had two
`"action:submit_cancel"` keys" is unrecoverable. `serde`'s `MapAccess`, however,
visits every key/value pair *as it appears in the source text, duplicates
included*, before any map is built — the collapsing happens in the map's own
`insert`, not in the deserializer. So `labels` deserializes through a
hand-written `Visitor` that collects `Vec<(String, String)>` (order preserved,
nothing discarded) rather than a map directly; `parse_glossary` then inserts into a
`BTreeMap` itself, and a `BTreeMap::insert` returning `Some` for a key that was
already present is exactly the duplicate. This was verified with a standalone
`Vec<(String, String)>` proof before relying on it: parsing
`{"a":"x","a":"y","b":"z"}` through this Visitor yields all three entries,
including both `"a"`s.

`FSL-DOC-LABEL-CONFLICT` is always an error (default and `--strict` alike), per the
issue's own table — it is not a policy choice `--strict` gates, so it is returned
unconditionally from `parse_glossary`, alongside a few v1-only additions:
`schema` must equal `"fslc.document-glossary.v1"`, `locale` must be `ja`/`en`, an
empty label string is rejected (it would render as bare `（`id`）` noise), and an
unknown top-level key is rejected (the frontmatter parser already treats an
unrecognized key this way; a glossary sidecar keeps the same closed-key
discipline).

## Target validation (`FSL-DOC-LABEL-UNKNOWN`)

`unknown_targets(glossary, model) -> Vec<UnknownTarget>` validates each label
target against the real `KernelModel` fields, not against RCIR's own claim
vocabulary (an `action:` label is valid even for an action that appears only
inside an acceptance trace, with no claim of its own):

| Target shape | Validated against |
|---|---|
| `action:NAME` | `model.actions.iter().any(\|a\| a.name == NAME)` |
| `state:NAME` | `model.state.iter().any(\|(n, _)\| n == NAME)` |
| `enum:TYPE.MEMBER` | `model.enum_members.get(MEMBER)` is `FslValue::Enum { type_name, .. }` with `type_name == TYPE` |

`action:`/`state:` reuse `fsl_core::action_target`/`state_target` — the same
target-string helpers the origin registry already uses internally — rather than
inventing a second naming scheme. `enum:Type.Member` has no equivalent existing
helper (`type_target` names only the type, not a member), so it is hand-constructed
and split on the *last* `.` (a type name can itself contain one, via
`display_name`'s `__` → `.` convention; a member name cannot). A target outside
these three namespaces, or missing the expected shape, is also
`FSL-DOC-LABEL-UNKNOWN` — the issue defines exactly two label-safety codes, and
"this isn't a namespace v1 recognizes" is semantically the same failure as "this
target doesn't exist," not a third code. v1 recognizes exactly these three
namespaces (matching the issue's own examples); a `property:` namespace for
property-claim headings is a plausible, non-breaking future addition, not added
speculatively here.

Per the issue's table, an unknown target is a warning by default and an error under
`--strict`, checked in `run_document_generate` after the existing #327 diagnostics
(`FSL-DOC-NO-REQUIREMENTS` always; `FSL-DOC-UNTAGGED-TARGET`/
`FSL-DOC-UNSUPPORTED-TARGET` under `--strict`) and after the model is built (target
validation needs the model). A glossary file that fails to load or parse at all is
checked earlier, before the model — those are shape/identity problems, not
target-existence problems, and should not require a successful kernel build to
report. Note that in practice `--strict` on a typical spec already fails on
`FSL-DOC-UNSUPPORTED-TARGET` before glossary validation ever runs, since `init` is
unconditionally unsupported (issue #325) whenever any `init { ... }` statement
exists — this is pre-existing #327 behavior, unrelated to and unchanged by this
issue, and is why this issue's own CLI tests use a fixture with an empty `init {}`
block to observe `FSL-DOC-LABEL-UNKNOWN` in isolation.

## Rendering scope: two narrow, additive display points — not expression substitution

The renderer has two categories of place a raw identifier appears: a single,
well-defined display name per claim (`metadata_header`'s `display` argument — an
action name for an operation claim, a property name or case id for every other
kind), and identifiers *embedded inside* arbitrary rendered expression/lvalue text
(`fsl_core::source_expr_text`, the safe-pattern recognizer in
`document_render_expr.rs`, `source_binder_text`) — the exact same pretty-printer
`explain --readable` depends on, producing opaque strings like `` `scr[c].st ==
CancelForm` `` with no separately addressable identifier tokens.

v1 deliberately does not touch the second category. Rewriting the shared
pretty-printer to consult a glossary at every identifier leaf would risk `explain
--readable`'s own output and requires understanding its full precedence/
parenthesization logic; a blind textual substitution pass over already-rendered
text risks matching a label's target name as a substring inside an unrelated,
longer identifier. Given that risk, v1 instead does two things, both additive and
independently safe:

1. **The claim's own heading** (`render_operation`'s two `metadata_header` calls
   only — every other claim kind's heading argument is a property name or case id,
   which have no glossary namespace at all in v1). `metadata_header` gained one new
   parameter, `glossary_label: Option<&str>`; when present, the heading becomes
   `#### {kind}: {label}（`{id}`）` (ja) / `#### {kind}: {label} (`{id}`)` (en) —
   label first, canonical identifier always retained in a parenthesized code span,
   never replaced outright. The identifier is the join key back to the FSL source,
   `explain`, and every trace; hiding it would work against the document's own
   auditability story. This exact string is a deliberate, if somewhat arbitrary,
   choice a human should feel free to revise — it is pinned by exactly one golden
   assertion, not treated as load-bearing prose.
2. **A new "Glossary" reference section**, rendered only when a glossary was
   applied, listing every accepted `target: label` pair sorted by target (`Glossary
   ::labels` is already a `BTreeMap`, so iteration order is free). This is the
   *only* place a `state:`/`enum:` label is ever shown — those targets almost
   always occur only inside expression text (a state variable's own declaration is
   not itself rendered anywhere as a heading), so without this section, accepting
   and validating a `state:`/`enum:` label would have zero visible effect,
   contradicting the issue's own worked example. The section is placed after
   "Analysis scope" and before "Generation info" — an appendix-style position,
   consistent with a glossary being reference material rather than normative
   content.

This is a narrower scope than an equally reasonable but riskier design that also
substitutes a label at safe-pattern-recognizer atom/operand sites inside condition
text (restricted to whole-`Expr::Var` matches, never a substring). That design was
considered and is a legitimate future extension; it is not implemented here so this
issue ships a smaller, independently-verifiable change rather than a larger one
whose correctness depends on getting every atom/operand call site in
`document_render_expr.rs` right in one pass. `composite_lvalue_code_span_is_never_
rewritten_by_a_state_label` (`rust/fsl-tools/tests/document_glossary.rs`) is a
negative control pinning exactly this boundary: a `state:scr` label must never
touch a `scr[c].st`-shaped code span.

Threading: `render_requirements_document` gained one new parameter,
`glossary: Option<&AppliedGlossary<'_>>`, where `AppliedGlossary<'a> { glossary:
&'a Glossary, digest: &'a str }` bundles the parsed glossary with the digest of its
own raw file bytes (computed by the CLI, which does the file I/O; `fsl-tools` never
touches a filesystem). `Ctx` (the renderer's existing per-render context struct)
gained a `glossary: Option<&Glossary>` field, populated once at the top of
`render_requirements_document`; no other function signature in
`document_render_expr.rs` changed, since only `render_operation` and the new
`glossary_section` function ever consult it.

## `fslc document generate --glossary PATH`

Reuses the exact `error_output("document", message)` + `code` field convention
`generate`'s existing diagnostics already use. A new shared helper,
`load_glossary(path, expected_locale)`, is used by both `generate` and `check`
(below) — both need the identical loading rule, since both re-render with
whatever glossary (or lack of one) applies. It reads raw bytes (for the digest),
decodes them as UTF-8 (a decoding failure is an `"io"`-kind error, not a
`"document"`-kind one — it is a file-reading problem, not a glossary-content
problem), parses, and rejects a glossary whose own `locale` does not match
`--lang` (a category error: applying `ja` labels into an `en` document makes no
sense, and the glossary self-declares its locale for exactly this reason).

The envelope gains a `"glossary": {"digest": "sha256:...", "labels": N}` object
when `--glossary` was given, and a `"warnings"` array of `{"kind":
"label_unknown", "code": "FSL-DOC-LABEL-UNKNOWN", "target", "detail"}` entries when
any label target didn't resolve (default mode only — under `--strict` this is a
hard error instead, so no warning is ever emitted together with a `--strict`
failure). In the no-`-o` bypass path (`document_command` prints the raw Markdown
directly to stdout and never reaches the envelope, per #327's own convention),
warnings would otherwise be silently dropped since stdout must stay exactly the
document; they are printed to stderr instead in that path only.

`fslc document claims` gains no glossary awareness at all: it emits raw RCIR JSON
with no rendered prose, and its digests are exactly the ones the glossary must not
affect — accepting a `--glossary` flag there would be the "accepted but silently
ignored" anti-pattern issue #327's own design doc already rejected for the flags it
deferred.

## `fslc document check` glossary parity

Frontmatter gains one new optional key, `glossary_digest` (a plain, non-framed
`sha256:`-prefixed digest of the raw glossary file bytes — the same identity
`artifact_digest` uses for the whole document, not RCIR's canonical-JSON `framed_
digest` scheme, since the recorded fact is "this exact sidecar file"). Emitted only
when `--glossary` was given to `generate`, so a glossary-less document's bytes are
completely unaffected by this issue (verified directly:
`no_glossary_renders_byte_identically_to_no_glossary_argument`).

`check` needs the actual label strings to reproduce the labeled text, and the
artifact stores only a digest of them — so, unlike `lang`/`view` (which `check`
reads entirely from frontmatter with no flag of its own), reproducing a labeled
render requires the file itself. `check` therefore gains its own `--glossary PATH`
flag, loaded through the same shared `load_glossary` helper, and re-renders with
whatever it was given (or without, if nothing was given) before calling
`check_requirements_document`.

The comparison itself needed **no new parameter** on `check_requirements_document`:
that function already parses both the artifact's frontmatter and a freshly
rendered copy's frontmatter (to compare `renderer`/`renderer_version`/digests), and
the fresh copy's own `glossary_digest` already reflects whatever glossary `check`'s
caller actually supplied when it re-rendered. Comparing `frontmatter.glossary_
digest` against `fresh_frontmatter.glossary_digest` is the entire mechanism:

| Recorded (artifact) | Supplied (fresh re-render) | Detail |
|---|---|---|
| absent | absent | no reason — behaves exactly as before this issue |
| present, equal | present, equal | no reason |
| present | absent | `generated_with_glossary` |
| absent | present | `generated_without_glossary` |
| present, different | present, different | `glossary_digest_mismatch` |

Every mismatch is one new drift reason, `kind: "glossary_changed"`, code
`FSL-DOC-GLOSSARY-CHANGED` (not folded into `FSL-DOC-RENDERER-CHANGED`: the two are
semantically distinct, "the renderer build changed" vs. "the display-label input
changed," and the remediation differs completely — regenerate vs. supply/fix the
sidecar). A changed glossary skips per-claim-body and residue text comparison the
same way a changed renderer already does (the shared `skip_text` flag, renamed
from `renderer_changed` to reflect that both conditions now gate it) — comparing
labeled prose against unlabeled prose byte-for-byte would otherwise bury the one
meaningful reason (`glossary_changed`) under a flood of `claim_changed`/
`edit_outside_slot` noise for every claim a label happens to touch. Structural
checks (claim/slot presence, order, duplicates) are unaffected either way, since
glossary labels never change which claims or slots exist, only how their text
reads.

## What v1 does not do

- No expression-level label substitution (see above) — `state:`/`enum:` labels are
  visible only in the "Glossary" section, never inline in a condition.
- No glossary awareness in `fslc document claims`.
- No `property:` (or any other) glossary namespace beyond `action:`/`state:`/
  `enum:`.
- No reverse validation that every *declared* action/state/enum has a label — the
  glossary is opt-in presentation, not a coverage requirement.

## Verification evidence

`rust/fsl-tools/tests/document_glossary.rs` (17 tests): parsing and every
`GlossaryIssue` variant, including the duplicate-key mechanism itself (a single
duplicate, a triple duplicate, and a duplicate with identical values, all
detected); target validation for all three namespaces (known and unknown cases,
including a member matched to the wrong enum type); an unrecognized namespace and
a colonless target; the action-heading label format in both locales; the Glossary
section's content and sort order; the composite-lvalue negative control; and a
direct confirmation that passing `None` renders byte-identically to before this
issue.

`rust/fslc/tests/document_glossary_cli.rs` (13 tests): the happy path (envelope
shape, frontmatter, heading, Glossary section); `FSL-DOC-LABEL-CONFLICT` always an
error; `FSL-DOC-LABEL-UNKNOWN` as a default-mode warning and (on a fixture with an
empty `init {}` block, so no pre-existing #327 coverage issue masks it) a
`--strict` error; wrong schema and locale-mismatch as `FSL-DOC-GLOSSARY-INVALID`; a
missing glossary file as an `"io"`-kind error; the issue's own required digest test
(`claim_set_digest` and `spec_digest` unchanged, `artifact_digest` changed, across
three glossary states); `check` conformant with the same glossary; `glossary_
changed` for each of the three mismatch shapes, with an explicit assertion that no
`claim_changed`/`edit_outside_slot` noise accompanies it; and a regression guard
that a glossary-less artifact checks exactly as it did under issue #329.
