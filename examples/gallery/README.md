# FSL Example Gallery

This gallery is teaching material for "reading FSL small and trying it big." Each
`.fsl` file writes `expected-command` / `expected-result` / `expected-kind` in
comments, and `tests/test_gallery.py` matches them against the actual `fslc` JSON.

## valid: correct specs

| Size | File | Topic | One-liner | Command |
|---|---|---|---|---|
| tiny | `valid/tiny_turnstile.fsl` | Turnstile | Minimal state machine of `coin` and `push` | `fslc verify ... --engine induction` |
| tiny | `valid/tiny_traffic_light.fsl` | Traffic light | Basics of `enum` and nested `if` | `fslc verify ... --engine induction` |
| tiny | `valid/tiny_bounded_counter.fsl` | Bounded counter | Keep the bound with bounded types and `requires` | `fslc verify ... --engine induction` |
| tiny | `valid/tiny_option_structural_equality.fsl` | Option equality | Compare `none`/`some` structurally; use `is some` to bind | `fslc verify --depth 1 --deadlock ignore` |
| small | `valid/small_vending_machine.fsl` | Vending machine | `Map`, `Option`, stock decrement, `ensures` | `fslc verify --depth 6 --deadlock ignore` |
| small | `valid/small_elevator.fsl` | Single elevator | Consistency of floor, door, and target floor | `fslc verify ... --engine induction` |
| small | `valid/small_tcp_handshake.fsl` | TCP-like handshake | Entry point to `fair action` and `leadsTo` | `fslc verify --depth 6 --deadlock ignore` |
| medium | `valid/medium_dining_philosophers_deadlock_demo.fsl` | Philosophers | Observe a deadlock-shaped state with reachable | `fslc verify --depth 6 --deadlock warn` |
| medium | `valid/medium_two_phase_commit.fsl` | 2PC | Voting and commit safety | `fslc verify ... --engine induction` |
| large | `valid/large_order_workflow.fsl` | Order workflow | Order, shipment, return, ledger invariant | `fslc verify --depth 8 --deadlock ignore` |

## errors: learning from broken examples

Broken specs are teaching material for reading FSL's diagnostics. `result` is the
broad classification; `kind` / `violation_kind` are the classification of the
cause. At first, reading only `message`, `hint`, `trace`, and `invariant` in the
JSON is enough.

| kind | File | What the output looks like |
|---|---|---|
| `parse` | `errors/parse_missing_expression.fsl` | `{"result":"error","kind":"parse","expected":"one of: ..."}` |
| `type` | `errors/type_undeclared_type.fsl` | `unknown type 'UserId'` |
| `type` | `errors/type_struct_set_field.fsl` | `struct field ... has non-scalar type` |
| `semantics` | `errors/semantics_duplicate_assignment.fsl` | `double assignment to 'x' on the same execution path` |
| `vacuous` | `errors/vacuous_contradictory_init.fsl` | `init constraints are unsatisfiable` |
| `invariant` | `errors/violated_invariant_counter.fsl` | `{"result":"violated","violation_kind":"invariant"}` |
| `type_bound` | `errors/violated_type_bound_missing_guard.fsl` | `_bounds_stock` is broken |
| `ensures` | `errors/violated_ensures_wrong_postcondition.fsl` | the action name appears in the `invariant` field |
| `partial_op` | `errors/violated_partial_op_unchecked_pop.fsl` | `guard the action with requires q.size() > 0` |
| `leadsTo` | `errors/violated_leads_to_starvation.fsl` | a loop trace and a fairness hint appear |
| `deadlock` | `errors/violated_deadlock_terminal.fsl` | `violation_kind: deadlock` with `--deadlock error` |
| `refinement_failed` | `errors/refinement_failed_map.fsl` | expected is `abs_requires_failed`. Currently a DOGFOOD-6 bug candidate |
| `acceptance` | `errors/error_acceptance_false_expect.fsl` | returns the failure state of a false `expect` |

Detailed actual JSON excerpts are in `errors/README.md`.

## adversarial: examples aimed at fooling the verifier

`adversarial/` holds examples whose results are obvious to a human but that probe
the verifier's boundaries: deep `if`, a full `Seq.push`, an empty `Seq.head`,
`Option` + `struct` + `Set` + `Seq`, quantifier bounds, refinement-mapping
boundaries, placement of double assignments, and a `leadsTo` satisfied at the
same time point.

`adversarial/refine_mapping_boundary_map.fsl` is a regression example of the
"full-unfolding deadlock → vacuous `refines`" bug (`docs/DOGFOOD-6.md`). It is
resolved by incremental prefix unfolding, and now correctly returns
`refinement_failed/abs_state_mismatch` (detecting the mismatch between the post-bump
update result n=1 and α(n)=2 after a jump, before the boundary check). A residual
case with the same root cause (where a violating transition reaches the terminal
**only on some branches**) was closed by a fix that checks each prefix using only
the constraints up to step t (`design_bypasses_control` in
`examples/refinement_liveness/` is the regression example).

## How to read a broken example

1. Check what is expected from the `expected-*` comments at the top of the file.
2. Run the command in the comments and look at the JSON's `result` and `kind` / `violation_kind`.
3. If there is a `trace`, read what broke it from the last `action` and `changes`.
4. If the expectation and the actual output differ, do not fix the spec to match
   the observation; first record it as a bug candidate in `docs/DOGFOOD-6.md`.
