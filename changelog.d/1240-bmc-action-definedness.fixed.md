Fixed (#1240): `fslc verify` (BMC, and the base case of `--engine induction`)
reported `verified` / `proved` with exit 0 for a spec whose reachable action
guard or body overflowed checked i64 arithmetic or read or wrote a finite `Map`
outside its key domain, whenever that action contained none of LANGUAGE.md
§6's partial operations and `init` was nondeterministic (a deterministic `init`
already failed through the concrete pre-pass). BMC now asks every action
instance's guard and enabled-body definedness at every step below `--depth`,
as it already did for invariants, `ensures`, and the other properties, and
reports the exit-2 `semantics` error an action with a partial operation already
produced (`action '<name>' guard evaluation has a non-partial failure`, or
`body`). Because actions are checked in declaration order, such an error in an
earlier action now precedes a later action's `partial_op` in the same step, as
in the explicit engine. Specs without such a failure keep their verdicts: none
of the 552 corpus specs changed verdict, exit code, or message under `bmc`,
`induction`, or `explicit`. Witness values (deadlock, reachable, and CTI
traces) may differ because the solver answers additional queries, and
`cost.properties` gains a `{"kind":"partial_op","name":"actions"}` row that
answers the all-defined case once per step, so per-action `partial_op` check
counts can drop (`docs/design/DESIGN-verification-cost.md`). The row also
counts the range-lemma queries that keep this check fast: each `Int` state
leaf's bound at a step is kept only when the solver proves it entailed, and is
conjoined only to the action definedness queries, whose answers it cannot
change. `fslc mutate` uses
the same check, so a mutant that removes an `init` assignment of an unbounded
`Int` can now be reported `killed_by: "build_spec"` (the definedness error)
instead of by an invariant. The induction step obligation (#1196) still
excludes non-partial undefinedness (`docs/design/DESIGN-induction.md` §2.6), so
an overflow or out-of-domain `Map` access first reachable beyond `--depth` is
still not reported by `--engine induction`; the frozen Python reference still
reports `verified`. Migration: a spec that now exits 2 reaches an overflow or
an out-of-domain `Map` access in the named action; guard the access (e.g.
`requires i <= 3 and m[i] == 0`) or bound the state that feeds it.
