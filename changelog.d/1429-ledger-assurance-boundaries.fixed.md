Fixed (#1429): `fslc ledger` and `fslc html` property assurance, and the
ledger's `保証限界` line, no longer over-claim for a verification envelope that
does not report a completed bounded check. A per-property class falls back to
`bounded` only under top-level `completeness: "bounded"`; a missing, unknown or
method/depth-only envelope now shows `not_run`. The guarantee line no longer
states BMC coverage of every execution for `not_run`, error, replay or
statistical input, and a bounded report without a valid depth says the depth
is unreported instead of printing `深さ null`. A rendering of a non-error
result that reports bounded or unbounded completeness and its depth is
unchanged; verdicts, raw findings, locations, exit codes and the
aggregate/external-evidence token (#994/#995) are unchanged. Migration: an
approval bound to a ledger or HTML rendering of an affected envelope fails its
existing drift check; regenerate and re-approve it.
