Native artifact output refuses a destination that is the same file as an input
the command reads, before anything is written: `fslc html s.fsl -o s.fsl` no
longer replaces the spec with the report. The spec, imported files and file
options such as `--evidence`, `--glossary`, `--approval` and `--artifact`
count, compared by file identity, so `./s.fsl`, a symlink and a hard link to
the input are refused too. The refusal is a `kind: "io"` error with exit 2, in
the envelope of the existing non-regular-target refusals; it covers `html`,
`ledger`, `testgen`, `counterexample export`, `document generate`/`claims`,
`db import`, `domain expand`/`generate`/`testgen`, `approval create` and
`causal observe-expectations` (#1440).
