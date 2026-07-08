# FSL k-Induction Engine — Implementation Design (v1.1, elaboration of DESIGN-v1.md §9)

This document is an implementation-level specification of `--engine induction`.
The protocol of §9 (`proved` / `unknown_cti` / JSON shape) is finalized; here we
specify the semantics, the algorithm, the integration into existing code, and
edge cases.

## 1. Goals and non-goals

- **Goal**: **unbounded-depth proof** of invariants (user-defined + automatic
  `_bounds_*`) and, when a `leadsTo` declares `decreases <expr>`, an unbounded
  ranking proof of that response property. On success, `result: "proved"`.
  Invariants promote BMC's "no violation up to depth K" to "holds in every
  reachable state"; ranked `leadsTo` promotes the bounded response check to a
  well-founded progress proof.
- **Non-goals (not handled in v1.1)**:
  - Proof of `reachable` (`reachable` may remain a bounded witness search;
    induction also searches for a witness the same way as BMC — depth reuses `--depth`)
  - Unbounded proof of unranked `leadsTo` (it remains a bounded lasso/stutter
    check attached as `leads_to.<name>.checked_to_depth`)
  - Inductive proof of `ensures` (ensures is a single-transition property, so
    induction is unnecessary — see §5)
  - IC3/PDR (no automatic strengthening from CTIs. Returning the CTI to the LLM
    is the v1.1 bet)

## 2. Algorithm

Inputs: spec, maximum induction depth `K_ind` (CLI `--k`, default 1, upper bound
around 4), BMC depth `K_bmc` (`--depth`, used for the base case and the
reachable witness).

Inv(s) := the conjunction of all invariants (user + `_bounds_*`).
T(s, s') := the same transition relation as the existing `transition()` (including choice variables).
Init(s) := the existing `init_constraints()`.

### 2.1 Base case

Run the existing BMC at depth `K_bmc` as-is (code reuse). If violated, return the
usual violated JSON and stop (the counterexample is a real trace, and returning
it here is best).

Note: the base of textbook k-induction is "up to depth k-1," but in FSL we take
**base = ordinary BMC (depth K_bmc ≥ k)**. The deeper the base is run, the more
false CTIs (violations that are actually reachable) can be detected first as
violated with a real trace, improving the quality of the response to the LLM.

### 2.2 Step case

Try k = 1, 2, ..., K_ind in order. For each k, decide **per invariant** (not the
whole conjunction at once, but individually; reason: to identify and return in
JSON which invariant is not inductive):

```
variables: free state sequence σ_0 .. σ_k (no init constraint)
constraints:
  ∀ t ∈ [0, k-1]:  Inv(σ_t)            // all invariants hold in the past k states
  ∀ t ∈ [0, k-1]:  T(σ_t, σ_{t+1})     // consecutive transitions
  ¬ inv_i(σ_k)                          // the target invariant breaks at state k
```

- **unsat** → inv_i is k-inductive. Move to the next invariant.
- **sat** → extract the CTI from the model (§3). If k < K_ind, retry at k+1.
  If still sat at k = K_ind, return `unknown_cti`.

If all invariants become unsat (each at some k ≤ K_ind), then `proved`.

Important: the premise Inv(σ_t) of the per-invariant decision may assume the
**conjunction of all invariants** (mutual induction; standard and sound — each is
proven under a stronger premise than simultaneous induction of all).

### 2.3 Ranked leadsTo step case

After the invariant step case succeeds, each `leadsTo L { P ~> Q decreases M }`
is checked per outer binding against the invariant abstraction:

```
Inv(s) ∧ P(s) ∧ ¬Q(s)              ⇒ M(s) >= 0
Inv(s) ∧ P(s) ∧ ¬Q(s)              ⇒ enabled(s)
Inv(s) ∧ P(s) ∧ ¬Q(s) ∧ T_a(s,s') ⇒ Q(s') ∨ (P(s') ∧ M(s') < M(s))
```

`M` must be integer-valued. The `P(s')` part is required for soundness: a
ranking argument only proves the response while the pending obligation remains
inside the ranked region. Fairness is not consulted here; every enabled action
must make progress or establish `Q`.

**With one or more `helpful action(args)` lines**, the third obligation is
relaxed for non-matching actions, and two extra obligations are added to
compensate (`bmc.py` `_prove_leadsto_rank_helpful_fairness`,
`_prove_leadsto_rank_helpful_sticky`, `_prove_leadsto_rank_no_deadlock`,
`_prove_leadsto_rank_progress`):

```
// every matching helpful instance i must be a fair action           (helpful_fairness)
Inv(s) ∧ P(s) ∧ ¬Q(s)                          ⇒ ∃ i. enabled_i(s)     (no_deadlock: disjunction)
Inv(s) ∧ P(s) ∧ ¬Q(s) ∧ enabled_i(s) ∧ T_a(s,s'), a ≠ i, ¬Q(s')
                                                ⇒ enabled_i(s')         (helpful_sticky, i indexed)
Inv(s) ∧ P(s) ∧ ¬Q(s) ∧ T_i(s,s')              ⇒ Q(s') ∨ (P(s') ∧ M(s') < M(s))
Inv(s) ∧ P(s) ∧ ¬Q(s) ∧ T_a(s,s'), a ∉ helpful ⇒ Q(s') ∨ (P(s') ∧ M(s') ≤ M(s))
```

The disjunctive `no_deadlock` obligation ("some helpful instance is enabled")
is not by itself enough to invoke any single instance's fairness: with two or
more helpful instances, *which* one is enabled can differ from state to
state, so no single instance is necessarily *continuously* enabled and its
`fair` declaration is never obligated to fire. `helpful_sticky` closes this
gap by requiring that once instance `i` becomes enabled while pending, no
other action can disable it again before it fires (or `Q` holds) -- only then
does `no_deadlock` + `fair` actually license weak fairness for that instance.
(With a single helpful instance, `helpful_sticky` is vacuous: `no_deadlock`
alone already proves it enabled at every pending state, i.e. continuously
enabled while pending.)

The last obligation is the other half: a non-helpful action must not
*increase* the measure either, only the helpful instance's own transition
must strictly decrease it. Without this bound, an unrelated action could pump
the measure up by more than the helpful action brings it down each time it
fires, so `Q` would never be reached even though the helpful action keeps
firing under fairness.

Failure of any of these is `unknown_cti` / `rank_failure`:
`progress_action_not_fair`, `helpful_action_not_enabled`,
`helpful_action_enabledness_not_sticky`, `non_decreasing_helpful_action`, or
`non_helpful_action_increases_measure` (§2.6 of `DESIGN-temporal.md`, §10 of
`LANGUAGE.md`).

### 2.4 Soundness notes (for implementers)

- Do **not** put Init into the premise (doing so makes it the same as BMC and not a proof).
- Include `_bounds_*` in Inv too. Since variables of bounded type become free
  variables in the step case, without assuming the bounds a large number of false
  CTIs originating from "ghost states out of range" appear. (The **check** of the
  bounds is borne by the base case, so putting them into the step premise causes
  no masking — because bounds violations across all reachable states are already
  detected in the base.)
- Among the physical-encoding constraints of enum / Option (e.g. enum value ∈
  [0, n-1], value is don't-care when `present == false`), add those that should
  always hold as a type to the step premise. Otherwise CTIs that are impossible
  under the physical encoding appear. Concretely:
  - enum field/variable v: `0 <= v < len(members)` (unnecessary if `_bounds_*`
    includes the enum; if not, add it explicitly)
  - Option: no additional constraint needed (any combination of present/value is meaningful)
- Deadlock checking is not done in induction (deadlock is a reachability
  property). The `deadlock` field is not included in the output of
  `--engine induction`.
- Action coverage also uses the base-case (BMC) result as-is.
- Ranked `leadsTo` has its own no-deadlock obligation in the pending region
  (`P ∧ ¬Q`). This is separate from ordinary deadlock reporting because it is a
  proof obligation over invariant states, returned as `unknown_cti` /
  `violation_kind:"leadsTo_rank"` if it fails.

## 3. Extracting the CTI (counterexample to induction)

When the step case is sat, build a trace of k+1 states from the model.
JSON (the shape finalized in §9, generalized to multiple states):

```json
{
  "fsl": "1.0",
  "result": "unknown_cti",
  "spec": "...",
  "invariant": "RevenueConsistent",
  "k": 2,
  "cti": {
    "states": [ {"step": 0, "state": {...}},
                {"step": 1, "state": {...}, "action": {...}, "changes": {...}},
                {"step": 2, "state": {...}, "action": {...}, "changes": {...}} ],
    "violated_at": 2
  },
  "hint": "this state sequence satisfies all invariants but leads to a violation; the start state may be unreachable — add an auxiliary invariant that excludes it, then re-run"
}
```

- The display of `states` uses the existing `_build_trace` logical-value recovery
  (`logical_state_values`) as-is (enum name reverse lookup, Option null/value,
  struct dict, no `__` internal names).
- The §9 `cti: {state, action, next_state}` shape (for k=1) is **not** an alias
  but is **unified into this general form** (even for k=1, a `states` array of
  length 2). The JSON example in DESIGN-v1.md §9 should be updated to follow this document's shape.
- **Monotone-counter suggestions (#74, post-processing only, no solver/engine
  change):** after the CTI trace is built for an invariant `unknown_cti`
  (not `leadsTo_rank`), `_suggest_monotone_invariants` (`bmc.py`) scans it for
  a state variable — scalar `Int`/domain, or a `Map<K, Int>` key-wise — that
  moves in only one direction across the trace *and* starts on the
  unreachable side of the concrete initial value obtained from
  `runtime.Monitor(spec).reset()`. When found, the result gains
  `"suggested_invariants": ["<expr>", ...]` and one sentence is appended to
  `hint` per suggestion, e.g. `"audit >= 0"` or, for a uniformly-initialized
  map, `"forall k: Case { audit[k] >= 0 }"`. If `reset()` fails, the relevant
  variable's init isn't a concrete `Int`, or the CTI start does not actually
  violate the would-be bound, no suggestion is added for it. This is
  trace-monotonicity, not a global-monotonicity proof, so it is phrased as a
  suggestion — see `docs/LANGUAGE.md` §9 "Auxiliary invariants from a CTI".
- The exit code is **not 2, not 1 of a new kind, and not 0**, but reuses `1`
  without introducing a new one (the "property not yet established" category;
  the repair loop branches on the result string, so exit-code granularity is unnecessary).

## 4. CLI / JSON changes

```
fslc verify <file.fsl> --engine induction [--k N] [--depth K]
```

- `--engine bmc` (default) behaves exactly as before. Code paths are untouched outside the shared parts.
- `--k N`: maximum induction depth K_ind. Default 1.
- `--depth K`: BMC depth of the base case + reachable witness search depth. Default 8.
- Output on success:

```json
{
  "fsl": "1.0",
  "result": "proved",
  "spec": "...",
  "engine": "induction",
  "completeness": "unbounded",
  "checked_to_depth": 8,
  "cost": {"elapsed_s": 0.01},
  "k_used": { "ShippedWasPaid": 1, "RevenueConsistent": 2, "_bounds_orders": 1 },
  "base_depth": 8,
  "invariants_checked": [...],
  "action_coverage": {...},        // from the base-case BMC
  "reachables": {...},             // result of the witness search on the base-case side
  "warnings": [...]
}
```

If a ranked `leadsTo` is proved, the ordinary `leads_to` entry is upgraded:

```json
"leads_to": {
  "ReachDone": {
    "checked_to_depth": 1,
    "proved": true,
    "completeness": "unbounded",
    "proof": "ranking",
    "decreases": "(5 - x)"
  }
}
```

- The exit code of `proved` is 0.
- If a reachable is not found, `reachable_failed` (exit 1) takes **precedence**
  over proved as before (0 only when all properties hold).
- `proved` is the only induction result with `completeness:"unbounded"`.
  `unknown_cti` and base-case failures remain `completeness:"bounded"` and carry
  `checked_to_depth` for the base BMC depth.
- Failed ranked `leadsTo` obligations return `unknown_cti` with
  `violation_kind:"leadsTo_rank"` and `rank_failure` naming the failed
  obligation (`unbounded_below`, `deadlock`, `non_decreasing_action`, or
  `pending_not_preserved`). Transition-progress failures include `last_action`,
  `measure_before`, `measure_after`, and a two-state CTI.
- Consistency with the existing schema: the `violated` shape is completely
  identical to BMC (it is so automatically because the base case returns it).

## 5. Integration into existing code (bmc.py)

New function `prove(spec, k_ind, base_depth, deadlock_mode)`:

1. Call `verify(spec, base_depth, ...)` (= base case + reachables + coverage).
   - If `violated` / `reachable_failed` / `error`, return that base-case verdict
     with the induction call's top-level cost metadata.
2. For the step case, build the state sequence σ_0..σ_k with `make_state(spec, t)`
   (a suffix such as `@ind{t}` to avoid name collisions), and push Inv(σ_0..σ_{k-1})
   and T onto a shared solver.
3. Per invariant: push / ¬inv_i(σ_k) / check / pop.
4. All invariant checks unsat → check ranked `leadsTo` obligations, if any.
   Failed ranking obligations return `unknown_cti`.
5. All unsat → reshape the verify result dict to `result: "proved"` and return.
   Any sat → extract the CTI and `unknown_cti`.
6. When incrementing k, you can reuse by **only adding** σ_{k+1} and Inv(σ_k)·
   T(σ_k, σ_{k+1}) (do not rebuild the solver). However, "adding Inv(σ_k) to the
   premise" must be done after the pop so as not to conflict with the ¬inv_i check at k.

Implementation notes:
- `eval_expr(inv, σ_t, {}, spec)` can be used as-is (just swap the state dict).
  `transition(spec, instances, σ_t, σ_{t+1}, ch_t)` likewise.
- Choice variables get a separate name for the step (`__ind_choice@t`).
- The resolution of PERF1 (expansion sharing) is a prerequisite. Build on top of the post-completion codebase.

## 6. Test plan (to be added to the regression suite)

1. **A spec that becomes proved**: `specs/cart_v1.fsl` has a SoldOut witness, so
   confirm it is proved + reachables as-is (all invariants should be inductive at
   k=1; if a CTI appears, that itself is a sign of an implementation bug such as
   "forgot to put bounds in the premise").
2. **counter latch** (reliably proved at k=1):
   `state { x: Int }  init { x = 0 }  action inc() { requires x < 5  x = x + 1 }
   invariant XRange { x >= 0 and x <= 5 }`
3. **A spec that becomes unknown_cti** (a true but non-inductive invariant):
   `state { x: Int, y: Int }  init { x = 0  y = 0 }
   action step() { requires x < 4  x = x + 1  y = y + 1 }
   invariant Sync { y <= 4 }` — Sync is true (y stays in sync with x and stops at
   4) but is not inductive without its tie to x (the auxiliary invariant `x == y`).
   A CTI is returned, `states` is JSON-serializable, and a hint is present.
   Furthermore, adding `invariant Aux { x == y }` changes it to **proved**
   (= end-to-end verification of the LLM strengthening loop).
4. **violated in the base**: cart_v1_buggy returns, under induction, the same
   violated JSON (shortest counterexample) as before.
5. **CLI**: the exit code of `--engine induction` (proved=0, unknown_cti=1), and
   the presence of the `engine`/`k_used` fields.
6. **A spec that requires k=2** (an example where 2 appears in k_used):
   one-step-delayed following such as
   `state { a: Bool, b: Bool }  init { a = false  b = false }
   action flip() { a = not a  b = a }` with
   `invariant Lag { b => a }` … may be tuned with a real example after
   implementation (see the comment). If hard to construct, the k=2 case may be
   substituted by verifying "with Aux removed, Sync is tried at k=2..4 and all
   are sat" (= that the k iteration runs).
7. **ranked leadsTo proof**: `leadsTo ReachFive { x < 5 ~> x == 5 decreases 5 - x }`
   proves `leads_to.ReachFive.completeness == "unbounded"` at `--depth 1`.
8. **ranking diagnostics**: `decreases x` reports `non_decreasing_action` with
   action/measure before-after; `decreases -x` reports `unbounded_below`.

## 7. Reflecting back into DESIGN-v1.md

On implementation completion, update §9 to a pointer to this document + the
finalized JSON shape (states-array form), and remove the "v1.1" note from
`--engine induction` in §7.1.
