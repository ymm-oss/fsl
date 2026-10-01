Fixed (#1217): `verify --engine induction` reported `proved`/`unbounded` for a
spec whose action `ensures` is false on a reachable step beyond `--depth`
(e.g. `ensures x != 6` on a counter from 0 to 10, proved at `--depth 2`, while
BMC at depth 8 reports `violated ensures`): the step case never asked
`ensures`. Induction now checks every `ensures` as a one-step obligation over
states that satisfy the proved invariants (and `trans`), with BMC's notion of
a reached `ensures` (selected, enabled, body defined, earlier `ensures` defined
and true). A failure is `unknown_cti` with `violation_kind: "ensures"`,
`invariant: "<action>"`, and `last_action`; an `ensures` that follows from an
invariant stays `proved`, and an unreachable CTI start is excluded with an
auxiliary invariant. The frozen Python reference still reports `proved`.
