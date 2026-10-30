# Repair protocol — result → next move

First classify the failure and reconcile the source intent. Never invent a guard,
weaken a contract, shrink the reported world or discard a trace simply to get green.
Edition-specific codes and origin reading are in
[fsl-verify](../../fsl-verify/SKILL.md#8-diagnostics-of-files-headed-fsl-5).

| Result / kind | Meaning and next move |
|---|---|
| `error / usage` | Invalid argv; fix operands/options, not the spec. |
| `error / parse` | Follow loc/expected/hint. Invalid expression `||`/`->` uses `or`/`=>`; do not rewrite structural arrows. Range endpoints can retain generic FSL-PARSE. |
| `error / unsupported_in_edition` | Read every finding/replacement; use migrate plan, then approved safe edits. |
| `error / type` or `semantics` | Fix the named type, scope, ownership, method signature, duplicate write or unproved index distinctness. No ordinal coercions for enums. |
| `error / vacuous` | Init constraints unsatisfiable; distinguish contradictory writes from a reachable bound violation. |
| `violated / invariant` | Read requirement, shortest trace, changes, bindings and blamed conjuncts. Decide whether model or intended rule is wrong. |
| `violated / trans` | Compare pre/post state; old() refers to the pre-state. |
| `violated / type_bound` | Automatic bound failed; inspect last action/assignment and its guard. Do not restate the bound as a user invariant. |
| `violated / partial_op` | Empty Seq/head/pop, bad Seq index, zero divisor; add the source-required short-circuit guard or total body. |
| `violated / ensures` | Body/postcondition disagree; adjudicate intent before choosing which to fix. |
| `violated / leadsTo` | Read pending_since, loop_start or stall. A fair progress action may be absent, blocked or incorrectly modeled. |
| `reachable_failed` | Inspect unreached classification, action_coverage/blocking_requires. Insufficient depth differs from over-constraint; preserve the intended witness. |
| `unknown_cti` | Induction did not decide. CTI start may be unreachable, but is not known unreachable. Try deeper BMC/explicit closure or independently prove a domain-truth auxiliary invariant; suggested_invariants are candidates, never assumptions. |
| `unknown_budget` | Search cutoff, not a pass or counterexample. Report budget/state count; a smaller exploratory run is a smaller claim. |
| `unknown_cti / partial_op` | Invariant-satisfying CTI reaches undefined guard/body/property/ensures. Inspect `_partial_*` and last_action; guard a reachable failure or prove an auxiliary exclusion for an unreachable start. |
| `unknown_cti / key_domain` | Finite Map index escapes its keys. BMC/explicit reports an error when reached; guard index or prove exclusion. |
| `unknown_cti / ensures` | A step between invariant states falsifies ensures. Reproduce reachability before calling the start phantom. |
| `unknown_cti / leadsTo_rank` | Read rank_failure: lower bound, pending preservation, deadlock, nondecrease, nonfair/disabled helpful action, increase by another action or nonsticky helpful enabledness. Helpful names grant no fairness. |
| `proved` with `not_covered: ["int_overflow"]` | Step proof reasons over mathematical integers; only base checks i64 overflow. Keep verdict but report gap, bound values or explore large enough BMC/explicit. |
| `refinement_failed / abs_requires_failed` | Detail permits an abstractly forbidden operation. Inspect impl_action/trace, upper guard origin and correspondence. |
| `refinement_failed / abs_state_mismatch` | Initial/step mapped state disagrees or upper safety fails. Inspect mapped mismatch path and expected/actual states. |
| `stutter_changed_abs` | Claimed internal step changes observable abstract state. Repair detail/map or revise the claim with owner approval. |
| `map_out_of_bounds` | Mapped value out of range; review mapping/domain correspondence, not a silent bound widening. |
| `map_partial_op` | Action-correspondence argument undefined on a reachable call. Repair its expression/guard, not abstract behavior. |
| `abs_partial_op` | Abstract body undefined at a mapped enabled step. Abstract contract has no defined outcome there. |
| `abs_step_violation` | Abstract successor breaks bound/invariant/trans/ensures or its definedness; read abs_violation and abs_after_attempted. Runtime violation is not a guard rejection. |
| `progress_lost` | Pulled upper response fails; read progress_failure, lasso/stall and lower fairness. |
| Impl self-violation during refine | No valid correspondence judgment: verify/fix the impl on its own first; do not conflate it with mapping failure. Read the actual outer command envelope. |
| Non-refines inline implements | Outer failure mirrors seam, exit 1. Nested summary is not standalone refine's full replay trace; run refine for full evidence. |
| Non-refines governance preservation | First failure owns outer verdict; inspect every preservation and run its standalone refine if needed. |
| `error / acceptance` | Positive scenario step/check failed; read ID/failed_step. |
| `error / forbidden` | Final call accepted, violated, uncallable, wrong-kind or out of scope. Read accepted_step/accepted_trace, violation or message/input fields. Only an actual rejection succeeds; violation is not rejection. |
| `error / forbidden_setup` | Earlier setup rejected, so final operation was never tested. Fix the setup or interpretation. |
| `document_drifted` | Regenerate from source if spec moved; revert out-of-slot edits. Never hand-patch generated claims. |
| AI evidence failure | dataset_invalid → evaluator_untrusted → insufficient_samples → inconclusive → statistically_unsupported precedence. Fix evidence/calibration/sample scope, not kernel formulas. |

For ranked-response repair, use the [authoring recipe](syntax.md#ranked-response-recipe),
not just the rank_failure label: a state-dependent helpful argument can fail
induction despite check/BMC acceptance; per-entity interleaving needs a matching
helpful instance or a justified global rank. For expensive lasso/stall diagnosis,
follow [scoped liveness exploration](commands.md#scoped-liveness-exploration);
the smaller/selected run does not discharge full-world completion or seams.

## Warning and review channels

| Signal | Response |
|---|---|
| never_enabled_action | Inspect coverage and blocking clauses through K; not permanent deadness. |
| vacuous_implication / vacuous_leadsto | Confirm antecedent/trigger actually occurs; do not weaken consequent to silence it. |
| always_true_requires | Clause adds no constraint under preceding requires; ask whether redundant or mis-modeled. |
| tautology_over_frozen | Frozen ghost checks no evolving behavior; suspect missing update or use const if intended. |
| urgency_freeze / vacuous_deadline | Time frozen or age always zero; calibrate deadline boundary and no-urgent mutant. |
| vacuity_probe_truncated | Vacuity undecided, not established; report cutoff. |
| partial_op_unguarded faithfulness class | Read concrete failing operation; add confirmed guard/replay evidence. |
| frozen_only_invariant | Run mutate and inspect sensitivity, not just green verification. |
| intent_unexercised | Add confirmed reachable witness/increase depth. |
| liveness_not_refined | Verify owning layer or explicitly preserve progress. |

Coverage retains public names; branch-split internal names may also have a
display_name. Blame identifies causal trace contributors, not an approved repair.
For progress mappings use the partial clause:

```fsl-fragment
preserve progress { respond EveryRequestHandled by answer, refuse, escalate }
```

It pulls the upper property through the mapping; the list is review metadata,
not fairness or implementation proof. After a confirmed repair record the
judgment in source assumptions/tags and re-run check/lint/verify/induction/seam.
After tightening a guard, recheck action coverage and intended reachables:
over-constraint is a second failure mode even when the original invariant passes.
