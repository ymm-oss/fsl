Fixed (#1251): `fslc refine`, `fslc verify`'s inline `implements`, and the
`fslc mutate` implements oracle stopped with `result:"error"` / `kind:"type"`
(exit 2) instead of a refinement verdict when the impl's domain differed from
the abstraction's: an action-correspondence argument outside the abstract
action's declared parameter domain (an impl parameter type wider than the
abstraction's: "parameter 'v' does not belong to its declared domain for
action 'push'"), or a state-mapping expression that reads an impl `Map` at a
key outside its finite key domain (for example an impl key type narrower than
the abstraction's: "map index outside finite key domain"). Both are now
`refinement_failed` / `kind:"map_out_of_bounds"` (exit 1) at the step or
`init` where they occur, the kind already documented for a shrunken impl
against a full-size abstract. The documented check order is kept: a false
abstract guard is still `abs_requires_failed`, and a stutter or action
mapping's own checks run before the escape is reported. A `Seq` index out of
range in a mapping, a `Map` read outside its domain in an
action-correspondence argument, and other abstract-guard evaluation errors
still stop with an error.
In `fslc mutate`, a `type_bound_*` mutant that widens or narrows such a type is
killed by `refinement` as before, and `--oracle-attribution` now lists
`refinement` among its `killers` (the attribution pass used to drop the error
and list none). Migration: a spec whose `refine`/`verify` exited 2 with one of
these messages now exits 1 with a located `map_out_of_bounds` finding; align
the impl's parameter or key type with the abstraction's, or map it explicitly.
