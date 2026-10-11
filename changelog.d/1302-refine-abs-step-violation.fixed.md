Fixed (#1302): a refinement whose corresponded abstract step is rejected by
the abstraction itself — the successor violates an abstract type bound
(`action go() { y = y + 5 }` with `y: 0..4`), an abstract invariant or
`trans`, the abstract action's `ensures` is false, or a post-state check or
`ensures` hits a partial operation (`ensures 4 / y == 0` at `y = 0`) — was
reported as `refines` (exit 0) whenever the impl step left the mapped state
unchanged, and as `abs_state_mismatch` with the rolled-back pre-state shown as
`abs_after_expected` when it changed it. It is now `refinement_failed` /
`kind:"abs_step_violation"` (exit 1) at that step, with `abs_violation`
(`{"kind", "name"}` of the abstract violation: `type_bound` / `invariant` /
`trans` / `ensures` / `partial_op`), `abs_after_attempted` (the successor the
abstraction rejected), and no `abs_after_expected`. Every consumer of the
refinement verdict reports it: `fslc refine`, `fslc check` / `fslc verify`
inline `implements` (`implements.violation.kind`), `fslc diff` (the failing
direction, with `abs_violation`), governance preservations
(`result:"refinement_failed"`, native and Worker), and the `fslc mutate`
implements oracle, which now kills a mutant that reaches such a step
(`killed_by:"refinement"`). The `refines` note for an abstraction with
`ensures` now reads "abs ensures are checked only on the abstract steps the
impl's transitions correspond to within checked_to_depth; verify/prove the
abstract spec separately". `abs_step_violation`, `abs_violation` and
`abs_after_attempted` are new; a consumer that matches on the documented kind
list or on the note text should add or update them. Migration: a refinement
that `refines` today may now fail with `abs_step_violation`; the abstract-side
conditions now checked on every corresponded step are the abstract type
bounds, invariants and `trans` at the abstract successor, and the abstract
action's `ensures` (including partial operations in any of them). Fix the
abstract action (or its `ensures` / invariant) or keep the impl from invoking
it in that state; a refinement that failed with `abs_state_mismatch` for this
reason now reports `abs_step_violation` instead. A false abstract guard is
still `abs_requires_failed`, a body partial operation still `abs_partial_op`,
and an abstract guard whose evaluation fails is unchanged.
