# No-user-invariants warning suppression (#961)

## Decision

The model-level warning `spec declares no user invariants (only implicit type
bounds are checked)` (`kind: "no_user_invariants"`) is emitted when a checked
kernel has no `invariant` or `trans` properties. It is suppressed only when
the spec carries safety-bearing declarations:

- `invariant` or `trans` on the kernel model, or
- `forbidden` on a requirements trace contract (source-only), or
- `implements` on a requirements document (source-only).

It is **not** suppressed by liveness or witness declarations alone:

- `reachable`, `leadsTo`, `until`/`unless` liveness halves, or
- `acceptance` trace cases.

This supersedes the behavior recorded in `CHANGELOG.md` at the v1.5.0 entry
(lines 5109–5111), which treated `reachable`/`leadsTo`/`acceptance`/`forbidden`
/`implements` uniformly.

## Authority surface

- **Emit (candidate):** `fsl-core::model_warnings` adds the warning when
  `invariants` and `transitions` are both empty. `reachable` and `leadsTo`
  no longer gate emission.
- **Finalize (single source of truth):**
  `fsl-core::finalize_model_warnings` / `finalize_envelope_model_warnings`
  apply the suppression rule from `ModelWarningContext` (`KernelModel` plus
  `has_forbidden` / `has_implements` parsed from source).
- **Callers:** native CLI (`check`, `verify`), Worker (`check`, `verify`).
  Frontends must not classify this warning by message string.

`has_trace_contract` (acceptance **or** forbidden present) must not be used
as a suppression predicate; acceptance-only specs must retain the warning.

## Regression

`rust/fslc/tests/issue_961_no_user_invariants_warning.rs` pins:

- emission for declaration-free and reachable-only kernel specs, and
- suppression for invariant, trans, forbidden, and implements fixtures.

## Frozen Python reference divergence

Native Rust and the frozen Python compatibility reference disagree on when
this warning is suppressed.

| Implementation | Suppresses when present |
|---|---|
| Native Rust (`fsl-core::finalize_model_warnings`, #961) | `invariants`, `transitions`, `forbidden`, `implements` |
| Frozen Python (`src/fslc/model.py:1689-1696`) | `invariants`, `leadstos`, `reachables`, `transitions`, `acceptance`, `forbidden`, `implements` |

Concrete example: `specs/cart_v1.fsl` declares only `reachable SoldOut`. After
#961, native `fslc check` emits `no_user_invariants`; frozen Python `fslc check`
does not (because `reachables` is non-empty in `model.py:1692`).

### Why this is allowed

`CLAUDE.md` names the native Rust workspace as authoritative and `src/fslc/` as
a **frozen** compatibility/LSP surface: new product behavior lands in Rust
first; the frozen Python reference moves only when an explicit compatibility
decision requires both implementations to move.

`tests/agreement.py` exercises symbolic/concrete expression agreement
(`bmc.eval_expr` vs the Monitor). It does not compare CLI `warnings` envelopes
between native and frozen Python. No existing parity gate therefore fails closed
on this diagnostic difference.

`tests/snapshots/corpus_snapshot.json` is likewise pinned to frozen Python
(`tests/test_corpus_snapshot.py` imports `run_check` / `run_verify` from
`fslc.cli`), so a Rust-only change to this warning does not move that snapshot.
That absence of movement is expected, not a missed regeneration.

### Permanence

This divergence is **intentional for #961**: narrowing suppression is a native
product correction; the frozen reference retains the broader pre-#961 predicate
until a separate compatibility decision says otherwise.

Aligning Python with Rust is a **follow-up compatibility task**, not part of
#961. Whether to open that follow-up is a maintainer decision; this design note
does not authorize or schedule it.
