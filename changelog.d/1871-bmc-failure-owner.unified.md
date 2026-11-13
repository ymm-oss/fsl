Unified (#1871): whether a bounded run's `BmcResult` fails, and on what,
has one owner, `fslc_rust::verification_output::first_bmc_failure`
(`BmcRunFailure`), in `verify`'s order: violation, unreached reachable,
deadlock, leadsTo. Its arguments state what each consumer counts: a
`DeadlockMode` (a deadlock fails only under `Error`) and
`UnreachedReachables`. `render_bmc_output` dispatches on it; `scenarios`
(its `--deadlock` parsed once), the `explain`/`html` counterfactual runs
and the `mutate` oracle call it instead of each testing the fields by hand,
`mutate` and the counterfactual runs still not counting a deadlock. `html`
and `ledger` build their depth, deadlock mode and engine once
(`DerivedRunOptions`) and pass it to every stage; the deadlock mode of the
witness stage is a field of it, which `html`'s explain stage keeps at
`warn`, as `explain` does, and `ledger` sets to the requested mode. Command
output and exit codes are unchanged.
