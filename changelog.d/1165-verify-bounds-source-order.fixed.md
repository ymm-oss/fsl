Fixed (#1165): when a spec's `verify { }` block bounds several undeclared
`entity`/`number` names, `check` now reports the first offending bound in
source order, with its location, instead of a name that varied from run to
run. CLI `--instances`/`--values` overrides were already deterministic and
are unchanged.
Source order now also applies across the two kinds: an undeclared `values`
bound written before an undeclared `instances` bound is reported first. The
frozen Python reference (`src/fslc/dialects.py`) still reports every
`instances` bound before any `values` bound, so the two differ when both
kinds are undeclared.
