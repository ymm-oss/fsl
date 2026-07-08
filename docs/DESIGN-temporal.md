# FSL v2.0-lite — `leadsTo`, Fairness, and Ranked Induction Implementation Design

The first two items of DESIGN-v1.md §10 v2.0. The motivation is DOGFOOD-1 F1 /
DOGFOOD-2 F7: "eventually Y after X" (a response property) cannot be written with
state alone.

## 1. Syntax

```fsl
fair action release_handoff() { ... }       // fairness annotation (weak fairness)

leadsTo WaiterGetsLock {
  forall p: ProcId {
    waiters.contains(p) ~> (holder is some(h) and h == p)
  }
}

leadsTo LeavesDecayZone {
  inZone ~> !inZone
  decreases waterBudgetLeft
}
```

- Add `leadsTo <Name> { <lt> }` as a top-level item.
  `lt := <expr> "~>" <expr> | "forall" binder "{" lt "}"` (forall only on the
  outside, nesting allowed. `~>` is an operator exclusive to the leadsTo block
  and cannot be used in general expressions).
- A `leadsTo` block may end with `decreases <expr>`. The expression must be
  integer-valued. It is ignored by the bounded BMC check and used only by
  `--engine induction` to prove the response unboundedly.
- `fair` is a prefix modifier on an action definition. Its meaning is **weak
  fairness** (if that instance stays continuously enabled, it is eventually
  executed).
- In the requirements dialect, the same fairness prefix can be combined with an
  action-level refinement correspondence:

  ```fsl
  fair action tick() maps stutter {
    y = y
  }
  ```

  Here `fair` still has the weak-fairness meaning described in this document.
  `maps stutter` belongs to the requirements/refinement layer: the action is an
  internal upper-layer step whose mapped abstract state must not change. A CLI
  repro with `map x = y` observed `fslc check` returning `result:"ok"` and
  `implements.result:"refines"`, and `fslc verify --depth 1` returning
  `result:"verified"` with the same implements result.

## 2. Semantics

`P ~> Q`: in every execution, "from the point where P holds, Q holds at some
point including that same point." (If P and Q hold simultaneously, it is
satisfied immediately.)

A counterexample is an infinite execution, which in a finite-state system can be
finitely represented as a **lasso** (prefix + repeating loop) or a **deadlock
stall** (stopping forever in that state):

### 2.1 Lasso Counterexample

For positions i < j ≤ K,

```
loop(i, j)   := states[j] ==L states[i]                  // logical state equality (§2.3)
violation    := ∃ i < j ≤ K, ∃ p < j:
                  loop(i, j)
                ∧ P(states[p])
                ∧ ∀ q ∈ [min(i, p), j-1]: ¬Q(states[q])
                ∧ fairness_ok(i, j)                       // §2.2
```

- The range of `¬Q` is `[min(i,p), j-1]` because all states inside the loop are
  revisited infinitely often, so whether p is inside the loop or in the prefix,
  Q must never hold over "p onward + the entire loop."
- K reuses `--depth`. The expansion piggybacks on verify's shared expansion
  (`_bmc_explore`), with one query per leadsTo via push/pop. Since i, j, p are on
  the order of K ≤ 10, a **bounded expansion (enumeration of Or)** suffices (it
  is more straightforward and easier to debug than integer variables).

### 2.2 Excluding Lassos by Weak Fairness

For each instance a of an action annotated `fair`:

```
fairness_ok(i, j) := ∀ a ∈ FairInstances:
    (∃ q ∈ [i, j-1]: ¬enabled_a(states[q]))     // becomes disabled at least once in the loop
  ∨ (∃ q ∈ [i, j-1]: choices[q] == a)           // or is executed at least once in the loop
```

A loop that is continuously enabled yet never executed is excluded from
counterexamples as "does not happen in reality." Actions without `fair` have no
constraint.

### 2.3 Logical State Equality `==L`

The state comparison for loop detection is done by **logical equality, not raw
comparison of physical variables**:

- scalar / Map / Set: equality of physical variables
- `Option`: presents equal, and `present => values equal` (value when absent is
  don't care)
- `Seq`: `len` equal, and `∀ idx < len: data equal` (the tail is don't care)

With raw comparison, a difference in the don't-care part would miss a loop of the
same logical state (= missing a counterexample; required so as not to degrade
precision even for a bounded check).

### 2.4 Deadlock Stall Counterexample

If all actions are disabled (deadlock) at states[j], the execution stalls there
forever:

```
violation_stutter := ∃ j ≤ K, ∃ p ≤ j:
    deadlock(states[j]) ∧ P(states[p]) ∧ ∀ q ∈ [p, j]: ¬Q(states[q])
```

(Fairness is not applied in a deadlock state — there is nothing enabled.)

### 2.5 leadsTo with forall

`forall x: T { P(x) ~> Q(x) }` is expanded per instance, and §2.1/§2.4 are
checked **independently for each binding** (if even one has a counterexample it
is violated; the counterexample JSON includes `bindings`).

### 2.5.1 Symmetry Reduction

A finite domain or enum may be declared symmetric:

```fsl
symmetric type TaskId = 0..2
symmetric enum Worker { A, B, C }
```

The declaration means values of that type are interchangeable entity identities.
For `leadsTo` lasso and deadlock-stall queries, fslc adds a symmetry-breaking
constraint to the representative state:

- lasso: the loop-head state `states[i]` must be canonical
- stall: the deadlocked/stalled state `states[t]` must be canonical

Canonicalization is intentionally partial and conservative. For each symmetric
type, fslc builds per-entity rows from state variables shaped as
`Map<SymmetricType, V>` and `Set<SymmetricType>`, as long as `V` does not itself
mention any symmetric type. Rows are ordered lexicographically by source order of
the contributing state variables/fields. This covers the common
`Map<TaskId, Status>` / `Map<TaskId, Bool>` liveness model without trying to
canonicalize references such as `holder: Option<TaskId>` or `Seq<TaskId, N>`.

Soundness argument: the transition relation and properties of a valid
`symmetric` model are equivariant under a single global renaming of that type's
values. Given any lasso or stall counterexample, choose the global permutation
that sorts the representative state's row vector. The renamed trace is still a
valid counterexample; `leadsTo` bindings are still enumerated after renaming.
fslc therefore constrains only the loop head or stalled state, not every
intermediate state, avoiding the unsound "canonical state after every step"
shortcut.

### 2.6 Ranked Induction with `decreases`

For `leadsTo L { P ~> Q decreases M }`, induction first runs the ordinary
bounded base check. After all invariants are proved inductive, it discharges the
response over the invariant abstraction with these obligations, per outer
`forall` binding:

```
Inv(s) ∧ P(s) ∧ ¬Q(s)              ⇒ M(s) >= 0
Inv(s) ∧ P(s) ∧ ¬Q(s)              ⇒ some action is enabled
Inv(s) ∧ P(s) ∧ ¬Q(s) ∧ T_a(s,s') ⇒ Q(s') ∨ (P(s') ∧ M(s') < M(s))
```

The final `P(s')` conjunct is intentionally stronger than a bare decrease check:
it keeps the response obligation in the ranked region until `Q` holds. Without
that persistence condition, a trigger could disappear before `Q` and the rank
would no longer justify the original response property. Fairness annotations are
not used by the ranking proof; every enabled action must make ranked progress.

With one or more `helpful action(args)` lines, only the matching instance(s)
must decrease `M`; other actions must instead keep the obligation pending
(or resolve it) *without increasing* `M`. Two extra obligations then license
using each matching instance's `fair` declaration: every matching instance
must itself be `fair` (`helpful_fairness`), and -- when two or more distinct
`helpful` actions are declared -- once an instance becomes enabled while
pending, no other action may disable it again before it fires or `Q` holds
(`helpful_sticky`). Without `helpful_sticky`, the weaker "some helpful match
is enabled at every pending state" does not by itself prove any single
instance continuously enabled (which instance is enabled can vary by state),
so its `fair` declaration is never actually obligated to fire. See
`DESIGN-induction.md` §2.3 for the full obligation set and rationale.

## 3. Positioning and Result of the Check

- **violated (counterexample found) is a definite violation** (the lasso is a
  real infinite execution).
- **No counterexample is a bounded guarantee of "no counterexample up to
  depth K"** (a lasso with a prefix exceeding K is not seen). It has the same
  positioning as `verified` for invariants, and puts `checked_to_depth` in the
  `leads_to` field.
- With `--engine induction`, unranked `leadsTo` performs **the same check on the
  base case (BMC) side** and remains bounded. A ranked `leadsTo` whose
  obligations in §2.6 are unsat is reported as an unbounded ranking proof in its
  `leads_to` entry.

## 4. JSON

### 4.1 Violation

```json
{
  "fsl": "1.0",
  "result": "violated",
  "violation_kind": "leadsTo",
  "invariant": "WaiterGetsLock",
  "loc": { "line": 40, "column": 3 },
  "bindings": { "p": 0 },
  "pending_since": 1,
  "trace": [ { "step": 0, ... }, ... ],
  "loop_start": 2,
  "stutter": false,
  "hint": "P held at step 1 but the loop from step 2 can repeat forever without Q; if progress relies on some action being taken eventually, annotate it with `fair action ...`"
}
```

- `trace` is the existing format (state / action / changes). The final state is
  logically equal to the loop head (`loop_start`). In a deadlock-stall
  counterexample, `stutter: true` and, instead of `loop_start`, it stalls at the
  final step.
- `pending_since`: the step where P held (and Q does not come thereafter).
- The exit code is 1, same as other violated.

### 4.2 On Success (Addition to verified / proved)

```json
"leads_to": {
  "WaiterGetsLock": { "checked_to_depth": 8 }
}
```

For a ranked response proved by induction:

```json
"leads_to": {
  "LeavesDecayZone": {
    "checked_to_depth": 1,
    "proved": true,
    "completeness": "unbounded",
    "proof": "ranking",
    "decreases": "waterBudgetLeft"
  }
}
```

If a ranking obligation fails, induction returns `unknown_cti` with
`violation_kind: "leadsTo_rank"`, `rank_failure` (`unbounded_below`,
`deadlock`, `non_decreasing_action`, or `pending_not_preserved`; with
`helpful`, also `progress_action_not_fair`, `helpful_action_not_enabled`,
`non_decreasing_helpful_action`, `non_helpful_action_increases_measure`, and
-- with two or more distinct helpful actions --
`helpful_action_enabledness_not_sticky`), the relevant
binding, a logical-state CTI, and the selected action/measure values when the
failure is a transition-progress failure.

## 5. Implementation Notes

- **grammar.py**: `leadsTo_def`, `~>` (`LEADSTO_OP`), optional
  `decreases <expr>`, and the `fair` modifier.
  AST: `("leadsto", name, binders, P, Q, loc, meta, decreases)` (binders is the
  list of the outer forall), `fair: bool` on the action.
- **model.py**: propagate `leadstos` into the spec dict, and `fair` into the
  action/instance. `symmetric type` / `symmetric enum` are carried in
  `spec["types"][name]["symmetric"]`, `spec["symmetry"]`, and
  `spec["state_type_refs"]` so the verifier can distinguish nominally different
  domain types with the same numeric range. The whitelist validation is
  unchanged. Make the grammar such that `~>` appearing in a general expression is
  a parse error (do not put it in the expression hierarchy).
- **bmc.py**:
  - `_logical_eq(spec, s1, s2)` — a helper that returns the logical equality of
    §2.3 (built using the phys_vars metadata — Option's present/value, Seq's
    data/len).
  - The leadsTo check, after `_bmc_explore` (like verify's reachable handling),
    runs on the shared solver per leadsTo × binding with push/pop:
    `s.add(Or over (i,j,p) of [loop ∧ P ∧ ¬Q sequence ∧ fairness_ok])` → if sat,
    identify (i, j, p) from the model (attach a selector Bool to each (i,j,p)
    candidate and read it from the model) and build the trace.
  - enabled_a reuses the same `_eval_requires` conjunction as the coverage check
    (expr_cache works).
  - The deadlock stall (§2.4) reuses the enabled expression of the existing
    deadlock check.
  - Symmetry reduction (§2.5.1) adds canonical row-order constraints only inside
    the lasso/stall push/pop queries. It is not asserted on the shared path
    solver, so safety/reachability behavior and finite transition construction
    remain unchanged.
  - Ranked induction (§2.6) is checked after invariant induction succeeds:
    lower-bound, no-deadlock, and per-action progress queries are issued per
    `leadsTo` binding. Failure is `unknown_cti` because it is a failed proof
    obligation over the invariant abstraction, not necessarily a reachable
    bounded violation.
  - Performance: one query per leadsTo binding. The expression size is
    O(K² · (|P|+|Q|+|Fair|·K)). For K=8 and a number of bindings on the order of
    the capacity, there is no problem (it runs on top of PERF1's shared
    expansion).
- **cli.py**: minimal changes (only a new violation_kind).
- **scenarios**: leadsTo is out of scope (future: leave a comment about the
  possibility of turning a pending→achieved trace into a scenario).

## 6. Test Plan (tests/test_temporal.py)

1. **stutter counterexample**: a spec that deadlocks after becoming P so Q never
   comes → violated / leadsTo / stutter: true.
2. **lasso counterexample (no fairness)**: a mutex with a noop self-loop,
   `waiters.contains(p) ~> holder == some(p)` → violated, loop_start in trace, a
   `fair` suggestion in the hint.
3. **proof with fairness**: annotate `fair` on 2's release_handoff (and others
   if needed) → the counterexample disappears (leads_to.checked_to_depth is
   returned). Confirm that the noop loop is excluded because
   "release_handoff is enabled yet not executed."
4. **simultaneous satisfaction**: a transition where P ∧ Q hold at the same time
   → not a violation.
5. **forall leadsTo**: a check per binding. bindings is returned on violated.
6. **logical-equality loop**: a loop of the same logical state differing only in
   the don't-care tail of a Seq is detected (reproduce the case that raw
   comparison would miss and make it a regression test).
7. **existing compatibility**: the verify/proved output of a spec without
   leadsTo is completely unchanged.
8. **combined with induction (unranked)**: `--engine induction` of a spec with
   unranked leadsTo returns proved + leads_to.checked_to_depth.
9. **ranked induction**: a response with `decreases` returns
   leads_to.<name>.proved + completeness:"unbounded" at a depth too small to
   represent full completion.
10. **bad ranking diagnostics**: non-decreasing and negative measures return
   `unknown_cti` / `leadsTo_rank` with action/measure or lower-bound detail.

## 7. Scenario-ization of leadsTo (Implemented in v2.1)

Add a response scenario to `fslc scenarios`. For a leadsTo `P ~> Q` (per
binding), generate the **shortest trace from when P holds until Q holds**:

- Search: on the shared expansion, decide "∃p ≤ t: P(states[p]) ∧ Q(states[t]) ∧
  ∀q ∈ [p, t-1]: ¬Q(states[q])" in order of t = 0..K via push/pop, and build the
  trace from the model of the first t that becomes sat (p is also identified from
  the model).
- Scenario form:

```json
{
  "name": "respond_WaiterGetsLock_p0",
  "kind": "leadsTo",
  "property": "WaiterGetsLock",
  "bindings": { "p": 0 },
  "steps": [ ... ],
  "pending_at": 1,
  "satisfied_at": 3,
  "initial_state": { ... },
  "expected_states": [ ... ]
}
```

- A binding where P never holds within depth K generates no scenario, and puts
  `{message: "leadsTo <name> <bindings>: P never holds within depth K", hint}` in
  warnings (not a silent cap).
- P∧Q simultaneous satisfaction (pending_at == satisfied_at) is also generated as
  a legitimate scenario.
- Implementation location: piggybacks on the same place as the reachable
  scenario generation in scenarios(). The name is `respond_<property name>` +
  binding suffix (the `_p0` form; sanitization is handled by the existing
  mechanism on the testgen side).
- Tests: with mutex_queue (WaiterGetsLock × 3 bindings), scenarios are generated
  and the Monitor replay of steps confirms pending→satisfied; the warnings for a
  binding where P does not hold; and that the testgen output is importable.

## 8. Documentation Reflection

- Note "implemented (lite): fair / leadsTo" on the v2.0 item of DESIGN-v1.md §10.
- LANGUAGE.md: add `~>` (leadsTo block only) to §3, `fair` and `leadsTo` to §1,
  leadsTo to the §6 table, and "history ghost variable vs leadsTo: when to use
  which" to the §9 idiom collection (a fact of state → ghost, a response
  property → leadsTo).
- Add `fair` + `WaiterGetsLock` to mutex_queue.fsl as a worked example (resolving
  DOGFOOD-2 F7).
