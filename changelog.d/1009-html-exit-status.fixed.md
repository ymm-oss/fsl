Fixed (#1009)!: `fslc html` now exits according to the embedded verification
verdict instead of always returning 0 after generating the report. Pipelines
that gated only on the process exit code while ignoring `result` will see exit
1 for `violated`, `reachable_failed`, `unknown_cti`, `unknown_budget`,
`refinement_failed`, and `impl_violated`; successful verification still exits
0 and the HTML artifact is still written on failure.
