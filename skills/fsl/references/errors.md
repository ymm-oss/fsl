## Repair protocol (result → next move)

Machine-readable `faithfulness_class` tags are a quick routing layer over the
existing result/kind fields:

| faithfulness_class | Recommended action |
|---|---|
| `partial_op_unguarded` | Add the missing guard / run bounded Monitor (replay) |
| `frozen_only_invariant` | Run mutate to check kill-rate |
| `intent_unexercised` | Add a single-shot reachable for the action / raise `--depth` |
| `liveness_not_refined` | Re-prove liveness at each layer or add `preserve progress` to the refinement mapping |

| result / violation_kind | Meaning | Next move |
|---|---|---|
| `violated` / `invariant` | Counterexample found (trace is shortest) | Read the trace's `changes` and `violating_bindings`; add a guard or fix the invariant |
| `violated` / `trans` | Two-state safety counterexample found | Compare the trace's previous state with the violating step; decide between adding a guard, fixing the action, or fixing the trans |
| `violated` / `type_bound` | Bounded type out of range (automatic check) | Insufficient guard on `last_action`. Keep within range via `requires` (do not hand-write an invariant) |
| `violated` / `partial_op` | pop/head on an empty Seq, index out of range, or divisor 0 | Guard with `requires q.size() > 0` / `requires d != 0` or an `if` |
| `violated` / `ensures` | Postcondition not satisfied | Decide whether the body or the ensures is correct, and fix accordingly |
| `violated` / `leadsTo` | Response-property counterexample (lasso / stall) | Check the trace's `loop_start`. Either add `fair` to the action that drives progress, or fix the spec |
| `unknown_cti` / `leadsTo_rank` | Ranked response proof failed | Read `rank_failure`: `progress_action_not_fair` means `helpful` named a non-fair action; `helpful_action_not_enabled` means the matching progress action is blocked while P is pending; `non_decreasing_helpful_action` means the helpful action fires without lowering the measure; otherwise repair the rank or pending preservation |
| `reachable_failed` | A state you want to reach is unreachable | Read `action_coverage`'s `blocking_requires` (unsat core). Loosen a guard / add an action / increase `--depth` |
| `unknown_cti` | The invariant is true but not inductive | **The CTI's starting state = a phantom state satisfying all invariants. Add an auxiliary invariant (one that is a domain truth) that excludes it, then re-run.** Check `suggested_invariants` first — for the monotone-counter idiom the result carries ready-made candidate expressions. Track record: converges in one round (e.g. "no duplicates in the queue," "refunds only from Captured") |
| warning / `vacuous_implication` | The antecedent of an implication invariant is never reached within depth | Check whether an action / reachable witness that makes the antecedent hold is missing, or whether the antecedent expression is reversed or too strong relative to intent. Do not simply weaken the consequent |
| warning / `vacuous_leadsto` | The leadsTo trigger is not reached within depth | Check the action / guard / initial condition for entering the trigger state. Look first at whether P (not the response target Q) actually occurs in the spec |
| warning / `never_enabled_action` | No instance of an action is enabled through the checked depth | Inspect its `action_coverage` and blocking requires; add the missing enabling path or increase the bound. This is bounded evidence, so do not treat absence at K as permanent deadness. |
| warning / `always_true_requires` | Under the context of the preceding requires, this requires clause is not effective as a constraint | Decide whether the clause is redundant or whether a path to the state where the clause bites is missing. Do not delete it automatically |
| warning / `tautology_over_frozen` | An invariant that depends only on frozen variables no action ever assigns to, and is dynamically always true (a dead ghost = hollow) | Make the variable `const`, or suspect a missing action that should change it. A sign that the invariant "thinks it is checking a contract but checks nothing" |
| `error` / `parse` | Syntax error | Follow `loc` and `expected` (candidate tokens) |
| `error` / `type` | Type error | Follow the `hint`; Option equality requires compatible payload types, while ordering remains invalid |
| `error` / `semantics` | Double assignment, etc. | Do not assign to the same variable twice on the same path (an if's then/else are separate paths, so it is allowed) |
| `error` / `vacuous` | init is unsatisfiable (contradictory assignments, etc.) | Review init. Check that you are not giving one state variable contradictory values. A violation from an out-of-range value is different and becomes `violated`/`type_bound` |
| `refinement_failed` / `abs_requires_failed` | A detailed-layer transition breaks an upper-layer guard (e.g. a shortcut skipping approval) | Read `impl_action` and `impl_trace`. Add a guard to the detailed layer, or review the interpretation of the correspondence (`maps` / mapping) |
| `refinement_failed` / `abs_state_mismatch` / `stutter_changed_abs` / `map_out_of_bounds` | Mapping inconsistency (an update has no correspondence / a stutter nonetheless changes upper-layer state / a mapped value is out of the type's range) | Compare the `mismatch` path with `abs_before/after`. Fix the mapping expression or the action correspondence |
| `refinement_failed` / `progress_lost` | A `preserve progress` mapping pulled an upper `leadsTo` into the lower layer and found a lasso/stall | Read `progress_failure`, `impl_trace`, `pending_since`, `loop_start`/`stutter`, and the `progress.actions`. Add/restore lower-layer `fair action` on the progress action, add a lower-layer ranked `leadsTo`, or revise the progress mapping |
| `implements.result: refinement_failed` / `impl_violated` within check/verify | The requirements layer deviates from the upper (business) layer (`impl_violated` = the requirements spec breaks its own bounds/invariants, so no refinement verdict was reached) | **The top-level `result` is still `ok`/`verified` and the exit code is still 0 — read this field, never the exit code.** The contents of `implements.violation` have the same shape as refinement_failed. Same procedure as above + check the `requirement` on the requirements side |
| `error` / `acceptance` | Replay of an acceptance criterion failed | The ID and step of the failed AC are returned. Decide whether the procedure's precondition (state) or the expect is correct, and fix accordingly |
| `error` / `forbidden` | An operation sequence that should be rejected was accepted (under-constraint; the kind that a safety invariant stays silent about) | `accepted_trace` is the accepting path. The requires enabling the last operation is too loose → add a guard or review the spec |
| `error` / `forbidden_setup` | A precondition (non-final) step of the forbidden is not enabled (invalid trace) | Review the setup procedure. The non-final steps are there to reach that point and are not treated as success |
| `statistically_unsupported` / `dataset_invalid` / `evaluator_untrusted` / `insufficient_samples` (fsl-ai evidence commands) | External statistical/migration/drift evidence failed a gate — there is no kernel counterexample to read | Route by the status priority list in `docs/DESIGN-stochastic.md`: fix the evidence (records, calibration, sample size) or the component/rollout — not the spec, and do not expect a trace |

For an action whose coverage is `false`, `blocking_requires` pinpoints "which
requires is blocking it" on a per-clause basis, and `hint` summarizes the
blocking factors. Do not silently ignore it. For branches-split actions,
diagnostics keep the internal name (`submit__b1`) and add a human
`display_name` such as `submit[a <= AUTO_LIMIT]`.

Ordinary refinement still propagates safety, not liveness: safety refinement can
return `refines` while a lower-layer `leadsTo` fails. If the upper response must
be preserved at refine time, add to the mapping:

```fsl
preserve progress {
  respond EveryRequestHandled by answer, refuse, escalate
}
```

This checks the upper `leadsTo` after pulling it through the state mapping. A
failure is `refinement_failed / progress_lost`. The `by` actions are validated
impl action names and review metadata; they do not create fairness or prove
implementation conformance. The actual lasso exclusion still comes from
lower-layer `fair action` declarations.

When a counterexample makes you **change an interpretation** (added a guard,
loosened an invariant, decided how to handle an exception), record that judgment in
the assumption ledger (the `// ASSUME-n:` comments / tags in the `.fsl`) only after
the source material or the human confirms it. If the counterexample exposes a
missing requirement or design decision, ask instead of choosing the repair on the
user's behalf. The shortest path to verified is often "weakening the spec," so
without confirmation and a record of what was weakened and why, you later cannot
distinguish a hollowing-out repair from a legitimate fix. The mirror failure is
**over-constraining**: a guard added to fix a `forbidden`/`violated` can tighten
the action into a dead one. After such a fix, re-run `verify` and confirm the
repaired action's `action_coverage` is still `true` (and any affected `reachable`
still witnessed) — over-tightening surfaces as a *new* `reachable_failed` /
`covered:false`, not as a failure of the original fix.
