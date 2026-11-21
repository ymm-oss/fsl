Fixed (#1961): `verify --engine explicit` and `--engine auto` could report
`proved` or `verified` for a spec with a reachable undefined `terminal` that
BMC reports as `violated`. In one BFS level, the first dead non-terminal
state recorded the deadlock and later dead states of that level skipped the
terminal; and at the depth a state whose only action had a guard reaching a
partial Seq read was never dead, so its terminal was never evaluated even
when the read could make the guard false. Explicit now follows BMC's order
at one step: first the action partial operations of the level's steps
(guards, bodies, `ensures`); then, until a deadlock is recorded, the
terminal of every dead state of the level, where the first partial
operation is the violation and wins over a deadlock on another state, and
otherwise the first dead non-terminal state in frontier order is the
deadlock; then the other violations of the level's steps. Below the depth a
state is dead when it has no candidate. At the depth a state whose only
candidates are partial guards is dead unless one guard is shown true for
every value BMC's deadlock query can choose (division by zero totalized, a
partial Seq read unknown); where that reading cannot show a guard true for
every value, explicit reports and BMC may not. A held terminal failure is
not replaced by a state-budget stop. The same held for a `reachable`: once
one state of a level witnessed it, later states of that level skipped it,
so a partial operation in it there went unreported; it is now checked on
every state of the level, as BMC checks it over every state of a step.
Explicit can now report an action `partial_op` where it used to report an
earlier violation of another kind in the same level. `mutate` no longer
certifies such a mutant clean through the explicit run. The default engine
and `--engine bmc` are unchanged.
