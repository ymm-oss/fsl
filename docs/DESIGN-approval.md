# Spec-digest-bound approval records

Issue: #190.

## Problem and record

Stakeholders often approve a rendered ledger, HTML report, or scenarios file,
not raw FSL. An approval must therefore bind the rendering, its generation
input, and the exact normalized specification meaning.

```bash
fslc approval create spec.fsl \
  --rendered review.html --rendering-kind html \
  --approver "Product Owner" \
  --command "fslc html spec.fsl -o review.html" \
  -o spec.approval.json
```

`fslc-approval.v0` records:

- approval timestamp and approver;
- spec name, requirement IDs, and SHA-256 of the canonical post-desugaring
  kernel AST with source locations removed;
- rendering kind/path/SHA-256 and the recorded generation command;
- an embedded source snapshot of the entry and every relative `from "..."`
  dependency.

The stable schema is `schemas/fslc/approval-record.v0.schema.json`. The source
snapshot makes the record a self-contained semantic baseline rather than a
hash that can say “changed” but cannot explain how.

## Checking and ledger status

```bash
fslc approval check spec.fsl --record spec.approval.json
fslc approval check spec.fsl --record spec.approval.json --rendered review.html
fslc ledger spec.fsl --approval spec.approval.json
```

The spec is `approved` when its normalized digest equals the record. It is
`drifted` otherwise. When a current rendering is supplied, its bytes are also
classified `approved`/`drifted`; without one, rendering status is
`not_checked`. Overall status is drifted if either checked surface drifted.

Ledger adds an approval column only when `--approval` is present, preserving
the default ledger bytes. Every requirement ID carries the same record status
because v0 approves one compiled spec unit; finer per-requirement baselines can
be added without weakening this whole-spec binding.

## Semantic diff baseline

```bash
fslc diff --approval spec.approval.json current.fsl --depth 8
```

The command safely materializes the embedded source group, recomputes its
normalized digest (rejecting a tampered snapshot), and passes the approved root
plus current spec to the VCS-independent #176 semantic diff. Output records the
approved digest, approver, timestamp, and
`materialization:"embedded_source_snapshot"`.

This is stacked on the semantic-diff core. The approval layer does not change
boundedness: absence of a difference through depth K is not unbounded
equivalence.

## Boundaries

- Source dependencies outside the entry spec's directory are rejected in v0;
  a portable sidecar cannot safely embed an arbitrary parent tree implicitly.
- Rendering SHA proves byte identity, not that the stakeholder understood it.
- The approver string/timestamp are audit metadata, not a cryptographic
  signature. Repository signing policy may sign the committed JSON separately.
- Comment/whitespace-only FSL changes do not drift the normalized spec digest,
  but may drift a regenerated rendering. This separates semantic and rendering
  surfaces explicitly.
