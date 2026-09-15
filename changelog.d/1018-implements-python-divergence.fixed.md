Fixed (#1018): documented the native-vs-frozen-Python divergence for inline
`implements` fail-closed folding into `check` / `verify` top-level verdicts
(#1002, PR #1026 `246f0987`). Native is authoritative; the frozen reference
retains the pre-folding `ok` / `verified` primary result with the seam under
nested `implements`.
