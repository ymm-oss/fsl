---
name: fsl-requirements-document
description: Generate, edit, and re-verify a human-readable requirements document (Markdown) from a checked FSL requirements/spec dialect file, using fslc document generate/claims/check. The agent is a non-normative editor and review-support assistant, never a compiler — it may only fill in the document's editable slot (background, summary, review notes) and propose advisory findings; it must never rewrite a generated claim block or invent normative language FSL does not state. Use when the deliverable is a requirements document artifact for stakeholders, not FSL authoring (use fsl-requirements for that) or a verification audit ledger (use fsl for fslc ledger).
---

# FSL Requirements Document — Generate, Annotate, Re-check

This skill turns a checked `requirements` (or direct `spec`) dialect file into a
requirements document stakeholders can read, using the Requirement Claim IR (RCIR)
pipeline: `fslc document claims` (the machine-readable contract), `fslc document
generate` (deterministic ja/en Markdown), and `fslc document check` (structural
drift detection). See `../fsl/SKILL.md` and `../fsl/reference.md` for FSL syntax,
verification, and the general repair protocol — this skill only covers the
document layer built on top of an already-checked spec. If the `.fsl` itself still
needs authoring or repair, use `fsl-requirements` (or `fsl-design`) first and come
back here once `fslc check`/`fslc verify` are green.

## What this skill is for

The agent is an **editor of non-normative content and a review-support
assistant**, not a compiler. Every normative sentence in the document — enablement
conditions, effects, postconditions, fairness, deadlines, acceptance/forbidden
traces — comes from a fixed template deterministically rendered from the checked
FSL (issue #326). The agent never writes that text; it only fills in the one
editable slot the generator leaves open (`background`), and only ever *proposes*
findings about the source material, never encodes them into the normative
sections itself.

## Permitted operations

- Edit the non-normative `background` slot: project context, a plain-language
  summary, review notes for stakeholders.
- Propose a chapter reordering based on RCIR's own structure (e.g. grouping
  requirements by theme) — a suggestion for a human to apply, not a rewrite of
  generated content.
- Draft glossary candidates (issue #330) for terms that appear in requirement text
  or claims.
- Flag a candidate mismatch between the original requirement text and the
  formalized meaning as an advisory note — for example, source text that says
  "exactly once" where the formalized guard only enforces "at most once." **Never
  strengthen the FSL-derived meaning to match the source text to make the mismatch
  disappear.** Surfacing the gap is the value; resolving it is a human decision.
- Flag a missing requirement tag, a missing `acceptance`/`forbidden` case, or an
  `undecided` item that looks resolvable, as a review note.
- Point a stakeholder at a requirement whose assurance class is `not_run` in
  every dimension (issue #332) as a review note — never supply evidence
  yourself or assert a stronger class than the document already shows; the
  `--evidence` files come from real verification runs, not from the agent.
- Point a stakeholder at a document with no "Approval records" section (issue
  #333) as a review note ("this document is not yet approved") — never
  fabricate an approval record or assert that a document is approved without
  one.

## Forbidden operations

- Rewriting the content of any `<!-- fsl:claim begin ... --> ... <!-- fsl:claim
  end -->` block. That text is generated output; a correction always goes back to
  the FSL source, never to the Markdown (`fslc document check`, issue #329,
  detects and fails on exactly this).
- Adding a must/must-not statement to a normative section that the FSL itself does
  not state.
- Rewording an assurance class — in particular, never turn `bounded` into `proved`
  in prose. Assurance vocabulary is a verification fact, not a copy-editing choice.
- Inferring a general requirement from a single acceptance trace and writing it
  into a normative section. An acceptance trace is one example execution; RCIR
  says so explicitly, and the document must keep saying so.
- Guessing at unknown provenance. If a claim's source is not fully attributed,
  say so as a review note — never fabricate a citation.
- Collapsing a claim's many-to-many requirement relation into a single
  requirement. RCIR's relations are intentionally many-to-many; do not simplify
  them away when writing review notes or a summary.
- Presenting an approval record as proof of intent fidelity (issue #333). It
  records that a named approver reviewed the spec-document correspondence at
  a specific digest — report it like an assurance class, never upgrade it
  into a claim that the document matches stakeholder intent.

## Recommended workflow

```bash
fslc verify spec.fsl --depth 8
fslc document claims spec.fsl -o requirements.claims.json
fslc document generate spec.fsl --lang ja -o requirements.md
# agent edits only the non-normative `background` slot in requirements.md
fslc document check spec.fsl requirements.md
fslc ledger spec.fsl --depth 8 -o requirements-ledger.md

# optional: overlay saved verification evidence as a per-requirement
# assurance class (issue #332) — the same envelope shape --evidence
# already accepts for fslc ledger; check must be given the identical
# --evidence set or it reports evidence_changed, not a hard error
fslc document generate spec.fsl --lang ja --evidence verify-run.json -o requirements.md
fslc document check spec.fsl requirements.md --evidence verify-run.json

# optional: bind a reviewed artifact to a digest (issue #333) and display it
# in the document — a human approves, the agent never creates the record
fslc approval create spec.fsl --kind requirements_document \
  --artifact requirements.md --approver alice -o requirements.approval.json
fslc document generate spec.fsl --lang ja \
  --approval requirements.approval.json -o requirements.md
fslc document check spec.fsl requirements.md --approval requirements.approval.json
```

1. Confirm the spec is checked (`fslc check`, or the existing verification flow
   already run for this spec — do not generate a document from a spec that does
   not pass `fslc check`). If `fslc document generate`/`claims` instead reports
   `FSL-DOC-DIALECT-UNSUPPORTED` (issue #334), the input is a dialect other than
   `spec`/`requirements` (e.g. `business`, `domain`, `dbsystem`, `ai_component`)
   — this is a scope boundary of the document layer, not a defect in the spec;
   report it as such and do not attempt to work around it by rewriting the
   spec's dialect keyword or otherwise coercing the input.
2. Fetch the RCIR contract with `fslc document claims` when you need the raw
   claim/requirement/coverage structure (e.g. to draft a summary or a glossary
   candidate) rather than re-parsing `.fsl` yourself.
3. Generate the canonical Markdown with `fslc document generate`.
4. Edit only the `background` slot. Never touch text outside
   `<!-- fsl:slot begin name="background" normative="false" -->` /
   `<!-- fsl:slot end -->`.
5. Run `fslc document check spec.fsl requirements.md` before handing the document
   back. `document_conformant` (exit 0) means the edit stayed inside the slot and
   nothing drifted; `document_drifted` (exit 1) means either a normative block was
   touched (revert it) or the spec moved since generation (regenerate instead of
   patching the Markdown by hand — there is no reverse-sync from Markdown to FSL).
6. Generate the audit ledger separately with `fslc ledger` when the deliverable
   also needs verification-status-by-requirement, not just what is required.

**Keep the two artifacts distinct**: `requirements.md` says *what* is required;
`requirements-ledger.md` says *how far it has been verified and what a human still
needs to judge*. Do not merge review-status commentary into the requirements
document's normative sections, and do not put requirement text into the ledger.

## CI example: fail a build on document drift

```yaml
# .github/workflows/requirements-document.yml (example for a downstream project)
name: requirements-document
on: [pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Regenerate and diff the requirements document
        run: |
          fslc document check spec.fsl requirements.md
```

`fslc document check` exits 0 (`document_conformant`), 1 (`document_drifted`), 2
(parse/type/semantics/schema/I/O error), or 3 (internal error) — wire the job to
fail on any nonzero exit. A drifted result means either an out-of-slot edit
happened or the spec changed after `requirements.md` was last generated; the fix
in both cases is `fslc document generate` (never a hand patch to the Markdown).

## Division of responsibility with other skills

- **`fsl`** — FSL syntax, the verifier commands, and the general JSON-result
  repair protocol. Load it before touching `.fsl` syntax or interpreting a
  `verify`/`check` failure.
- **`fsl-requirements`** — authors and repairs the `requirements` dialect `.fsl`
  file itself (requirement IDs, acceptance/forbidden traces, NFRs). Use it first
  if the spec is not yet checked; this skill only operates on an already-checked
  spec and never edits the `.fsl`.
- **This skill** — turns an already-checked spec into a stakeholder-facing
  document, edits only its non-normative slot, and re-verifies structural
  conformance after every edit. It does not verify FSL semantics itself (that is
  `fsl`/`fsl-requirements`'s job) and does not produce an audit ledger (that is
  `fslc ledger`, documented in `fsl`'s reference).
