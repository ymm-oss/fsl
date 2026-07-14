# Digest-bound approval records

Issues: #190, #269

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

Supported target kinds are `ledger`, `html`, and `scenarios`. Omitting
`--signing-key` continues to produce this exact unsigned v1 contract.

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
`b"fslc.approval.v2\0" || canonical_json(record_without_signature.value)`.
Canonical JSON recursively sorts object keys and emits compact UTF-8 JSON while
preserving array order. This binds the schema, signature algorithm and key ID,
specification, target, and approval fields while remaining insensitive to JSON
object order and whitespace.

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
fields inside reviewed domain state. HTML normalizes embedded `elapsed_s` values. Ledger
Markdown is hashed as rendered. This preserves review-visible content while
preventing solver timing from producing false drift.

The approval overlay itself is not part of a ledger target digest. The base
ledger is rendered and hashed first, then approval status is added. This avoids
a recursive artifact hash.

## Status and failure reasons

An approval is `approved` only when all three bindings match:

- normalized specification digest;
- normalized rendered-artifact digest;
- renderer version.

Otherwise it is `drifted` with one or more machine reasons:
`spec_changed`, `rendering_changed`, or `renderer_changed`. Ledger output shows
the complete baseline digest and an executable `fslc approval diff` command for
every drifted requirement. Repeatable `--approval` records are applied in CLI
order; a later record replaces an earlier decision for the same requirement ID.

## Trust boundary

For unsigned v1, the `approver` string is attribution rather than a
cryptographic identity. For signed v2, successful verification proves that the
holder of the matching private key signed the complete record. Supplying a
public key as a trust anchor does not by itself establish that the signer is
authorized to approve a requirement. Authorization remains repository and
organizational policy: controlled trust-key distribution, CODEOWNERS/review
policy, branch protection, and audit-log retention. v1 parsing and verification
remain unchanged and are never used as a fallback for a v2 record.

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
