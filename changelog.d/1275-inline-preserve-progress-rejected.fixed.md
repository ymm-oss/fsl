Fixed (#1275): `preserve progress` inside an inline `implements { }` block is
now rejected instead of being accepted and silently unchecked. The inline
seam checks safety refinement only, so `check` / `verify` used to report
`implements.result: "refines"` (exit 0) for a mapping whose progress the
standalone `fslc refine` reports as `progress_lost`. Every command that loads
the document (`check`, `verify`, `scenarios`, `chain`, the LSP, the WASM
Worker) now fails with a located `kind: "parse"` error (exit 2) at the
`preserve` keyword. Migration: a requirements spec with `preserve progress`
in its `implements` block now exits 2; delete that item from the block, put
the mapping and its `preserve progress` in a standalone `refinement` file,
and run `fslc refine <impl> <abs> <refinement>` to check the upper `leadsTo`.
Inline `implements` blocks without `preserve progress` are unchanged.
