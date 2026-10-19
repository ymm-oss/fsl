Changed (#1315, breaking): the implementation side violating its own
contract has one public word, `impl_violated`, wherever it is reported.
`fslc refine` on an impl spec that breaks its own type bound, invariant,
`trans`, or `ensures` within `--depth` now reports top-level
`result: "impl_violated"` (was `"violated"`), and a governance
`preservation` whose `after` spec does the same reports
`governance.preservations[].result: "impl_violated"` (was `"violated"`) from
`check`, `verify`, `scenarios`, `ledger`, and the browser Worker alike. The
inline `implements` seam and the folded top-level verdict already used
`impl_violated`, so the nested and top-level values now agree. The evidence
fields (`violation_kind`, `invariant`, `violated_at_step`, `impl_trace`,
`note`) and every exit code are unchanged, and `fslc chain`'s own run verdict
for a failing layer stays `violated`. Migration: a consumer that matches
`"violated"` on `fslc refine`'s `result`, on a `chain` refine layer's
`result`/`detail.result`, or on `governance.preservations[].result` must match
`"impl_violated"` instead; `"violated"` from `fslc verify` keeps its meaning
(a counterexample of the verified spec itself).
