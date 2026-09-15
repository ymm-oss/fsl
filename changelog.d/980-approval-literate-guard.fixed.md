Fixed (#980): `approval create`/`check`/`diff` now reject a `.md` positional
with `kind:"usage"`/`FSL-INPUT-LITERATE-UNSUPPORTED` (exit 2), the same
input-kind guard `check`/`verify`/`db`/`ai`/`causal`/`domain` already apply
(#694), instead of parsing it as an FSL spec and reporting a false
`FSL-PARSE` syntax error at `1:2`. `--kind requirements_document`'s
legitimately `.md`-shaped input is `--artifact`, never the positional, so
`--kind` does not change this. The guard runs immediately after the
positional is resolved and before `check`/`diff` read `--record`, so a
mismatched record no longer masks the same defect. `kind` and
`diagnostic_code` change for this previously-incorrect input; `result` and
exit code (2) do not.
