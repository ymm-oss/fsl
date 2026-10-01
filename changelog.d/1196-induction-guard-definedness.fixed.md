Fixed (#1196): `verify --engine induction` reported `proved` for a spec whose
guard reached a partial operation in a reachable state (e.g. `requires x > 0
and x / d < 100` after another action sets `d = 0`), because the step case
evaluated guards with a totalizing evaluator; BMC and the explicit engine fail
the same spec once the state is within `--depth`. Induction now adds a
definedness obligation over every state that satisfies the proved invariants:
action guards, enabled bodies, invariants, `leadsTo` expressions, `trans`
properties, and reached `ensures` must not reach a partial operation, with the
same short-circuit semantics as BMC (`d != 0 and x / d < 100` stays
`proved`). A failure is `unknown_cti` with `violation_kind: "partial_op"` and
`invariant: "_partial_<action>"` / `"_partial_property_<name>"`; when the state
is unreachable, an auxiliary invariant that excludes it (e.g. `d != 0`)
restores `proved`. The frozen Python reference still reports `proved`.
