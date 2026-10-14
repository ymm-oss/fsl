Fixed (#1303): a governance `preservation` whose refinement does not
`refines` now fails `fslc check` and `fslc verify`, fail-closed. The first
non-`refines` preservation in declaration order sets the top-level `result`
through the same mapping an inline `implements` seam uses —
`refinement_failed` → `refinement_failed`, `violated` (the `after` spec breaks
its own semantics) → `impl_violated`, `unknown_budget` → `unknown_budget`, all
exit 1; any other value is `error`/`internal` (exit 3) — while
`governance.preservations[].result` keeps each preservation's own value.
`fslc verify` now runs the preservations (at depth 8, like `check`) and
attaches the same `governance` section; it is folded outside the verify
cache, so a cached `verified` cannot hide a preservation broken later. The
same verdict reaches a `fslc chain` spec layer, `fslc ledger` / `fslc html` /
`fslc mutate` (exit 1 from the shared verify baseline), and the browser
Worker's `check` and `verify`. Before this, `check` reported `ok` and `verify`
reported `verified`, both exit 0, so a CI gating on the exit code passed a
broken preservation. Migration: this changes exit codes of existing runs —
`check`/`verify` of a governance document with a failing preservation (for
example `examples/refinement_liveness/governance_detects_safety_loss.fsl`)
now exits 1 instead of 0, and its top-level `result` is the preservation's
failure instead of `ok`/`verified`. Fix the preservation, or, to keep it
advisory, check it in a separate `fslc refine` step whose exit the CI
ignores. `fslc verify` of a governance document whose preservation cannot
be evaluated (a missing or ill-typed `before`/`after`/mapping file) now
reports the same located `error` as `check` (exit 2) instead of `verified`
(exit 0). Consumers that read `verify` envelopes of governance documents
should expect the new `governance` key.
