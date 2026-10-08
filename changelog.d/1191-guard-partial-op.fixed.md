Fixed (#1191): a partial operation reached in an action's `requires` or `let`
(`requires s.head() == 0` or `let h = s.head()` on an empty sequence,
`requires 2 / x == 0` with `x == 0`) made `verify --engine explicit`, the
default `auto` engine, and `verify --engine bmc` on a deterministic init return
`result: error, kind: semantics` (exit 2) with the raw evaluation message. They
now report `violated` / `partial_op` / `_partial_<action>` (exit 1) with the
same replayable trace the symbolic engine already gave for a nondeterministic
init, as `docs/manual/LANGUAGE.md` §6 states. The `loc` of such a guard
`partial_op` is now, in every engine, the span the Public Kernel's
`partial_operations` gives that site: the `requires` clause, or the action for
a `let`. It used to be the action body's first statement, or `null` for an
action without a body (`rust/fslc/tests/fixtures/replay_trace.fsl`'s `partial`
now reports line 27). A `partial_op` in the body keeps its location. The concrete
action enumeration that explicit, Monitor BFS, the BMC/Worker boundary pre-pass,
refinement's self-violation walk, `scenarios`' cover and response walks, and
trace replay share no longer aborts on such a guard: it is that action's
`partial_op`, is not a deadlock, and is skipped by walks that only follow
successful steps. Commands that take a `verify` baseline follow it: `fslc
ledger` on such a spec now exits 1 and lists the `partial_op` row instead of
exiting 2 over a ledger that claimed nothing was detected
(`rust/fslc/tests/fixtures/replay_trace.fsl`). A spec whose guards never reach
a partial operation is unchanged.
