Fixed (#1080): `fslc sweep` no longer treats depth-limited-only reachability
cells as counterexamples. The public CLI contract is breaking: a grid made
entirely of such cells now returns `sweep_inconclusive` with exit 1 and a null
minimal counterexample, while grids with a determinate success and no true
failure return `sweep_passed` with exit 0.
Some grids that previously returned `sweep_failed`/exit 1 now return
`sweep_passed`/exit 0, and `minimal_counterexample` is null when no true failure
exists; consumers should gate on the exit code rather than
`result == "sweep_failed"`.
