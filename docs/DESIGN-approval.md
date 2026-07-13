# Digest-bound approval records

Issue: #190

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

The direct result is `approval_check` with `status:"approved"` or
`status:"drifted"`. Drift remains an analysis result and exits 0. A malformed,
unsupported, wrong-spec, or locally unavailable Git-baseline record is an error
and exits 2. Unknown sidecar fields are rejected so schema changes cannot be
silently ignored.

When drift is reported, compare the approved commit to the current working tree
(including uncommitted edits) without manually materializing the baseline:

```bash
fslc approval diff specs/order.fsl --record order.approval.json --depth 8
```

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

Supported target kinds are `ledger`, `html`, and `scenarios`.

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

The `approver` string is attribution, not a cryptographic identity. Authenticity
and authorization remain repository controls: signed commits where required,
CODEOWNERS/review policy, branch protection, and audit-log retention. Editing the
sidecar is equivalent to editing other governed repository evidence. A future
schema version may add detached signatures without changing v1 verification.

## Tests

Native integration coverage proves:

- create then check produces `approved`;
- comments do not move the spec or artifact digest;
- a semantic edit produces `drifted` with the baseline digest;
- rendering digest changes are classified separately;
- `approval diff` materializes the approved commit and reports semantic change;
- ledger output matches the approval snapshot;
- ledger, HTML, and scenarios are valid review targets.
