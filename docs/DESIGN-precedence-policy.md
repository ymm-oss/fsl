# FSL — business-layer no-bypass precedence policy (#75)

## Motivation

`business` deliberately keeps users from writing `state`/`invariant` directly — DESIGN-dialects.md
calls processes "pure stage graphs" and the whole point is that the layer stays in business
vocabulary (actors, entities, stages, transitions, policies, goals). But a very common control is
a **no-bypass precedence rule**: "a case that reaches Completed must have passed through Approved" —
Requested -> Refunded directly is the violation the control exists to catch. Before #75 this could
only be said by descending to `requirements` and hand-writing a kernel invariant plus the auxiliary
state to track it, which pushes the control out of the layer where it is owned and breaks REQ-ID
propagation back to the business policy that motivates it.

`policy <ID> "<text>" every <Entity> reaching <Stage> [or <Stage> ...] must have passed through
<Stage> [or <Stage> ...]` says the same thing in business vocabulary, closes the control at the
business layer, and propagates its REQ-ID through diagnostics exactly like the other policy forms
(`invariant`, `responds`, `every ... must eventually be ...`).

```fsl
policy CTRL-APPROVAL "承認を経ずに完了しない"
  every Return reaching Refunded must have passed through Approved
```

## Why this is not a purity violation

`_generate_business_items` (dialects.py) already synthesizes kernel state that the user never
writes: a stage enum, a `Map<Entity, Stage>`, `init` over all entities, and one fair action per
declared transition (DESIGN-dialects.md §3.2 rules 2 and, since #69, rule 7's sink-derived
`terminal { }`). "Business doesn't have state" is a constraint on the **user-facing surface**, not
on what the desugarer is allowed to build underneath it. A history flag is the same pattern one
layer further: it is invisible (the user never names it in their spec), structural (its value is
fully determined by which transitions fired, no user-supplied data or branching), and derived
entirely from declarations the user already wrote (the process's stages/transitions and the
policy's own waypoint list). It does not let the user encode arbitrary logic in business — it lets
the desugarer prove one more shape of property (safety over the stage graph's history, not just
its current value) without widening what the user is allowed to write.

## Desugaring

For each `biz_policy_precedence` body (`case_name`, `targets`, `waypoints`):

1. Resolve `case_name` to its process the same way `_stage_is`/`_any_stage` do
   (`_process_for_case`), and validate every target and waypoint stage belongs to that process.
   Both failures are type errors that name the policy's REQ-ID, e.g.
   `policy 'CTRL-APPROVAL': stage 'NoSuchStage' is not declared for process 'Return'`.
2. **Dedup.** Two (or more) precedence policies over the same `(process, waypoint-set)` share one
   history map — `visited`-style flags are pure functions of the process and the waypoint set, so
   there is nothing policy-specific to keep separate. The waypoint set is deduped and ordered by
   the process's own stage-declaration order (not written order, not alphabetical) so the
   synthesized name is deterministic regardless of how a given policy lists its waypoints:
   `<x_stage>_via_<Waypoint1>[_<Waypoint2>...]` (e.g. `return_stage_via_Approved`,
   `return_stage_via_Approved_Rejected`). This name is user-visible in traces by design — it should
   read as what it is, not as an opaque gensym.
3. **State.** One `history_var: Map<Entity, Bool>` per distinct history map, folded into the same
   `state { }` item rule 2 (DESIGN-dialects.md §3.2) already emits — not a second `state` block.
4. **Init.** `history_var[c] = true` for every entity if the process's `initial` stage is itself in
   the waypoint set, else `false` — folded into the same `init { }` item rule 2 emits. This is a
   deliberate, non-special-cased consequence of the rule: if a policy's target is also reachable
   directly from the initial stage without passing through any waypoint, and the initial stage is
   not itself a waypoint, the invariant is violated *at step 0* the moment the target is also the
   initial stage. That is a genuinely violated policy (the control never held), not a bug to route
   around.
5. **Transition injection.** Every synthesized transition action (rule 2) whose destination stage
   is in a history map's waypoint set gets `history_var[c] = true` appended to its body, before
   the `requires`/`set` shape for that action is finalized. A transition landing on a stage that is
   a waypoint for two distinct history maps (two policies, non-overlapping waypoint sets) appends
   both assigns.
6. **Property.** The policy compiles to a kernel invariant carrying the policy's own `meta` (id +
   text, so it propagates through `violated`/`unknown_cti`/coverage diagnostics exactly like the
   other policy forms):

   ```
   forall c: Entity { _any_stage(Entity, c, targets) => history_var[c] }
   ```

   `targets` disjunction reuses the same `_any_stage` helper the `every ... must eventually be
   ...` and `all <Entity> can be ...` forms already use.

## Ordering

Because the injection in step 5 mutates the *same* transition bodies rule 2 builds, precedence
policies must be collected (step 1–2, producing the history-var-per-waypoint-set table) **before**
the `state`/`init`/transition-emission loop runs, not interleaved with the policy-body emission
loop near the end of `_generate_business_items` (which is where the invariant itself, step 6, is
appended — that part runs where every other policy body is already handled, after `state`/`init`
and the transition loop, so it composes with #69's terminal-from-sinks derivation which runs
right after the transition loop and needs no changes).

## Semantics notes (not "fixed", documented)

- **Initial stage inside targets, not inside waypoints**: violated at init. See step 4 above.
- **Waypoint == target**: allowed. Arriving at a stage that is both a waypoint and a target
  satisfies the policy trivially — the transition landing there sets the flag before the invariant
  is (conceptually) re-checked, so `history_var[c]` is `true` in the very state where
  `_any_stage(targets)` first becomes true.

## Refinement limitation

The history flag exists only in the business-layer kernel spec that `expand_business` produces —
it is invisible synthesized state, not something a `requirements` spec written independently knows
about. A `requirements` spec that `implements`/refines a business spec carrying a precedence policy
must either (a) explicitly map the history flag in its refinement mapping file, or (b) restate the
no-bypass rule at its own layer (there is currently no `requirements`-layer precedence syntax; see
issue #75's alternatives section). Refinement checking does not currently propagate synthesized
business-layer state automatically. This is a known limitation, not a defect — it mirrors the
general rule that refinement mappings are explicit, not inferred.

## What did not change

`bmc.py` / `runtime.py`: nothing. The desugared output is an ordinary `Map<Entity, Bool>` state
variable, ordinary transition-body assigns, and an ordinary `forall`-wrapped invariant — all
existing kernel constructs. Both evaluators (`bmc.py` and `runtime.py`'s `Monitor`) already handle
these; `tests/test_precedence_policy.py` exercises BMC (`fslc verify`) and AST-level assertions on
the desugared kernel spec, and the corpus snapshot (`tests/test_corpus_snapshot.py`) is unaffected
since no existing spec uses the new syntax.
