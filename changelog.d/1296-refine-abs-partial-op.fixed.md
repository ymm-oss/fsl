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
partial), governance preservations (`result:"refinement_failed"`, native and
Worker), and the `fslc mutate` implements oracle, which now kills a mutant
that reaches such a state (`killed_by:"refinement"`). A false abstract guard
is still `abs_requires_failed`, and the stutter, guard, and out-of-domain
checks keep their order. `abs_partial_op` is a new `kind` value; a consumer
that matches on the documented kind list should add it. Migration: a spec or
mapping that `refines` today may now fail with `abs_partial_op`; guard the
abstract action (`requires y != 0`) or make its body total, or keep the impl
from invoking it in that state. A partial operation in the abstract action's
post-state checks or `ensures`, and an abstract guard whose evaluation fails,
are unchanged.
