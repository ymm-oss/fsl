Fixed (#1961): `verify --engine explicit` and `--engine auto` could report
`proved` for a spec with a reachable undefined `terminal` that BMC reports as
`violated`. In one BFS level, the first dead non-terminal state recorded the
deadlock and later dead states of that level skipped the terminal, so a
`partial_op` (`_partial_property_terminal`) on one of them was never
reported. Explicit now follows BMC's rule: at each level until a deadlock is
recorded, the terminal is evaluated on every dead state; a dead state whose
terminal reaches a partial operation is the violation, and it wins over a
deadlock on another state of the level; otherwise the deadlock is the first
dead non-terminal state in frontier order, as before. The same held for a
`reachable`: once one state of a level witnessed it, later states of that
level skipped it, so a partial operation in it there went unreported. A
`reachable` still unwitnessed when a level starts is now checked on every
state of the level, as BMC checks it over every state of a step. `mutate`
no longer certifies such a mutant clean through the explicit run. The
default engine and `--engine bmc` are unchanged.
