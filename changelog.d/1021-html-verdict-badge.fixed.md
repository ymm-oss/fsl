Fixed (#1021): `fslc html` drew a `verify` result outside a short hand list
as a neutral `info` badge, so `unknown_budget` and `error` looked harmless in
the report. The Result badge (hero summary and status section) now has an
explicit class for every verdict `verify` publishes: `verified` and `proved`
are `ok`; `violated`, `reachable_failed`, `refinement_failed`,
`impl_violated`, and `error` are `bad`; `unknown_cti` and `unknown_budget`
are `warn`. Any other value, or a missing `result`, is drawn as a `bad` badge
that reads `<value> (unrecognized verdict)` instead of a neutral one, so a
verdict added later without a class shows up in the report. A test in `fslc`
checks each listed verdict's class against `outcome_class`, the vocabulary's
owner, which `fsl-tools` cannot see. Coverage markers and witness `kind`
badges are unchanged. The `unknown_budget`, `error`, and unrecognized-value
badges now differ from the frozen Python reference, which draws them `info`;
this is intended. JSON output and exit codes are unchanged.
