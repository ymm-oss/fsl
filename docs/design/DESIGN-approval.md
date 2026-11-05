# Digest-bound approval records

Issues: #190, #269, #333, #1421

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
fslc ledger examples/e2e/2_requirements.fsl --depth 8 -o expense-ledger.md
fslc approval create examples/e2e/2_requirements.fsl \
  --kind ledger \
  --artifact expense-ledger.md \
  --approver alice \
  --depth 8 \
  -o expense.approval.json
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
fslc document generate examples/e2e/2_requirements.fsl --lang ja -o expense-requirements.md
fslc approval create examples/e2e/2_requirements.fsl \
  --kind requirements_document \
  --artifact expense-requirements.md \
  --approver alice \
  -o expense-requirements.approval.json
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
fslc document generate examples/e2e/2_requirements.fsl --lang ja \
  --approval expense-requirements.approval.json \
  -o expense-requirements.md
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
fslc approval check examples/e2e/2_requirements.fsl --record expense.approval.json
fslc ledger examples/e2e/2_requirements.fsl --approval expense.approval.json
```

`--signing-key` turns the same creation into a signed v6 record. Signed
records require an explicit trust anchor for approval check, diff, ledger,
and document generation (document check remains structural) —
the examples below use an Ed25519 key pair the approver holds
(`alice-private.pem` and its SPKI public half `alice-public.pem`):

```bash
fslc approval create examples/e2e/2_requirements.fsl \
  --kind ledger \
  --artifact expense-ledger.md \
  --approver alice \
  --depth 8 \
  --signing-key alice-private.pem \
  -o expense-signed.approval.json
fslc approval check examples/e2e/2_requirements.fsl --record expense-signed.approval.json \
  --trust-key alice-public.pem
fslc ledger examples/e2e/2_requirements.fsl --approval expense-signed.approval.json \
  --trust-key alice-public.pem
```

A completed check returns `approval_check` with `status:"approved"` or
`status:"drifted"`. A signed record can instead report
`status:"signature-invalid"`, which exits 1 and never grants approval. Drift
remains an analysis result and exits 0, as ordinary nongating `diff` findings do
(`docs/manual/LANGUAGE.md` §12). Explicit semantic/batch diff gates instead exit 1 when
their gate fails. Drift is the expected state during development — you
approved at one commit and kept working — and the command's job is to report
what changed and hand you `fslc approval diff`, not to fail a build. The ledger
renders it `⚠` accordingly, distinct from the `❌` it gives
`signature-invalid`. This differs deliberately from `fslc document check`,
whose `document_drifted` exits 1: there, drift means a document contradicts the
spec it documents, which is a state to correct, not a normal step in the loop.
`status:"approved"` confirms digest binding, not a verification pass: the record binds
digests, and the envelope carries no verification verdict. A `--kind ledger` record can be
created and then checked `approved` at exit 0 on a failed or unestablished baseline, including
`violated`, `unknown_cti`, `reachable_failed`, `refinement_failed` and `unknown_budget`. Approving a red
ledger stays allowed (`docs/design/DESIGN-ledger.md`), so `fslc verify` must be gated
separately. An approval gate requires exit 0 AND `result:"approval_check"` AND
`status:"approved"`. `drifted` is not approval despite exit 0. A malformed,
unsupported, wrong-spec, or locally unavailable Git-baseline record is an error
and exits 2. A signed record without its matching trust key is also a
configuration error and exits 2. These errors have no approval status. Artifact evaluation
in `approval create` and `approval check` that aborts preserves its complete downstream envelope and exit: scenarios violations exit 1,
internal failures exit 3, and failed HTML reports retain `generated`/`html_report` and exit 1.
A verification `result:"error"` during `--kind ledger` rerendering returns that envelope and
its exit (2, or 3 for internal) in place of `drifted` or an artifact mismatch.
No approval record is written and no digest-based status is invented after such a failure.
An aborted scenarios artifact leaves root and dependency-warning finalization to
its outer approval/report owner. Each distinct root finding and each finding from
one distinct dependency resource is attached once, preserving complete values,
order and locations. Dependency-edition refusals retain priority over the underlying
artifact result. Successful scenarios artifacts retain their existing finalized root
and dependency warnings before canonical serialization, preserving existing artifact
digests and approval signatures.
`fslc ledger --approval` does not propagate that envelope. When an `html` or `scenarios`
record's rerender stops on a failed verification baseline, it returns `result:"error"`, exit 2,
naming the record, and leaves the `-o` file untouched: passing exit 1 through would read as the
ledger's own verdict while a stale file stayed in place. `ledger` and `requirements_document`
records still complete, so the ledger is written and shows them `drifted`.
Approval HTML rerendering uses no output path: inline content and default output metadata
remain in memory, without an HTML file. Direct `html -o` instead writes its report even for a
failed verification baseline; a write error exits 2 and outranks the baseline verdict.
Approval-only ledger rerendering still produces a digest/evaluation at exit 0 for those failed
or unestablished baselines, distinct from direct ledger's generated report and exit 1.
Unknown sidecar fields are rejected so schema
changes cannot be silently ignored.

When drift is reported, compare the approved commit to the current working tree
(including uncommitted edits) without manually materializing the baseline:

```bash
fslc approval diff examples/e2e/2_requirements.fsl --record expense.approval.json --depth 8
```

Pass the same repeatable `--trust-key` option when diffing a signed record.

The result is the ordinary bounded `semantic_diff` envelope plus `approval`
baseline metadata. Before comparison, the materialized specification is hashed
again and must match the record's baseline digest.

## Record contract

New sidecars use v5 (unsigned) or v6 (signed), with repository-relative path
context as specified below. The retained legacy unsigned solver sidecar follows
[`schemas/fslc/approval/approval-record.v1.schema.json`](../../schemas/fslc/approval/approval-record.v1.schema.json):

```json
{
  "schema": "fslc.approval.v1",
  "spec": {
    "path": "examples/e2e/2_requirements.fsl",
    "digest_algorithm": "fsl-kernel-ast-v1+sha256",
    "digest": "sha256:<64 hex>",
    "git_commit": "<full commit>"
  },
  "target": {
    "kind": "ledger",
    "path": "expense-ledger.md",
    "digest_algorithm": "fsl-rendered-artifact-v1+sha256",
    "digest": "sha256:<64 hex>",
    "generator": "fslc",
    "generator_version": "4.7.0",
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
These retained legacy contracts remain unchanged — a `requirements_document`
target is never admitted into v1/v2's closed `kind` enum, by design (see
below).

Legacy signed solver records use
[`schemas/fslc/approval/approval-record.v2.schema.json`](../../schemas/fslc/approval/approval-record.v2.schema.json).
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
[`schemas/fslc/approval/approval-record.v3.schema.json`](../../schemas/fslc/approval/approval-record.v3.schema.json)
(unsigned) and
[`schemas/fslc/approval/approval-record.v4.schema.json`](../../schemas/fslc/approval/approval-record.v4.schema.json)
(signed, v3 plus the same `signature` object v2 adds to v1) — rather than a
conditional branch inside v1/v2. `spec`/`approval` are byte-identical to v1's;
`target` differs in three ways:

```json
{
  "schema": "fslc.approval.v3",
  "spec": { "...": "identical shape to v1" },
  "target": {
    "kind": "requirements_document",
    "path": "expense-requirements.md",
    "digest_algorithm": "fsl-rendered-requirements-document-v1+sha256",
    "digest": "sha256:<64 hex>",
    "reviewed_digest_algorithm": "fsl-reviewed-requirements-document-v1+sha256",
    "reviewed_digest": "sha256:<64 hex>",
    "claim_set_digest_algorithm": "fsl-rcir-claim-set-v1+sha256",
    "claim_set_digest": "sha256:<64 hex>",
    "generator": "fslc",
    "generator_version": "4.7.0",
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

### Repository-relative path context (v5 unsigned, v6 signed; #1421 AP1/AP2)

All new `approval create` records use
[`approval-record.v5.schema.json`](../../schemas/fslc/approval/approval-record.v5.schema.json)
without a signing key and
[`approval-record.v6.schema.json`](../../schemas/fslc/approval/approval-record.v6.schema.json)
with one. Each admits the existing solver target shape (`ledger`, `html`,
`scenarios`) or document target shape (`requirements_document`) as separate
closed branches. The nested objects, document digests and input variants retain
their existing contracts. Two required top-level strings add the context:

- `path_base` is the invocation directory at creation, relative to the Git root
  of the checkout holding the spec: `.` for the root, otherwise `/`-separated names
  with no `.`, `..` or empty name and no control character. The form is text, so a
  record reads the same on every platform, and the schema pattern and the reader
  check the same form.
- `render_source` is the nonempty original spec argument, verbatim, used only
  as a rendering label. `spec.path` remains Git-repository-relative.

For example, creation from a directory containing `spec.fsl` with the argument
`./spec.fsl` preserves that exact label, a target such as `./requirements.md`,
and input spellings such as `glossary.json` or `nested/../evidence.json`.
Target/input I/O later uses `git_root / path_base / stored_path`, where `git_root`
is the root of the checkout holding the spec being checked, not the caller's
directory and not the creating checkout. A clone, a worktree or a relocated checkout
therefore reads its own files, and nothing reads the creating checkout, which may
be gone. The resolver does not probe caller/sidecar/repository alternatives.

The root is found from the directory the spec's path names, not from where a final
symlink leads, and with Git's location variables (`git rev-parse --local-env-vars`)
cleared for these lookups, so a nested repository behind a link or a hook's `GIT_DIR`
cannot choose the files. A spec resolving outside that root is refused with
`kind:"semantics"`, exit 2. `core.worktree` remains Git's own answer for the checkout,
so when it points away from the spec this refusal applies. A link to the spec inside the
same checkout is safe and stays allowed: the record's `spec.path` and digest are still
checked against the resolved file, and the document and inputs still come from that
checkout. A link into a nested repository or submodule resolves below the outer root,
so `approval check` and `ledger` refuse it on `spec.path`, and `document generate`, which
does not compare `spec.path`, reads the outer checkout's files and reports their drift.
`approval diff` keeps following the spec and reads no document. v1–v4 records keep
their existing lookup, including the inherited environment, and their `document generate`
route calls no Git. On v5/v6 that route needs Git for this lookup.

A document record stays inside that root. Creation refuses, with `kind:"semantics"`
and exit 2, a creation directory outside the root and a target or input that is
absolute, climbs out of the root by its spelling, or leads out through a link. Check
refuses a stored path or base that climbs or links out in the same way, before reading
it. An absolute form is rejected earlier by the record reader, as `kind:"io"` from
`approval check` and `ledger` and `kind:"document"` from `document generate`; the schema
rejects the same forms, and the climbing and link checks need the root, so they run
at use. The stored spelling of a target or input stays literal, including any `\` a
Windows caller typed; a record that must read across platforms uses `/`.

A solver record reads no file through its base, so it can be created anywhere: its
target path stays a label, and outside the root it stores `.`. Its `approved` means the
spec and a fresh rendering match the record. The reviewed file is read at creation and
not checked afterwards.

Tampering with `path_base` invalidates a v6 signature. A v5 record is unsigned and
proves nothing against someone editing it: a changed base pointing at a copy with the
same content reports `approved`, and a solver record reads nothing through the field.
Use v6 when the record itself needs protecting. Not bound, by decision: a repository's
identity, and `document generate`'s commit and `spec.path` on identical content. A spelled
`d/../x` fails on read in a clone that has no `d`, because Git stores no empty directories.

The current explicit spec still supplies the actual bytes and dependency
directory. Reproduction uses the recorded source and evidence labels; resolved
I/O strings never replace those labels in rendered content. Kernel source-label
admission is unchanged: labels containing `..` or absolute source labels remain
unsupported by current document generation. This context feature does not
broaden that admission rule.

Root migration-warning locations embedded in rendering copies also use the
recorded source label. Public diagnostic locations and dependency-file labels
retain their actual explicit paths; no recursive path substitution is applied.

v6 covers both new strings with
`b"fslc.approval.v6\0" || canonical_json(record_without_signature.value)`.
Its unsigned binding is v5 with both strings retained. v2→v1 and v4→v3 bindings
and their original signing payloads remain unchanged. v1–v4 readers retain
their closed wire shapes, reject added context even when relabelled, and resolve
their relative paths against the consuming invocation directory. No record is
silently migrated, resigned, or downgraded. Older binaries reject v5/v6 as
unsupported schemas through their existing I/O error envelope and exit 2;
obtaining context behavior requires a new reproduction, review and approval.

Authoritative approval check and ledger evaluation use contextual input I/O and,
for documents, contextual reviewed-target I/O. Solver checks reproduce the
canonical artifact from spec/inputs and never read the live target, while create
still reads it. Document generation with `--approval` remains lightweight:
only its reviewed-target read uses stored context; explicit glossary/evidence
arguments and its rendering remain invocation-relative. It performs no extra
rendering, stored-input reads or Git-baseline check. Document check remains
structural without trust verification or live reviewed-target admission;
approval diff remains signature/spec-baseline-only. Reason ordering, envelopes,
exit policies, artifact normalization and assurance are unchanged.

### Raw document-input admission (#1421 AP4–7)

Authoritative approval check and ledger evaluation compare every document
`FileInput.digest` with the raw bytes actually loaded for reproduction, including
legacy v3/v4 and contextual v5/v6 records. The glossary digest and each evidence
digest come from the same buffer that is parsed, without a second read. Evidence
hashes retain recorded order for mismatch selection; the existing sorted combined
digest and rendering bytes remain unchanged.

All existing record/trust/signature/spec/Git/input-read/parse/locale/rendering and
reviewed-target errors keep their precedence. First compute the complete existing
evaluation, with ordered reasons `spec_changed`, `rendering_changed`,
`renderer_changed`, `claim_set_changed`, `artifact_changed`. If it is drifted,
return the same complete result and exit policy even when an input digest differs.
Only an otherwise-approved evaluation with a raw mismatch becomes the existing
`result:error`, `kind:io`, exit 2, with message
`approval input digest mismatch: <recorded path>`. Select the glossary first,
then evidence in its recorded order, preserving the literal path in that message.

Direct drift still exits 0. Ledger retains record order and its verification and
publication exit rules. Document generate keeps its explicit invocation inputs
and lightweight reviewed-target admission; document check remains structural and
approval diff remains signature/spec-baseline-only. These consumers gain no new
stored-input reads, except that document generate resolves a contextual record's
reviewed target from the spec's Git root. Solver targets gain no input comparison.

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
Markdown is hashed as rendered; its renderer omits only each appendix finding's
top-level `cost` and `cache`, retaining proof evidence and same-named domain fields.
This preserves review-visible content while
preventing solver timing from producing false drift.
`approval create --kind html` regenerates the default timing-bearing report and normalizes timing and ambient solver-statistic values without removing their keys, so it rejects a `--no-timing` artifact as not matching a fresh rendering.

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

Issue #333 established the following document-target coverage; its historical
new-create v3/v4 assertions are superseded by #1421's v5/v6 creation contract:

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

Issue #1421 AP1/AP2 adds independent CLI controls for unsigned/signed document
creation at root/subdirectory, literal source/input labels and reviewed bytes,
cross-CWD checks with distinct valid decoys, missing actual inputs/targets without
fallback, every solver kind without live-target reads, closed v5/v6 schema
validation, required context, and v1–v4 rejection of added context fields.
`issue_1421_checkout_base` adds tampered and untampered clones and worktrees, a
relocated and a deleted creating checkout, refusal of paths outside the root at
creation and check, and schema/reader agreement on the stored forms.

Issue #1421 AP4–7 adds controls for otherwise-approved raw mismatches in v3–v6,
first-mismatch selection including reversed evidence order, all 31 combinations
of the five existing drift reasons, earlier read/parse/trust/signature failures,
editable-slot bindings, ledger record/exit order, and the lightweight generation,
structural check and diff boundaries. The rejecting baseline distinguishes the
raw-digest gap from the existing preservation controls.
