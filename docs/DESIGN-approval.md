# Digest-bound approval records

Issues: #190, #269, #333

## Goal

Stakeholders often approve a rendered ledger, HTML report, or concrete scenario
set rather than reading the FSL source. `fslc approval` binds that reviewed
artifact to the normalized specification that produced it. A later check can
therefore distinguish an unchanged approval from specification, rendering, or
renderer drift without treating a checkbox in generated Markdown as evidence.

The implementation is native Rust. The frozen Python reference is intentionally
unchanged.

## Workflow

Approval creation requires a tracked specification and a clean Git worktree.
This makes the recorded commit a reconstructable semantic-diff baseline. First
generate and review an artifact using the same inputs that will be recorded:

```bash
fslc ledger specs/order.fsl --depth 8 -o order-ledger.md
fslc approval create specs/order.fsl \
  --kind ledger \
  --artifact order-ledger.md \
  --approver alice \
  --depth 8 \
  --signing-key alice-private.pem \
  -o order.approval.json
```

`approval create` regenerates the target before writing the record. A stale or
wrong artifact is rejected rather than approved. Omitting `--requirement`
approves every requirement ID found in the lowered model; the option is
repeatable when a decision covers only selected requirements. A specification
without requirement IDs is rejected because it cannot produce a meaningful
per-requirement ledger decision.

A generated requirements document (issue #325's `fslc document generate`) is
approved the same way, under its own target kind:

```bash
fslc document generate specs/order.fsl --lang ja -o order-requirements.md
fslc approval create specs/order.fsl \
  --kind requirements_document \
  --artifact order-requirements.md \
  --approver alice \
  -o order-requirements.approval.json
```

Unlike the other three kinds, `requirements_document` creation does not require
the reviewed artifact's bytes to match a fresh rendering exactly — issue #329's
one editable slot (`background`) may legitimately differ. Instead, the
reviewed artifact must pass `fslc document check`'s own structural conformance
gate (claim blocks, markers, and residue text outside the slot must match);
the target records both the fresh *canonical* rendering digest (the same
value `fslc document generate` reports) and a separate digest of the reviewed
file's literal bytes. The first detects reproducibility drift; the second
binds editable background text as part of the presentation actually approved. A `--kind
requirements_document` approval also accepts the same `--glossary`/`--evidence`
flags `document generate` does (recorded, not re-supplied at check time from
the command line — `approval check` re-reads the same paths from the current
filesystem); `--depth`/`--deadlock`/`--engine` are rejected for this kind, and
`--glossary`/`--evidence` are rejected for the other three, since neither pair
means anything for the other's generation model.

Once approved, `fslc document generate --approval` can display the record
directly in the rendered document itself:

```bash
fslc document generate specs/order.fsl --lang ja \
  --approval order-requirements.approval.json \
  -o order-requirements.md
```

This fails closed (`FSL-DOC-APPROVAL-DRIFTED`) if the current rendering no
longer matches what was approved, and (for a signed record) requires a
matching `--trust-key` the same way `approval check` does — a stakeholder
document must never display an unverifiable signed approval. `fslc document
check` accepts the identical `--approval` flag to reproduce the displayed
section for its own structural comparison (never re-verifying a signature;
admission was `generate`'s job).

Check a record directly or include it in a ledger:

```bash
fslc approval check specs/order.fsl --record order.approval.json
fslc ledger specs/order.fsl --approval order.approval.json
```

Signed v2 records require an explicit trust anchor on every consuming command:

```bash
fslc approval check specs/order.fsl --record order.approval.json \
  --trust-key alice-public.pem
fslc ledger specs/order.fsl --approval order.approval.json \
  --trust-key alice-public.pem
```

The direct result is `approval_check` with `status:"approved"` or
`status:"drifted"`. A signed record can instead report
`status:"signature-invalid"`, which exits 1 and never grants approval. Drift
remains an analysis result and exits 0. A malformed,
unsupported, wrong-spec, or locally unavailable Git-baseline record is an error
and exits 2. A signed record without its matching trust key is also a
configuration error and exits 2. Unknown sidecar fields are rejected so schema
changes cannot be silently ignored.

When drift is reported, compare the approved commit to the current working tree
(including uncommitted edits) without manually materializing the baseline:

```bash
fslc approval diff specs/order.fsl --record order.approval.json --depth 8
```

Pass the same repeatable `--trust-key` option when diffing a signed record.

The result is the ordinary bounded `semantic_diff` envelope plus `approval`
baseline metadata. Before comparison, the materialized specification is hashed
again and must match the record's baseline digest.

## Record contract

The committed sidecar follows
[`schemas/fslc/approval/approval-record.v1.schema.json`](../schemas/fslc/approval/approval-record.v1.schema.json):

```json
{
  "schema": "fslc.approval.v1",
  "spec": {
    "path": "specs/order.fsl",
    "digest_algorithm": "fsl-kernel-ast-v1+sha256",
    "digest": "sha256:<64 hex>",
    "git_commit": "<full commit>"
  },
  "target": {
    "kind": "ledger",
    "path": "order-ledger.md",
    "digest_algorithm": "fsl-rendered-artifact-v1+sha256",
    "digest": "sha256:<64 hex>",
    "generator": "fslc",
    "generator_version": "2.7.0",
    "inputs": {"depth": 8, "deadlock": "ignore", "engine": "bmc"}
  },
  "approval": {
    "approver": "alice",
    "approved_at": "2026-07-13T04:00:00Z",
    "requirements": ["REQ-1"]
  }
}
```

Supported target kinds for v1/v2 are `ledger`, `html`, and `scenarios`.
Omitting `--signing-key` continues to produce this exact unsigned v1
contract; **v1/v2 are unchanged by issue #333** — a `requirements_document`
target is never admitted into v1/v2's closed `kind` enum, by design (see
below).

Signed records use
[`schemas/fslc/approval/approval-record.v2.schema.json`](../schemas/fslc/approval/approval-record.v2.schema.json).
They preserve the v1 `spec`, `target`, and `approval` objects and add:

```json
{
  "schema": "fslc.approval.v2",
  "signature": {
    "algorithm": "ed25519",
    "key_id": "sha256:<64 hex>",
    "value": "<base64url without padding>"
  }
}
```

`--signing-key` accepts an unencrypted PKCS#8 PEM Ed25519 private key.
`--trust-key` accepts an SPKI PEM Ed25519 public key and is repeatable. The key
ID is SHA-256 over the raw 32-byte public key. No other key encodings or
algorithms are accepted.

The detached signature covers
`b"fslc.approval.v2\0" || canonical_json(record_without_signature.value)`
(v4's own signature covers `b"fslc.approval.v4\0" || ...` instead — the
domain-separation prefix is always the record's own `schema` string, so a v2
signature can never verify against a v4 payload or vice versa). Canonical
JSON recursively sorts object keys and emits compact UTF-8 JSON while
preserving array order. This binds the schema, signature algorithm and key ID,
specification, target, and approval fields while remaining insensitive to JSON
object order and whitespace.

### `requirements_document` (v3 unsigned, v4 signed)

Issue #333's own acceptance criterion 1 requires that v1/v2's closed `kind`
enum not be implicitly widened. A `requirements_document` target therefore
lands as an entirely new schema revision pair —
[`schemas/fslc/approval/approval-record.v3.schema.json`](../schemas/fslc/approval/approval-record.v3.schema.json)
(unsigned) and
[`schemas/fslc/approval/approval-record.v4.schema.json`](../schemas/fslc/approval/approval-record.v4.schema.json)
(signed, v3 plus the same `signature` object v2 adds to v1) — rather than a
conditional branch inside v1/v2. `spec`/`approval` are byte-identical to v1's;
`target` differs in three ways:

```json
{
  "schema": "fslc.approval.v3",
  "spec": { "...": "identical shape to v1" },
  "target": {
    "kind": "requirements_document",
    "path": "order-requirements.md",
    "digest_algorithm": "fsl-rendered-requirements-document-v1+sha256",
    "digest": "sha256:<64 hex>",
    "reviewed_digest_algorithm": "fsl-reviewed-requirements-document-v1+sha256",
    "reviewed_digest": "sha256:<64 hex>",
    "claim_set_digest_algorithm": "fsl-rcir-claim-set-v1+sha256",
    "claim_set_digest": "sha256:<64 hex>",
    "generator": "fslc",
    "generator_version": "2.7.0",
    "inputs": {
      "view": "requirements",
      "lang": "ja",
      "glossary": null,
      "evidence": []
    }
  },
  "approval": { "...": "identical shape to v1" }
}
```

- **`digest_algorithm`** is a distinct constant
  (`fsl-rendered-requirements-document-v1+sha256`) naming a distinct digest
  domain (a rendered requirements document) — but the recipe is the same
  plain, unframed `sha256` over rendered bytes that `fsl-rendered-artifact-v1
  +sha256` already uses for the other three kinds. This value is literally
  what `fslc document generate`'s own `artifact_digest` envelope field
  reports for the same canonical rendering, so a caller can compare the two
  directly without any approval-specific tooling.
- **`claim_set_digest_algorithm`/`claim_set_digest`** are new, required-only-
  for-this-kind fields recording RCIR's own claim-set digest (issue #325,
  `fsl-rcir-claim-set-v1+sha256`) alongside the artifact digest. No other
  target kind has an RCIR concept at all, so `evaluate()`'s new
  `claim_set_changed` reason (below) only ever fires for this kind.
  `spec.digest` (`fsl-kernel-ast-v1+sha256`) remains the join key between
  `fslc::approval` and RCIR's own `spec_digest` — the two are independently
  implemented but produce the identical value by construction (see
  `rust/fsl-tools/src/document_digest.rs`'s doc comments, written specifically
  so this issue could join on that identity).
- **`reviewed_digest_algorithm`/`reviewed_digest`** bind the exact Markdown
  bytes passed to `approval create`, including the editable background slot.
  Both `approval check` and `document generate --approval` re-read
  `target.path`; a later replacement or edit is reported as
  `artifact_changed`, independently of canonical rendering drift. The overlay
  displays this reviewed-artifact digest, not the canonical-rendering digest.
- **`inputs`** replaces the solver triple (`depth`/`deadlock`/`engine` —
  meaningless for a deterministic RCIR projection) with the actual
  reproducibility inputs a rendering depends on: `view`/`lang` (read from the
  reviewed artifact's own frontmatter, not a new CLI flag) and the exact
  `--glossary`/`--evidence` file set (each recorded as `{path, digest}`, so
  `approval check` can re-read the same paths from the current filesystem
  and reproduce the rendering deterministically). A record's `inputs` shape
  is therefore revision-determined: v1/v2 always carry the solver triple, v3/
  v4 always carry this document-input shape — `fslc approval create` rejects
  `--depth`/`--deadlock`/`--engine` for `--kind requirements_document` and
  rejects `--glossary`/`--evidence` for the other three kinds.

`approval create --kind requirements_document`'s conformance gate (rather than
the byte-equality the other three kinds use) is exactly `fslc document
check`'s own conformance check: `fsl_tools::check_requirements_document`
against a fresh canonical rendering (with the placeholder background text,
not the reviewed file's actual background content). A tampered claim block,
missing marker, or edit outside the slot fails creation with the same drift
`reasons` `document check` itself would report — `document check` and
`approval create` share one conformance definition, not two.

## Digest contracts

### Specification digest

`fsl-kernel-ast-v1+sha256` hashes the fully lowered kernel AST. Compose/use and
dialect dependencies are expanded before hashing, so a referenced-spec change
moves the digest. Source-location objects are removed; comments, whitespace,
line movement, and path spelling therefore do not create false specification
drift. Requirement metadata remains in the AST and is covered.

The digest is deliberately separate from the verification cache key. Cache keys
include source text, implementation versions, and verification options to avoid
stale diagnostics; those inputs would make a long-lived human approval unstable.

v1 uses one conservative whole-spec digest plus a list of approved requirement
IDs. Any semantic change marks every requirement in that record `drifted`.
Requirement-local hashing is deferred until a sound dependency slice can account
for shared state, actions, and cross-cutting properties.

### Rendered artifact digest

`fsl-rendered-artifact-v1+sha256` binds the reviewed presentation while removing
execution-only noise. Scenario JSON omits only top-level execution metadata
(`cost` and `cache`) before canonical serialization, preserving same-named
fields inside reviewed domain state. HTML preserves its historical normalization
of embedded `elapsed_s` values and normalizes solver check time, Z3 counters,
and memory only in the top-level verify envelope. Property identities, check
counts, and same-named fields in reviewed domain state remain bound. Ledger
Markdown is hashed as rendered. This preserves review-visible content while
preventing solver timing from producing false drift.

The approval overlay itself is not part of a ledger target digest. The base
ledger is rendered and hashed first, then approval status is added. This avoids
a recursive artifact hash.

### Requirements-document artifact and claim-set digests

`fsl-rendered-requirements-document-v1+sha256` is a plain, unframed `sha256`
over the canonical rendered Markdown — the identical recipe `fslc document
generate`'s own `artifact_digest` envelope field already uses, over the
identical bytes (the canonical rendering, with the editable slot's fixed
placeholder text). `fsl-reviewed-requirements-document-v1+sha256` separately
hashes the reviewed file's literal bytes, including that slot. Unlike
`ledger`/`html`/`scenarios`, there is no execution-only noise to strip: RCIR
rendering is fully deterministic, so nothing needs normalizing before hashing.

`fsl-rcir-claim-set-v1+sha256` (issue #325) is recorded separately alongside
the artifact digest, not folded into it, so a caller can distinguish "the
requirements projection itself changed" (`claim_set_changed`) from "only
presentation changed" (`rendering_changed` alone — a renderer version bump,
label, or evidence-overlay change with no change to what the RCIR claims say)
from "the spec moved somewhere the claim set doesn't reflect" (`spec_changed`
without `claim_set_changed`).

`fslc document generate --approval`'s own drift check (a lighter-weight,
display-only check, distinct from `fslc approval check`) compares the
supplied record's `spec.digest`/`target.digest`/`target.claim_set_digest`
directly against the values the current `document generate` invocation
already computed and re-reads `target.path` to compare its literal bytes with
`target.reviewed_digest`. It performs no additional live rendering and no Git
baseline reconstruction, since the overlay's job is display, not the
authoritative approval-governance check `fslc approval check` performs.

Adding the approval frontmatter binding and reference section advances the
requirements-document renderer from evidence overlay version `1.2.0` to `1.3.0`.

## Status and failure reasons

An approval is `approved` only when all applicable bindings match:

- normalized specification digest;
- normalized rendered-artifact digest;
- renderer version;
- for `requirements_document` only: the RCIR claim-set digest.
- for `requirements_document` only: the literal reviewed-artifact digest.

Otherwise it is `drifted` with one or more machine reasons: `spec_changed`,
`rendering_changed`, `renderer_changed`, or (`requirements_document` only)
`claim_set_changed` and `artifact_changed`. Ledger output shows the complete baseline digest and an
executable `fslc approval diff` command for every drifted requirement.
Repeatable `--approval` records are applied in CLI order; a later record
replaces an earlier decision for the same requirement ID.

## Approval does not prove intent fidelity

An approval record — of any kind, but especially `requirements_document` — is
an organizational record that the named approver reviewed the correspondence
between the specification and the reviewed artifact **at a specific digest**.
It is not, and cannot be, a proof that the artifact captures the original
business intent correctly: no mechanical check exists for that, and none is
implied by a green `approved` status. `fslc document generate --approval`'s
rendered "Approval records" section states this explicitly, in both locales,
as the section's own first line — never only in this design document — so a
reader of the generated document sees the same caveat a reader of this doc
does:

> 承認記録は、記載された承認者が仕様と本ドキュメントの対応を確認したことを示す組織上の記録である。原意への忠実性を証明するものではない。
>
> An approval record is an organizational record that the named approver
> reviewed the correspondence between the specification and this document.
> It does not prove fidelity to original intent.

`skills/fsl-requirements-document/SKILL.md` carries the same guardrail for an
agent operating on a generated document: never present an approval record as
proof of intent fidelity, and never fabricate one.

## Trust boundary

For unsigned v1, the `approver` string is attribution rather than a
cryptographic identity. For signed v2, successful verification proves that the
holder of the matching private key signed the complete record. Supplying a
public key as a trust anchor does not by itself establish that the signer is
authorized to approve a requirement. Authorization remains repository and
organizational policy: controlled trust-key distribution, CODEOWNERS/review
policy, branch protection, and audit-log retention. v1/v3 parsing and
verification remain unchanged and are never used as a fallback for a v2/v4
record (respectively); a v2 signature can never verify a v4 payload or vice
versa, since the domain-separation prefix is always the record's own
`schema` string.

## Tests

Native integration coverage proves:

- create then check produces `approved`;
- comments do not move the spec or artifact digest;
- a semantic edit produces `drifted` with the baseline digest;
- rendering digest changes are classified separately;
- `approval diff` materializes the approved commit and reports semantic change;
- ledger output matches the approval snapshot;
- ledger, HTML, and scenarios are valid review targets.
- v1 remains unsigned and byte-shape compatible;
- v2 signing is stable across JSON object reordering;
- missing or mismatched trust anchors fail closed;
- tampering reports `signature-invalid` and never appears approved in a ledger;
- signed checks, semantic diffs, and ledgers report the signature status.

Issue #333 adds:

- `requirements_document` create + check round-trips as `approved`, records
  schema `fslc.approval.v3`, and validates against
  `schemas/fslc/approval/approval-record.v3.schema.json`;
- a requirement-text edit produces `drifted` with `spec_changed`,
  `claim_set_changed`, and `rendering_changed` together;
- editing the `background` slot before approval leaves the canonical digest
  unchanged but produces a distinct reviewed-artifact digest; changing it
  after approval reports `artifact_changed`, and restoring it returns to approved
  (creation succeeds and the digest matches an unedited artifact's);
- tampering a claim block is rejected at `approval create` itself, with the
  same drift `reasons` `fslc document check` would report;
- `--glossary`/`--evidence` are recorded as `{path, digest}` `inputs` and
  reproduced at check time from the current filesystem; a changed glossary
  file surfaces as `rendering_changed` only, never `spec_changed`;
- signed `requirements_document` records use schema `fslc.approval.v4`,
  verify against a matching `--trust-key`, and reject a tampered signature;
- `--depth`/`--deadlock`/`--engine` are rejected for `--kind
  requirements_document`, and `--glossary`/`--evidence` are rejected for the
  other three kinds;
- a hand-edited record that widens `kind` across a schema revision boundary
  (a v1 record edited to `kind: requirements_document`, or a v3 record edited
  to `kind: ledger`) is rejected — the closed-contract enforcement itself;
- `approval diff` accepts a `requirements_document` record (it only ever
  reads `spec.*`, unaffected by the new `target` shape);
- `fslc document generate --approval` renders a matching "Approval records"
  section (with the intent-fidelity disclaimer, in both locales), rejects a
  drifted or non-`requirements_document` record, and fails closed on an
  unverifiable signed record; `fslc document check --approval` reproduces the
  same section and reports `approval_changed` when the flag is omitted,
  without ever verifying a signature itself.
