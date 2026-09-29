Fixed (#1165): when a spec's `verify { }` block bounds several undeclared
`entity`/`number` names, `check` now reports the first offending bound in
source order, with its location, instead of a name that varied from run to
run. CLI `--instances`/`--values` overrides were already deterministic and
are unchanged.
