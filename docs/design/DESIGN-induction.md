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
    no k-chain is needed; it is a one-step obligation under the proved
    invariants instead — see §2.7)
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

### 2.5 Ranking as a BMC fast path for the lasso search (#1149)

`verify` (BMC, and the base case of `--engine induction`) runs the §2.3
ranking obligations as a pre-pass for every `leadsTo` that declares
`decreases`, and skips the bounded fair-lasso search
(`DESIGN-temporal.md` §2.1–2.2; `check_leadstos` in
`rust/fsl-verifier/src/bmc.rs`) for each property the ranking discharges.
A discharge is a position in `model.leadstos`, which the ranking and the
lasso search both walk in order — never a property name, because `check`
accepts two `leadsTo` blocks with the same name, and a name key would let one
block's ranking withdraw the other block's search. That search is `B·D(D+1)(2D+1)/6` probes for `B` bindings at depth `D` — the
cubic term — and the ranking is a constant number of checks per binding.

**What is still claimed.** For each `leadsTo` block, exactly what the full
search claims for that block: no fair lasso counterexample with a loop inside
the first `D` steps. `result`,
`completeness:"bounded"`, `leads_to.<name>.checked_to_depth`, and every
witness are unchanged; the property is *not* reported `proved`. The ranking
could only support an unbounded claim if the invariants it assumes were
themselves proved inductive, which BMC does not do — that promotion remains
the induction engine's (§2.3, §4).

**Why the skipped probes are all `unsat`.** The pre-pass assumes, at the
pre-state, exactly the properties the same BMC run checks at every unrolled
step: every user invariant, and each `_bounds_*` the run's property
selection keeps (`checked_bounds`; a bound the run does not check is not
assumed, because an unrolled state may then violate it). BMC reaches the
lasso search only after every one of those checks came back `unsat` at every
step, so every unrolled state satisfies the ranking's premise. Take any
candidate lasso: a pending step `p`, a loop `states[i..j]` with
`states[j] ==L states[i]`, and `¬Q` on every state from `min(i, p)` to `j`.
Each step of it is a real transition, so the ranking's transition obligation
applies to it:

- Without `helpful`: every step from a pending state establishes `Q` (ruled
  out) or keeps `P` with `M` strictly smaller. So every state from `p` on is
  pending, and `M` strictly decreases around the loop back to the state it
  started from — impossible. No lasso exists, fair or not.
- With `helpful`: the non-helpful steps keep the obligation pending without
  increasing `M`, and a helpful step strictly decreases it, so no helpful
  instance fires inside the loop. `no_deadlock` makes some matching helpful
  instance enabled at each loop state; with one instance it is enabled at
  all of them, and with several, `helpful_sticky` keeps an enabled one
  enabled around the loop (it never fires there and `Q` never holds). That
  instance is `fair` (`helpful_fairness`), so the loop violates the weak
  fairness condition every lasso probe conjoins (`DESIGN-temporal.md` §2.2).
  No *fair* lasso exists, which is what the search asks for.

`M`, `P`, and `Q` are functions of the state (and the binding), so their value
at a loop's closing state is their value at its head. Measures whose
evaluation leaves a term unconstrained are covered too: the ranking is
proved for every value of that term.

**What is not skipped.** The per-step stagnation probe (pending deadlock,
`DESIGN-temporal.md` §2.4), the `within` deadline probe, and the per-step
definedness checks of `P`/`Q` still run for a discharged property. Since
#1189 the ranking includes a no-deadlock obligation over the same premise (a
pending invariant state has an enabled action — a matching helpful one when
`helpful` is declared), so a discharged property has no reachable pending
deadlock and its stagnation probes are all `unsat` too. They are kept
anyway: the fast path's argument above does not need that obligation, and
the stagnation term is only quadratic, `B·(D+1)(D+2)/2`. A ranked `leadsTo`
that stalls fails the no-deadlock obligation, so nothing is discharged and
the stall is reported exactly as before (`stutter: true`).

**Fail-closed fallback.** The ranking stops at its first failing property;
only the properties proved before it are discharged, and every other
`leadsTo` keeps the full search. A ranking error (unsupported measure,
solver `unknown`, fail-closed `where` filters) discharges nothing and is not
reported — the BMC run then produces whatever it produced before, including
its own error. The same holds when the pre-pass cannot run at all (no
thread, no solver) and when it panics: the run proceeds without it (the
panic message still reaches stderr). The pre-pass never reports a verdict or
a witness of its own.

**Budget.** Every pre-pass check has a 5 s wall-clock limit
(`RANKING_PREPASS_CHECK_TIMEOUT_MS`); a check that runs out answers
`unknown` and so discharges nothing. The number of checks is linear in
bindings × action instances. The limit depends on machine load, so load can
decide whether the shortcut is taken, and with it `cost`. It can never turn
`verified` into `violated` or the reverse: a discharged lasso search has no
`sat` probe to find. The one outcome load can move is error versus verdict —
when the shortcut is not taken, a lasso probe that the solver answers
`unknown` makes the run an error, as it always did, while a run that took
the shortcut never asks that probe.

**Solver isolation.** The pre-pass runs on its own thread with its own Z3
solver. The native backend's `Solver::new()` uses the thread's default
context, and running the ranking in the BMC thread — even on a separate
`Solver` — was observed to change which model Z3 returns for later BMC
witness queries (the lasso trace of `helpful` specs whose ranking fails). On
its own thread the BMC session's query history up to the lasso search is
exactly what it is without the pre-pass. The byte-identity claim is narrower
than "every witness": it covers the fallback path, a lasso search that runs
with nothing discharged before it in the same run (in particular every run
whose ranking failed on its first ranked property), whose witness is then
byte-identical to the full search's. A lasso search that runs *after* a
discharged property starts from a session that skipped that property's
probes; its verdict is the full search's (the skipped probes were `unsat`),
but Z3 may return a different, equally valid lasso witness (the corpus
differential observed none).

**Envelope change.** Only `cost`: `cost.properties` gains a
`{"kind":"leadsTo_rank","name":<leadsTo>}` row for each ranked `leadsTo`
(its checks and time are also in `cost.solver`), and the `leadsTo` row of a
discharged property drops to the stagnation probe count. `--engine
induction` runs the ranking twice — once in its base-case BMC and once as
the §2.3 proof — so its `leadsTo_rank` row doubles.

The pre-pass is wired into the CLI's BMC path (`solve_bmc` in
`rust/fslc/src/verification.rs`). The library entry points `verify_bounded*`
and their other callers (the browser worker, `refine`, causal and mutation
helpers) keep the full search.

### 2.6 Definedness obligation (#1196)

The step case evaluates guards, bodies, and properties with the totalizing
symbolic evaluator, so by itself it cannot see a reached partial operation
(LANGUAGE.md §6), which BMC and the explicit engine report as `partial_op`
(DESIGN-kernel-contract.md: "a reached partial expression is `partial_op`").
After every invariant and transition property is proved, a definedness
obligation is checked over a free state `s` and a free successor `s'`
(no Init), mirroring BMC's per-step checks
(`bmc::check_state_properties`, `bmc::check_action_partial_operations`):

```
Inv(s) ∧ first_partial(P, s)                              P: each invariant, leadsTo trigger/goal
Inv(s) ∧ first_partial(guards_a, s)                       each action instance a
Inv(s) ∧ enabled_a(s) ∧ first_partial(body_a, s)
Inv(s) ∧ T(s,s') ∧ Inv(s') ∧ Trans(s,s') ∧ first_partial(R, s, s')
                                                          R: each trans; each ensures reached
                                                          (selected ∧ enabled ∧ body defined ∧
                                                          earlier ensures defined and true)
```

`first_partial` is the same path-sensitive predicate BMC uses: guards in
source order, `and`/`or`/`=>`/`if` short-circuit, and property context keeps
division total while sequence access stays partial. So `d != 0 and x / d <
100` is defined, `x / d < 100` alone is not. Only expressions with a static
partial-operation candidate are queried, so specs without one issue no new
solver checks. Checked i64 overflow is *not* part of this obligation (step
states are unbounded integers); it remains a base-case fail-closed error.

Soundness: every reachable state satisfies the proved invariants, and every
real (defined) step is a step of the totalized `T`, so `unsat` for every
query shows that no reachable state or step reaches a partial operation —
independent of the base depth. `sat` is returned as `unknown_cti` with
`violation_kind: "partial_op"` and `invariant: "_partial_<action>"` /
`"_partial_property_<name>"` (the names BMC uses); the start state may be
unreachable, like any CTI, and an auxiliary invariant that excludes it
restores `proved`. When the undefined state is within `--depth`, the base
case reports it first. The obligation is 1-step (it does not use the
`--k` chain), which keeps it independent of the base depth. The k-induction
premises (Inv and T over the `ind*` chain) are asserted inside a solver scope
that is popped before this obligation and before the ranked-`leadsTo` proof
reuse the solver; otherwise an unsatisfiable chain (no Inv state with k
successors) would make every later query vacuously `unsat`.

Scope of the premise: `Inv` is the set of invariants in the run's model.
`--lemma` adjudication proves only the lemma's truth
(`prove_induction_invariants`, no definedness), because its candidate model
drops the user invariants and would otherwise reject a lemma whose actions are
protected by one of them; a used lemma joins the target run as an auxiliary
invariant, where this obligation is checked. `--property <invariant>` (and
`sweep --property` under `--engine induction`) narrows the model to the
selected invariant, so a division that only a dropped invariant protects is
reported as `_partial_<action>` there — a sound-side change, since the
narrowed run claims nothing the dropped invariant would have to carry.
Selecting a `trans` keeps every invariant as hypothesis
(`selected_transition_induction_model`). Non-partial undefinedness BMC fails
closed on (checked i64 overflow, a finite `Map` read outside its key domain)
is not part of this obligation.

### 2.7 Ensures obligation (#1217)

`ensures` is checked by BMC on every reached step, but before #1217 the step
case never asked it, so a false `ensures` beyond `--depth` was reported
`proved`/`unbounded`. It is now part of the same one-step obligation as §2.6,
over the same scope (free `s`, `s'`, the k-chain popped, no Init):

```
Inv(s) ∧ T(s,s') ∧ Inv(s') ∧ Trans(s,s') ∧ reached(E, s, s') ∧ defined(E) ∧ ¬E(s, s')
```

for every `ensures` E of every action instance. `reached` is BMC's
(`bmc::check_state_properties`): the instance is the selected one, its
guards are enabled, its body is defined, and every earlier `ensures` of the
action is defined and true; E's own `partial_op` is asked first (§2.6).

Soundness is the §2.6 argument: every reachable step goes from and to states
that satisfy the proved invariants (and the proved `trans`) and is a step of
`T`, so `unsat` shows no reachable step falsifies a reached `ensures`. `sat` is
`unknown_cti` with `violation_kind: "ensures"`, `invariant: "<action>"` and
`last_action` (BMC's `ensures` naming), a two-state CTI whose last step is the
attempted action. An `ensures` whose truth follows from an invariant stays
`proved` because `Inv(s)` is a premise; an unreachable CTI start is excluded
with an auxiliary invariant as usual. `--lemma` adjudication does not ask it
(`prove_induction_invariants`, §2.6).

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
  (not `leadsTo_rank`), native `suggested_invariants` in
  `rust/fslc/src/verification.rs` scans it for a state variable — scalar
  `Int`/domain, or a `Map<K, Int>` key-wise — that
  moves in only one direction across the trace *and* starts on the
  unreachable side of the concrete initial value obtained from
  `fsl_runtime::deterministic_initial_state`. Map candidates additionally
  require every changed key to move in the same direction. When found, the
  result gains
  `"suggested_invariants": ["<expr>", ...]` and one sentence is appended to
  `hint` per suggestion, e.g. `"audit >= 0"` or, for a uniformly-initialized
  map, `"forall k: Case { audit[k] >= 0 }"`. If deterministic initial-state
  construction fails, the relevant variable's init isn't a concrete `Int`, or
  the CTI start does not actually violate the would-be bound, no suggestion is
  added for it. This is
  trace-monotonicity, not a global-monotonicity proof, so it is phrased as a
  suggestion — see `docs/manual/LANGUAGE.md` §9 "Auxiliary invariants from a CTI".
- The exit code is **not 2, not 1 of a new kind, and not 0**, but reuses `1`
  without introducing a new one (the "property not yet established" category;
  the repair loop branches on the result string, so exit-code granularity is unnecessary).

## 4. CLI / JSON changes

```
fslc verify <file.fsl> --engine induction [--k N] [--depth K]
            [--lemma "<expr>"]...
```

- `--engine bmc` (default) behaves exactly as before. Code paths are untouched outside the shared parts.
- `--k N`: maximum induction depth K_ind. Default 1.
- `--depth K`: BMC depth of the base case + reachable witness search depth. Default 8.
- `--lemma EXPR`: repeatable, independently proved auxiliary-invariant
  candidates. Only `proved` candidates may enter the target proof; see
  `DESIGN-induction-lemmas.md` for the adjudication, CTI-exclusion, JSON, and
  cache contract.
- Output on success:

```json
{
  "fsl": "1.0",
  "result": "proved",
  "spec": "...",
  "engine": "induction",
  "completeness": "unbounded",
  "checked_to_depth": 8,
  "cost": {
    "elapsed_s": 0.01,
    "solver": {"checks": 12, "check_elapsed_s": 0.004, "conflicts": 2, "decisions": 8, "propagations": 21, "memory_mb": 18.2},
    "properties": [{"kind": "invariant", "name": "ShippedWasPaid", "checks": 2, "elapsed_s": 0.001}]
  },
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
