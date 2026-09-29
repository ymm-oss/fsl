# Local Referance semantic-drift audit

Status: accepted, opt-in local workflow (#709).

## Decision and authority

Referance is adopted only as a developer- or agent-invoked local workflow. Its primary surface is
the append-only Store: public-observation probes and FSL verification/freshness evidence are recorded
with provenance and inspected through `active-view`. CodeReferance is only an auxiliary read-only
static detector. Neither surface is a CI,
merge-readiness, product, promotion, or release gate. Its CodeReferance graph, probe records,
and Store entries are shadow/local evidence and never override this repository's authority order:

1. accepted language/design documents, native tests, and CI contracts;
2. the authoritative Rust Kernel, JSON, and process contracts;
3. frozen Python behavior only where an explicit compatibility contract remains;
4. Referance observations.

The go decision is deliberately narrow. Store-backed, content-addressed probes retain reproducible
public observations, and FSL bindings expose stale evidence. Exact symbol lookup adds an auxiliary
early reference-drift signal. Repo-wide or unclassified tension review is no-go: it is too noisy to
be a bug oracle and cannot establish behavior.

## Store-first workflow

For a new audit, create and verify a task-local Store first, then import the behavioral probe and FSL
verification, inspect `active-view`, and run the rejecting and freshness controls. Use the bounded
CodeReferance lens afterward to locate or disambiguate symbols during authority triage. Do not turn
the static audit's tension list into Store claims mechanically; a raw tension has neither behavioral
evidence nor repository authority.

## Auxiliary bounded CodeReferance slice

The maintained static profile is `tools/referance/domain-slice.toml`. It covers the accepted domain and
conformance designs, representative domain FSL, frozen compatibility code, authoritative Rust
syntax/lowering/tooling/tests, and versioned domain schemas. Invoke it explicitly:

```bash
referance code-mcp --root . --config tools/referance/domain-slice.toml
```

This command starts the auxiliary read-only code/doc lens, not the Referance Store workflow. Always
confirm `audit_summary.root` before using a result. Use `find_symbol` to locate a symbol,
then `resolve_symbol(..., arm="P")` on a unique bare or dotted name. A symbol match proves only
existence. In particular, `DomainTypeSourceForm.ValueObject` being indexed does not prove that
`DomainType.invariants` has executable semantics.

The 2026-08-04 pilot measured 5 documentation files, 43 source files, 968 definitions, and 849
code symbols. A cold scan took about 32 seconds; a cached `audit_summary` or `find_symbol` call took
about 0.36 seconds. The raw result contained 257 tensions: 197 unresolved-symbol candidates, 31
name-based `spec_impl_gap` candidates, 7 expected parse failures from invalid fixtures, and smaller
homonym/reference groups. Treat these as a queue for authority triage, not findings. Python/Rust
homonyms must remain distinct; qualify the symbol or stop.

## Differential probe

`tools/referance/domain-generate-probe.json` sends the same case bundle to the frozen Python and
native Rust `domain generate` commands. `tools/referance/domain-probe-adapter.py` validates an exact
case schema and retains the complete JSON stdout plus process exit code. It has no inclusion
allowlist, so a newly emitted envelope field participates automatically. The manifest binds the
selector, seed, case IDs, FSL inputs, both implementations, and the adapter by SHA-256.
`tests/test_referance_domain_manifest.py` pins the behavior owners and accepted contracts that must
remain in both manifests; its negative control removes one owner and requires a completeness failure.
It also injects an unknown envelope field and requires exact retention, calibrating against a shared
lossy stdout projection. The declared artifact set is a reviewed slice, not a complete transitive
dependency closure. Only isolated `--commit <SHA>` execution binds the whole tracked repository and
can become enforceable; working-tree runs are reference-only and record `enforceable=false`.

Use a verified task-local Store path. A mistyped path silently creates an empty Store, so create the
parent intentionally and confirm `active-view` before and after mutation:

```bash
REFERANCE_RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/fsl-referance-709.XXXXXX")"
STORE="$REFERANCE_RUN_DIR/semantic.db"
case "$STORE" in "$REFERANCE_RUN_DIR"/*) ;; *) exit 2 ;; esac
test ! -e "$STORE"
referance active-view --db "$STORE" # expected: {}
referance import-transpilation-probe --db "$STORE" \
  --manifest tools/referance/domain-generate-probe.json --root . \
  --timeout 240 --observed-at '<review timestamp>' \
  --commit '<reviewed commit SHA>' --by 'agent:<name>'
referance active-view --db "$STORE" # inspect the appended evidence and status
```

`pass` means only that this finite observation found no difference. It cannot authorize Rust
semantics or a cutover. `tools/referance/domain-generate-mismatch-control.json` changes only the
candidate exit-code observation and must return `fail`. An early pilot run used Referance's private
Python environment for the frozen package and produced a false mismatch; the corrected adapter uses
the repository Python boundary, and the bad record was superseded append-only rather than deleted.
Omit `--commit` only for an explicitly non-enforceable working-tree reference run. After review and
any required export/handoff, remove the validated temporary directory with
`rm -rf -- "$REFERANCE_RUN_DIR"`; never point this cleanup at a repository or unresolved path.

## FSL evidence freshness

Bind a representative verified FSL file to the Rust lowering paths and the comparison adapter:

```bash
referance import-fsl-verification --db "$STORE" \
  --path examples/domain/order_functional_ddd.fsl \
  --binding-id fsl:domain-lowering:freshness \
  --code-paths rust/fsl-core/src/domain_lowering.rs,rust/fsl-core/src/domain.rs,tools/referance/domain-probe-adapter.py \
  --engine bmc --depth 4 --deadlock warn --by 'agent:<name>'
referance check-fsl-drift --db "$STORE" --root .
```

`drifted` means that evidence is stale and must be re-verified; it does not mean the implementation
is semantically wrong. Never ground or promote an AI assertion. Correct Store mistakes with
`impact` followed by append-only `supersede` or `retract`, and notify owners of invalidated contracts.

## Calibration controls

The workflow is usable only while all of these controls retain their expected classification:

| Control | Accepting observation | Rejecting observation |
|---|---|---|
| static reference | a doc mention resolves to a Rust struct | renaming only the struct produces `unresolved_symbol` |
| behavioral probe | domain generation manifest returns `pass` | exit-code mutation manifest returns `fail` with the case observation |
| freshness | unchanged FSL/code binding is `current` | changing only a bound adapter copy is `drifted` / `code_changed` |
| intentional divergence | ordinary compatibility cases compare | `examples/annotations/annotated_domain.fsl` is classified native-only by `tests/dialect_registry.py`, not as a Rust bug |
| nested semantics | symbol/parser presence is insufficient | `issue_681_precedence_policy` native BMC and explicit controls reject a bypass |

The static fixture and stale mutation are created in a temporary directory and removed after the
run. Raw audit reports, Store databases, and timing logs are temporary evidence and must not be
committed.

## Authority-ordered triage

For every mismatch, first reproduce the raw observation, then inspect accepted docs/tests, then the
Rust typed/public contract, and only then frozen Python. Classify it as one of: compatibility breach,
accepted-but-hollow/absent Rust semantics, obsolete Python behavior, accepted Rust-only divergence,
detector/projection defect, or insufficient evidence.

The pilot classified the known rendering differences under #690 and #691, the annotation example as
intentional Rust-only divergence (#281), and opened #710–#713 for four independent accepted-but-hollow
domain findings. The lens also exposed a fail-closed detector limitation: re-feeding a Rust
`candidates[].id` containing a path does not resolve although bare/dotted names do. Do not widen or
guess around that result; use a unique bare/dotted term and retain the limitation as detector noise.

## Stop conditions

Stop or shrink the slice if cold scans cease to be bounded, exact Rust/Python namespaces cannot be
disambiguated, the adapter cannot retain a total public observation, raw noise is being treated as
authority, or the Store path/root cannot be verified. Findings are never auto-grounded, promoted,
or filed. A human or agent must complete authority triage before issue creation.
