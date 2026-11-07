Fixed (#1429): `fslc ledger` and `fslc html` property assurance, and the
ledger's `保証限界` line, no longer over-claim for a verification envelope that
does not report a completed check. The header and every property row now read
completeness the same way (the top-level `completeness`, else the nested
`kernel.completeness` that `fslc domain check` and `fslc ai check` report). A
row is `bounded` only under reported `completeness: "bounded"` and `proved`
only under reported `completeness: "unbounded"`; a missing, unknown or
method/depth-only envelope, or `result: "proved"` without completeness, now
shows `not_run`. The guarantee line no longer states BMC coverage of every
execution for `not_run`, error, replay or statistical input, and a bounded
report without a valid depth says the depth is unreported instead of printing
`深さ null`. This includes a native explicit-engine `unknown_budget` result
(`completeness: "unknown"`) passed to the library renderers, which showed
`bounded(BMC depth N)` and BMC coverage before and now shows `not_run`;
`fslc ledger` and `fslc html` accept only `bmc` and `induction`. A rendering of
a non-error result with top-level bounded or unbounded completeness and its
depth is unchanged; verdicts, raw findings, locations, exit codes and the
aggregate/external-evidence token (#994/#995) are unchanged. Migration: an
approval bound to a ledger or HTML rendering of an affected envelope fails its
existing drift check; regenerate and re-approve it.
