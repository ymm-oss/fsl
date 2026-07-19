<!-- SPDX-License-Identifier: Apache-2.0 -->

# `fslc document generate` / `fslc document claims`

Status: accepted. Implements issue #327.

This design wires the RCIR v1 projector (issue #325,
`docs/DESIGN-document-requirement-claim-ir.md`) and the ja/en controlled-language
renderer (issue #326, `docs/DESIGN-document-controlled-language-renderer.md`) into
two CLI entry points. It adds no new semantics: `fslc document generate` calls
`fsl_tools::project_requirement_claims_from_source` then
`fsl_tools::render_requirements_document`; `fslc document claims` calls only the
projector and serializes its result. Generated block markers and the drift check
(`fslc document check`) are issue #329; the glossary sidecar, evidence overlay, and
approval-target integration are issues #330/#332/#333 and are deliberately not wired
into `generate`'s flag surface here — see "Deferred flags" below.

## CLI surface

```
fslc document generate <spec.fsl> [--view requirements] [--lang ja|en]
                                  [--strict] [--strict-rendering] [-o requirements.md]
fslc document claims    <spec.fsl> [--view requirements] [-o requirements.claims.json]
```

`--view` accepts only `requirements` in v1 (reserved for `business`/`design`, issue
#334); any other value is a usage error. `--lang` defaults to `ja`. Both commands
follow the exact `-o`/no-`-o` convention already established by `ledger`/`html`/
`testgen`: with `-o`, the artifact is written to disk and a small JSON envelope goes
to stdout; without it, the raw artifact (Markdown for `generate`, the RCIR JSON
itself for `claims`) is printed directly to stdout with no envelope wrapper, so
`fslc document generate spec.fsl | less` and `fslc document claims spec.fsl | jq`
both work without an intermediate file.

`document claims`'s raw output (`-o` file content, or stdout when `-o` is absent) is
byte-for-byte the serialized `RequirementClaimSet` — no `"fsl"` envelope key is
merged in, unlike `fslc kernel`, because RCIR's own schema is closed
(`additionalProperties: false` at the top level) and a consumer must be able to
validate the command's output directly against
`schemas/fslc/document/requirement-claims.v1.schema.json` without stripping a
wrapper first. `generate`'s envelope (the `-o` case, or the wrapper fields that
would exist if content weren't bypassed) is a distinct, unversioned CLI-summary
shape, not part of RCIR:

```json
{
  "fsl": "1.0",
  "result": "generated",
  "kind": "requirements_document",
  "output": "requirements.md",
  "spec_digest": "sha256:...",
  "claim_set_digest": "sha256:...",
  "artifact_digest": "sha256:...",
  "coverage": {
    "authored_targets": 18, "rendered_targets": 18,
    "unattributed_targets": 0, "unsupported_targets": 0,
    "formula_fallbacks": 2
  },
  "provenance": {"completeness": "complete"}
}
```

`spec_digest`/`claim_set_digest` are read straight from the projected
`RequirementClaimSet::spec` (so `document claims`'s own output digests match
`generate`'s envelope for the same input, tested directly). `artifact_digest` is new
in this issue: a plain `sha256:`-framed digest of the exact rendered Markdown bytes,
computed with `approval::sha256_bytes` — the same primitive `fslc approval` already
uses for artifact binding, reused directly rather than re-exported from `fsl-tools`
(RCIR v1 already made this call for its own spec/claim-set digests; see that design
note's "deliberately a separate implementation" note). `coverage` and `provenance`
are read from `RequirementClaimSet::coverage`/`provenance` plus the renderer's
`formula_fallback_count`; they are not re-derived.

## Diagnostics

| Code | Condition | Default | `--strict` / `--strict-rendering` |
|---|---|---|---|
| `FSL-DOC-NO-REQUIREMENTS` | `claims.requirements` is empty | error | error (unconditional) |
| `FSL-DOC-UNTAGGED-TARGET` | `coverage.counts.unattributed > 0` | warning (rendered in the document's own "not linked to any requirement ID" section, issue #326) | error under `--strict` |
| `FSL-DOC-UNSUPPORTED-TARGET` | `coverage.counts.unsupported > 0` | warning (rendered in the document's own unsupported-elements list) | error under `--strict` |
| `FSL-DOC-FORMULA-FALLBACK` | `formula_fallback_count > 0` | info (counted in the envelope and the document's own classification line) | error under `--strict-rendering` |

Diagnostics are checked in this order: `FSL-DOC-NO-REQUIREMENTS` always, then (only
under `--strict`) `FSL-DOC-UNTAGGED-TARGET`, then `FSL-DOC-UNSUPPORTED-TARGET`, then
(only under `--strict-rendering`, after rendering) `FSL-DOC-FORMULA-FALLBACK`. A
failing diagnostic returns before any file is written. `FSL-DOC-PROVENANCE-PARTIAL`
from the issue's own diagnostics table is intentionally not implemented as a hard
`--strict` failure in v1: the origin registry is sparse by design for the
`requirements`/`spec` dialects (established in #325's `provenance_for` fallback), so
treating `completeness: partial` as a `--strict` error would reject specifications
that are otherwise fully covered. `provenance.completeness` is surfaced in the
envelope and in the rendered document (issue #326) so a caller can act on it without
the CLI hard-failing on a condition the issue itself only weakly specifies ("error
option").

`document claims` has no diagnostics gate at all — it is a raw projection dump, and
any consumer decides its own policy over `coverage`/`provenance`, consistent with
scoping the diagnostics table (issue #327's own heading) to `generate` only.

## Deferred flags

The issue's CLI table for `generate` also lists `--glossary PATH` (#330),
`--evidence PATH` (#332), and `--approval RECORD` (#333) next to the flags this
issue implements. They are not added here: none of #330/#332/#333 is implemented
yet, and a flag accepted now but silently ignored is worse than one that does not
exist yet — a caller has no way to tell "ignored" from "applied." Each of those
issues adds its own flag when the feature behind it exists.

`--glossary` landed in issue #330 (`docs/DESIGN-document-glossary.md`), on both
`generate` and `check`. `--evidence` (#332) and `--approval` (#333) remain
deferred for the same reason stated above.

## Verification evidence

`rust/fslc/tests/document_cli.rs`: byte-identical repeated `generate` runs
(acceptance criterion 1); `en` locale selection; `-o` writes the file and an
envelope with `sha256:`-framed digests and zeroed coverage for a clean fixture
(acceptance criterion 3); default-mode success and `--strict` failure
(`FSL-DOC-UNTAGGED-TARGET` / `FSL-DOC-UNSUPPORTED-TARGET`) against the `#325`
fixtures that already carry an unattributed target
(`document_claims_fixture.fsl`) and unsupported targets (`document_kpi_fixture.fsl`)
(acceptance criterion 2); `--strict-rendering` failure
(`FSL-DOC-FORMULA-FALLBACK`); usage errors for an unknown `--view`/`--lang` value and
an unknown subcommand; `document claims`'s stdout validates against the checked-in
RCIR v1 schema with the `jsonschema` crate (mirroring `fsl-tools`'s own schema test);
`document claims`'s `-o` file content and envelope digests agree with `generate`'s
envelope digests for the same input.
