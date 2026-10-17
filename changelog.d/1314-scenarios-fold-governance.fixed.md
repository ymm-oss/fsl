Fixed (#1314): `fslc scenarios` now folds governance `preservation`
results and an inline `implements` seam into its envelope exactly as `fslc
verify` does, through the same mapping (#1303). Before this it answered from
its own bounded run without reading either one, so a document whose
preservation or seam failed got `result:"scenarios"` / exit 0; only when its
own kernel failed did it fall back to the `verify` envelope, which carried
them. A governance document never takes that fallback, so its preservations
were never read. Now a failing preservation or seam makes the top-level
`result` the folded verdict (`refinement_failed`, `impl_violated`, or
`unknown_budget`, exit 1), every run attaches the `governance` / `implements`
section `verify` attaches (also when everything `refines`), and a governance
contract error or inline `implements` error that `verify` reports is the
same located `error` (exit 2). `fslc approval create --kind scenarios` reads
the same envelope. Migration: this changes `scenarios` exit codes and output
for governance documents and requirements documents with an inline
`implements`, and nothing else. In this repository's corpus, exit 0 → 1:
`examples/refinement_liveness/governance_detects_safety_loss.fsl`,
`tests/fixtures/chain/requirements_broken_implements.fsl`, and four test
fixtures; exit 0 → 2: the three governance error examples under
`examples/gallery/` and two test fixtures; every other governance or
inline-`implements` document stays exit 0 and gains the section. Fix the
preservation or seam, or, to keep it advisory, run it as a separate `fslc
refine` step. A `scenarios` approval of such a document no longer matches its
recorded artifact digest (`fslc approval check` reports `drifted`, exit 0);
re-approve it. `testgen`, `explain`, `testplan`, `diff`, `analyze`, and
`kernel` are unchanged.
