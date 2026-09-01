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
