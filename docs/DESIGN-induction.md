# FSL k-Induction Engine — Implementation Design (v1.1, elaboration of DESIGN-v1.md §9)

This document is an implementation-level specification of `--engine induction`.
The protocol of §9 (`proved` / `unknown_cti` / JSON shape) is finalized; here we
specify the semantics, the algorithm, the integration into existing code, and
edge cases.

## 1. Goals and non-goals

- **Goal**: **unbounded-depth proof** of invariants (user-defined + automatic
  `_bounds_*`). On success, `result: "proved"`. It promotes BMC's "no violation
  up to depth K" to "holds in every reachable state."
- **Non-goals (not handled in v1.1)**:
  - Proof of `reachable` (`reachable` may remain a bounded witness search;
    induction also searches for a witness the same way as BMC — depth reuses `--depth`)
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

### 2.3 Soundness notes (for implementers)

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

- The exit code of `proved` is 0.
- If a reachable is not found, `reachable_failed` (exit 1) takes **precedence**
  over proved as before (0 only when all properties hold).
- `proved` is the only induction result with `completeness:"unbounded"`.
  `unknown_cti` and base-case failures remain `completeness:"bounded"` and carry
  `checked_to_depth` for the base BMC depth.
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
4. All unsat → reshape the verify result dict to `result: "proved"` and return.
   Any sat → extract the CTI and `unknown_cti`.
5. When incrementing k, you can reuse by **only adding** σ_{k+1} and Inv(σ_k)·
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

## 7. Reflecting back into DESIGN-v1.md

On implementation completion, update §9 to a pointer to this document + the
finalized JSON shape (states-array form), and remove the "v1.1" note from
`--engine induction` in §7.1.
