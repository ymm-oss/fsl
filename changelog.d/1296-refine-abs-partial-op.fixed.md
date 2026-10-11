Fixed (#1296): a refinement whose corresponded abstract action hits a partial
operation in its own body at the mapped state (`action go() { y = 4 / y }`
reached at `y = 0`, or `s.pop()` on an empty `Seq`) was reported as `refines`
(exit 0) whenever the impl step left the mapped state unchanged: the abstract
step's `partial_op` was recorded and then ignored. It is now
`refinement_failed` / `kind:"abs_partial_op"` (exit 1) at that step, with
`abs_before` / `abs_after_actual` and no `abs_after_expected` (the abstraction
has no defined successor there). Every consumer of the refinement verdict
reports it: `fslc refine`, `fslc check` / `fslc verify` inline `implements`
(`implements.violation.kind`), `fslc diff` (the direction whose abstraction is
partial), governance preservations (native and Worker: only
`governance.preservations[].result` becomes `"refinement_failed"`; the
top-level `result` and the exit code of `fslc check` on the governance file
are unchanged, as for any other failed preservation), and the `fslc mutate` implements oracle, which now kills a mutant
that reaches such a state (`killed_by:"refinement"`). A false abstract guard
is still `abs_requires_failed`, and the stutter, guard, and out-of-domain
checks keep their order. `abs_partial_op` is a new `kind` value; a consumer
that matches on the documented kind list should add it. Migration: a spec or
mapping that `refines` today may now fail with `abs_partial_op`; guard the
abstract action (`requires y != 0`) or make its body total, or keep the impl
from invoking it in that state. The other violations
the abstract step reports -- an abstract `ensures` that is false, a partial
operation inside an abstract `ensures` or post-state check, a post-state
type-bound violation, and an abstract invariant (or `trans`) violation -- are
not `abs_partial_op` but `abs_step_violation` (#1302). An abstract guard
or leading `let` whose evaluation fails (for example `let q = 4 / y`) still
stops with `result:"error"` (exit 2).
