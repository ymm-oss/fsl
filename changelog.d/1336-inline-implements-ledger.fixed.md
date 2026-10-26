Fixed (#1336): `fslc ledger` now shows its folded verification verdict and
inline `implements` seam (`abs`, `result`) in a new `## inline implements`
section after the header and approval comparison, before governance and the
risk lists. Passing seams are listed too; only `refines` is green. Previously
a failed seam exited 1 without its verdict or seam appearing in the body.
This section addition alone does not change ledgers without `implements`;
the separate sentinel-coverage correction and verdict findings (#1338) can
change them, including governance ledgers. Migration: an approved
inline-implements ledger's rendering digest changes, so `fslc approval check`
reports `drifted` (exit 0); review the section and re-approve the ledger.
