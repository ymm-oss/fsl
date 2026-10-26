Changed (#1446, #1341, #1342, #1343, #1447): **Breaking CLI change.** Native
argv acceptance is validated centrally against the embedded CLI contract.
Non-owned shared options on `explain`, `typestate`, `mutate`, `testgen`,
`html`, and `ledger`, non-owned `scenarios` options even at default values,
and unknown options or excess operands on `version` are usage errors
(exit 2), rather than silently accepted or interpreted by another command's
parser. Both `testgen` and `domain testgen` now publish the existing
`--allow-unwired` flag in their contract actions and help.
All choices are checked before source I/O, including previously deferred
checks on `analyze`, `causal analyze`, `db import`, `db check`, `ai check`,
`domain check`, `domain generate`, `domain testgen`, and `testgen`: invalid
values now use `kind: "usage"` instead of downstream `io` or `semantics`,
with the existing messages preserved. Unsupported `--kernel-version`
values on `kernel` and `conformance` likewise change from `semantics` to
`usage`, retaining the unsupported-public-Kernel-major message and exit 2.
Odd `refine` chain operands and malformed `diff --git` ranges change from
`io` to `usage`, preserving their messages, branch order, and exit 2;
nonexistent Git refs are unchanged. Unknown-option diagnostics now name
the real command and option on `scenarios`, `ai check`, `domain check`,
`domain expand`, and `db observe`, and identify dash-prefixed unknown
options on the `compat` group. Existing root/other-group wording,
`compat` no-argument/bare-unknown usage and `--include-ai` wording, and
terminal `ai compare` missing-value wording are preserved.
`ai compare` no longer accepts literal `--` as an option value, including labels
and dataset: it reports a missing-value usage error (exit 2) instead.
