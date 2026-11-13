Unified (#1870): a refinement walk's `fsl_runtime::RefinementVerdict` has one
projection onto its public result word, exit code and failure kind,
`fslc_rust::outcome::refine_outcome` (`RefineOutcome`). `fslc refine` (and
`chain`'s refine layer through it), inline `implements` in `check`/`verify`,
the Worker's governance preservations, `fslc diff` and the `mutate`
implements oracle read it instead of each matching the verdict and spelling
the word and exit by hand; a budget stop is `RefineOutcome::Inconclusive`,
which `mutate` keeps reporting as an inconclusive mutant. The failure arms
take their word and exit from the `Verdict` table, which gains no `refines`
entry. `fsl-runtime`'s `fsl-refine` driver keeps its own match, because
`fsl-runtime` cannot depend on `fslc-rust`. Command output and exit codes
are unchanged.
