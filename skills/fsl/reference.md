# FSL Language Reference Card (complete, condensed)

Read this entire file before writing a spec. This is the full syntax and full set
of rules as of v2.x.

## 1. Top-level structure

```fsl
spec <Name> {
  const <NAME> = <const expr>             // integer constant (expressions allowed: CAP - 1, etc.)
  type  <Name> = <lo>..<hi>               // domain type (bounded integer)
  symmetric type <Name> = <lo>..<hi>      // interchangeable entity identities
  enum  <Name> { <Member>, ... }
  symmetric enum <Name> { <Member>, ... }
  struct <Name> { <field>: <type>, ... }  // field: scalar | Option<scalar>

  state { <var>: <type>, ... }
  init  { <stmt>... }                     // assign exactly once to every variable (deterministic)

  [fair] action <name>(<p>: <type name>, ...) {
    requires <expr>                        // 0 or more. conjunction. enabled condition
    let <x> = <expr>                       // local binding
    <stmt>...
    ensures <expr>                         // 0 or more. old(expr) for the old state
  }

  invariant <Name> { <expr> }
  trans     <Name> { <expr> }            // two-state safety. old(expr) for the old state
  reachable <Name> { <expr> }
  leadsTo   <Name> { <expr> ~> [within K] <expr> [decreases <int expr>] } // outer forall x: T { … } may wrap the response
  unless    <Name> { <expr> unless <expr> } // safety: preserve P until Q
  until     <Name> { <expr> until <expr> }  // unless safety + progress P ~> Q
  terminal  { <expr> }                     // intended terminal state (excluded from the deadlock check)
}
```

Business/requirements dialects also have type-kinds whose finite bounds live in
a sibling top-level `verify` block instead of inline ranges:

```fsl
business <Name> {
  entity <Entity>                          // identity sort; size set by verify.instances
  control <ID> "<text>"                    // optional governance/control metadata
  policy <ID> "<text>" satisfies <ID> ...  // optional control traceability
}
requirements <Name> {
  entity <Entity>                          // optional explicit identity sort
  number <Number>                          // numeric sort; range set by verify.values
  process <Entity> with f: <Number> { ... } // process also declares the entity kind
}
verify {
  instances <Entity> = <N>
  values <Number> = <lo>..<hi>
}
```

Optional governance catalog (metadata and cross-business checks; no new kernel
semantics):

```fsl
governance <Name> {
  authority <Actor> owns <ControlID>, ...
  control <ControlID> "<text>" [owner <Actor>] [severity <Name>] [applies_to <Entity>]...
  delegates <BusinessName> from "<business.fsl>" {
    require <ControlID>
    <ControlID> is satisfied_by policy <PolicyID>, goal <GoalID>
  }
  preservation <Name> {
    before <AsIsBusiness> from "<asis.fsl>"
    after  <ToBeBusiness> from "<tobe.fsl>"
    preserve <ControlID>
    checked_by refinement "<mapping.fsl>"
  }
}
```

Composite spec (a separate top-level form):

```fsl
compose <Name> {
  use <SpecName> as <alias> from "<relative path>"   // multiple allowed. nested compose not allowed
  state { ... }  init { ... }                    // additional state on the composite side (optional)
  action <n>(<p>: <alias>.<Type>, ...) =
      <a>.<act>(<expr>...) [ || <b>.<act2>(<expr>...) ] {  // synchronize (run atomically together)
    [requires <expr>]... [<stmt>...]             // extra guards / assignments to composite-side state
  }
  internal <alias>.<action>                      // forbid standalone firing (only via synchronization)
  invariant/trans/reachable/leadsTo ...          // cross-reference via alias.var
}
```

Compose synchronization does **not** inherit `fair` from component actions. If a
fair constituent action is synchronized into a non-fair composite action, the
result JSON `warnings` includes `kind: "fair_not_inherited"` naming the
composite action and fair constituent(s). Use `fair action <name>(...) = ...`
when the synchronized action itself must be fair.

Compose synchronized arguments are **structural by bounded value range**, not
nominal by type name. Passing `core.TaskId` to an action parameter declared
`NoteId` is intended when both domains cover the same values: a repro with
`TaskId = 0..2`, `NoteId = 0..2`, and
`action sync(t: core.TaskId) = core.choose(t) || note.attach(t) { }` returned
`ok` from `fslc check` and `verified` from `fslc verify --depth 1`. If the target
is narrower (`NoteId = 0..1`), `check` still returns `ok`, but verification can
fail with `violated/type_bound` on the target component's `_bounds_...`
invariant (`sync(t: 2)` in the repro). Idiom: use same-range component-local
domain types for shared IDs; add a sync-action `requires` guard when passing to
a narrower domain.

refinement mapping (the third file; `fslc refine impl.fsl abs.fsl this.fsl`):

```fsl
refinement <Name> {
  impl <ImplSpecName>
  abs  <AbsSpecName>
  maps auto                                      // optional identity defaults for same-named compatible state/actions
  map <abs_var> = <expr over impl state>          // scalar abstract variable
  map <abs_var>[<x>: <KeyType>] = <expr>          // per-element mapping of a Map
  // conditional expressions allowed only inside mapping/argument expressions: if <c> then <a> else <b> (else required)
  action <impl_act>(<formal params>...) -> <abs_act>(<expr>...) | stutter
  // formal params may be bare names or name: Type annotations matching the impl action
  // explicit map/action entries override maps auto; incompatible same-name candidates are type errors
  preserve progress {                            // optional, only when upper leadsTo must be preserved
    respond <AbsLeadsTo> by <impl_act>, ...
  }
}
```

## 2. Types

| Type | How to write | Notes |
|---|---|---|
| Int / Bool | `n: Int` | Int is unbounded |
| Domain type | `type Qty = 0..5` | **automatic bound check** (violated/type_bound) |
| Inline state domain | `state { qty: 0..5 }` | Shorthand for a named domain type in a state-variable declaration |
| symmetric domain | `symmetric type TaskId = 0..2` | Same as a domain type, plus liveness symmetry reduction |
| entity kind (dialects) | `entity Claim` / `process Claim ...` | Finite identity sort for business/requirements; bound by `verify { instances Claim = N }` |
| number kind (dialects) | `number Amount` | Finite numeric sort for business/requirements; bound by `verify { values Amount = lo..hi }` |
| enum | `enum St { A, B }` | members are referenced and displayed by bare name |
| symmetric enum | `symmetric enum Worker { A, B }` | Same as enum, plus liveness symmetry reduction |
| struct | `struct S { f: Qty, o: Option<K> }` | field = scalar or Option<scalar> only |
| Option<T> | `c: Option<ItemId>` | T is a scalar. `none` / `some(e)` |
| Map<K, V> | `m: Map<ItemId, Qty>` | K is a bounded scalar (Int keys give a deprecation warning) |
| Set<T> | `s: Set<OrderId>` | T is a bounded scalar |
| Seq<T, N> | `q: Seq<JobId, CAP>` | T is a scalar, N is a positive constant. FIFO |

Scalar = Int / Bool / domain type / enum. In a `state` declaration,
`x: lo..hi` is an anonymous domain type and is equivalent to declaring
`type X = lo..hi` and writing `x: X`.
**State-variable whitelist**: scalar | Option<scalar> | struct |
Map<bounded scalar, scalar|Option|struct> | Set<bounded scalar> | Seq<scalar, N>.
Anything else (nested structs, Set/Map/Seq as a Map value, etc.) is rejected by
check as a type error.

## 3. Expression catalog

- Arithmetic: `+ - * / %`, unary `-`, `min(a,b)` `max(a,b)` `abs(a)`
  (in `a//b` everything after `//` becomes a comment, so write division with a
  space: `a / b`)
- Comparison: `== != < <= > >=` / logic: `and or not =>`
- Quantification: `forall x: T { expr }`, `exists x: T { expr }` (`where expr`
  allowed), `forall x in set_or_seq { expr }` / `exists x in set_or_seq { expr }`
  for expression-only Set/Seq iteration, and the v0 form
  `forall i in lo..hi: expr` (range is a constant expression: `0..CAP-1` recommended)
- Aggregation: `count(x: T where expr)`, `sum(x: T of expr [where expr])`
- Cardinality predicates: `unique(x: T where expr)` / `exactlyOne(x: T where expr)`;
  `x in set_or_seq [where expr]` is also allowed. `unique` means at most one
  matching binding; `exactlyOne` means exactly one.
- Option: `x == none` `x != none` `x is some(v)` (v is usable afterward within that
  formula). **`x == some(e)` and arithmetic/ordering on Option are type errors**
- struct: literal `S { f: 0, o: none }`, `s.f`, `==` (field-wise equality; for an
  Option field, presence matches ∧ present ⇒ values match)
- Set: `Set {}` `Set { 1, 2 }`, `.add(e) .remove(e) .contains(e) .size()`
- Seq: `Seq {}` `Seq { 1, 2 }` (element count ≤ N), `.push(e) .pop() .head() .at(i)
  .contains(e) .size()`, `==` (length + all elements)
- ensures/trans only: `old(expr)` / leadsTo only: `P ~> Q`,
  `P ~> within K Q`, plus optional `decreases <int expr>` for induction ranking /
  mapping-expression only:
  `if c then a else b`

## 4. Statements (init / action body)

- Assignment: `x = e`, `m[k] = e`, `m[k].f = e`, `o.f = e`, `o.f = some(e)`
- Set/Seq are re-assigned: `s = s.add(x)`, `q = q.pop().push(y)` (chaining allowed)
- `if expr { stmt... } [else { stmt... }]` (may nest with an if inside else)
- `forall x: T { stmt... }` (bulk assignment)

## 5. Semantic rules

1. One step = one action instance (name × parameters) executes atomically.
2. **Simultaneous assignment**: every RHS in the body reads the old state.
   Unassigned variables are unchanged (automatic framing).
3. **Double assignment = semantics error**: assigning twice to the same
   variable/field on the same path. then/else are separate paths (assigning in both
   is allowed). Assigning to the same variable **after an if** as inside a branch is
   also an error.
   For `Map<K, Struct>` values, the path includes the field: `m[k].f1 = ...`
   and `m[k].f2 = ...` in one action are allowed independent field writes
   (`check` and `verify --depth 1` succeed in the repro). Repeating the same
   field, e.g. `m[k].f1 = 1; m[k].f1 = 2`, fails during verification with
   `kind:"semantics"` and `double assignment to 'm' field 'f1' on the same path`.

   ```fsl
   struct Pair { f1: V, f2: V }
   state { m: Map<K, Pair> }
   action update(k: K) { m[k].f1 = 1  m[k].f2 = 2 }
   ```
4. enabled when all requires hold. ensures is checked after the transition.
5. For Seq `pop/head/at` and a nonzero divisor of `/` `%`, **well-definedness is
   checked automatically** in action context (partial_op). A requires guard or an if
   guard both work (path conditions are considered). An out-of-range at() inside an
   invariant/reachable is an undefined value — always guard with `i < q.size() =>`.
6. `fair` = weak fairness: an infinite execution in which a fair instance that is
   enabled throughout the loop is never executed is excluded from leadsTo
   counterexamples.
7. `leadsTo ... decreases M` under `verify --engine induction` proves an
   unbounded response when, under the proved invariants and while P holds and Q is
   false, M is non-negative, some action is enabled, and every enabled action
   either makes Q true or keeps P true while strictly decreasing M. Without
   `decreases`, leadsTo remains bounded to `--depth`.
8. `symmetric type` / `symmetric enum` means those values are interchangeable
   entity identities. For leadsTo lasso/stall search, fslc symmetry-breaks the
   representative state using canonical rows from `Map<SymmetricType, V>` and
   `Set<SymmetricType>` state (`V` is used only when it contains no symmetric
   identity type); use it only when no identity is semantically special.

## 6. Automatic checks (checked even if not written)

Type bounds (`_bounds_<var>`, including Map values, struct fields, and the Seq live
prefix) / partial operations (`_partial_<action>`, Seq pop/head/at and nonzero
divisor) / action coverage (+ unsat-core diagnostics) / deadlock (warning, with
state, `deadlock reachable at step N (state: …)`, violated under
`--deadlock error`) / leadsTo (lasso + stall).
An **intended terminal state** (processing complete, etc. — a state where stopping
is correct) is declared with `terminal { <predicate> }` — a stop satisfying the
predicate is excluded from the deadlock check, while other unexpected deadlocks
continue to be detected (more precise than `--deadlock ignore`, which uniformly
ignores all stops). vacuity is a warning only on the verified/proved path:
an unreached antecedent of an implication invariant (`vacuous_implication`), an
unreached leadsTo trigger (`vacuous_leadsto`), a requires clause always true under
the context of the preceding requires (`always_true_requires` — actions with
coverage false and compose synchronized actions are excluded; a synchronized
action's clauses are inherited copies from its components and are checked by
verifying the component spec on its own), and **an invariant that depends only on a
frozen state variable no action ever assigns to and is dynamically always true**
(`tautology_over_frozen` — a dead ghost; make it `const`, or suspect a missing
action that should change it), and a generated deadline `tick` proven dead because
urgency freezes time (`urgency_freeze`). `--vacuity error` gives
`result:"error"`; `--vacuity ignore` disables it.

## 7. CLI and JSON essentials

```
fslc check <f>                                  # syntax / names / types only
fslc verify <f> [--depth K=8] [--engine bmc|induction] [--k N=1]
               [--deadlock warn|error|ignore] [--vacuity warn|error|ignore]
               [--property <Name>]                  # check one named property in isolation
                                                    #   (invariant / trans / leadsTo / reachable)
               [--exclude-property <Name>]...       # skip named invariant/trans/leadsTo/reachable
               [--strict-tags] [--requirements ids.txt]
fslc explain <f> [--depth K=8] [--readable]    # JSON by default; --readable emits a text review view
fslc mutate <f> [--depth K=8] [--by-requirement] [--max-mutants N=200]
fslc scenarios <f> [--depth K]                  # reach_* / cover_* / respond_* / deadlock_terminal
fslc replay <f> --trace <events.json>           # conformant | nonconformant
fslc testgen <f> [--depth K] [--strict] [--target pytest|vitest|swift|kotlin|dart|phpunit] [-o out]  # Adapter skeleton + conformance tests (pytest default / Vitest / Swift Testing / kotlin.test / package:test / PHPUnit)
fslc refine <impl> <abs> <mapping> [--depth K]  # refines | refinement_failed
fslc chain [fsl-project.toml] [--keep-going]     # manifest-driven business -> req -> design -> impl table + JSON
fslc typestate <f> [--ts]                       # state machine -> ghost-type applicability + TS skeleton
```

`chain` reads `fsl-project.toml` by default. Each `[business]`,
`[requirements]`, and `[design]` table has `file = "..."`; adding `depth = K`
runs `verify`, while omitting `depth` runs `check`. A layer with
`refine_against = "requirements"` must also set `mapping = "..."`. `[impl]`
runs its shell `command` from the manifest directory. JSON is stdout; the
consolidated table is stderr. Without `--keep-going`, execution stops after the
first failed layer and later layers are marked `skipped`.

- `mutate` applies a deterministic single mutation to the kernel AST (requires
  deletion/negation, assignment deletion, enum swap, integer/type-bound ±1,
  then/else swap, fair deletion), re-runs `build_spec` on each mutant, and reports
  whether it is killed by BMC/acceptance/forbidden/refinement. exit is always 0.
  A survivor is not a failure but an equivalent mutant or a review candidate for
  under-constraint. If the baseline is not clean at depth K, no mutation is done and
  the baseline result is returned. `--by-requirement` aggregates by the requirement
  tag of the "killed property" and warns on zero kills as `empty_formalization`
  (a lower bound observed for this mutant set and depth).
- `verify --property Name` resolves across invariant, `trans`, `leadsTo`, and
  `reachable` declarations and checks only the named property kind in isolation.
  `--exclude-property Name` is repeatable and acts as the cross-kind inverse:
  it removes named invariants, `trans`, `leadsTo`, and `reachable` checks from
  the run and from checked-property outputs. If both options name the same
  property, exclusion wins.
- `explain` is deterministic formatting with no LLM. JSON mode enumerates
  state/action/requires/writes/properties/implicit checks by source loc and
  structural traversal, and attaches to each user invariant the shortest
  counterfactual trace that breaks it under requires/assignment/fair removal.
  `--readable` emits a text view that surfaces verification bounds, fairness,
  KPI projections, branch lowering, and synthesized refinement mappings.
  Invariants for which none is found are explicitly marked
  `no counterfactual within depth K`.
- `--strict-tags` on `check` / `verify` adds traceability warnings only to
  ok/verified/proved success results. The targets are untagged
  action/invariant/trans/reachable/leadsTo, and IDs declared via
  `--requirements ids.txt` or a `requirement` block in the requirements dialect but
  never referenced. A declaration with a tag such as `MODEL: ...` / `ASSUME-n: ...`
  does not become a warning.
- `typestate`: determines how far a state machine (a struct field with enum values /
  a state variable / an `Option<_>` slot) can be mapped onto the host language's
  **typestate (ghost types)**. Each action is classified as
  `derivable` (the from-state is the entity's own local guard) /
  `branching` (data-dependent inside an `if`) /
  `relational` (no local guard, the premise lives in an external structure — cannot
  be expressed in the type and remains a runtime/verification obligation).
  An entity's `applicability` is `full` only when all transitions are
  derivable/branching. `relational` ones carry a reason (diagnostics) and a
  requirement ID. `--ts` outputs only the TypeScript for the derivable portion.
- Counterexample trace: `[{step, state, action{name,params,loc}, changes{path:{from,to}}}]`.
  Shortest guaranteed. State is the logical representation (enum name / Option as
  null|value / Seq as an array / composition as `alias.var` keys). Internal names
  (`__`) do not appear.
- `unknown_cti`: `cti.states` (k+1 states) + `violated_at`. The starting state is an
  unreachable phantom — add an auxiliary invariant to exclude it.
- `verified` / `reachable_failed` / `violated` from BMC are bounded and include
  `completeness:"bounded"`, `checked_to_depth`, and `cost: {"elapsed_s": ...}`.
  Bounded `verified` may include a saturation `hint` when the depth-K frontier
  first witnesses a reachable/vacuity/coverage fact during normal exploration.
- `proved`: `completeness:"unbounded"`, `checked_to_depth` (the base BMC depth),
  `cost`, and `k_used` (the k used per invariant); reachables/coverage come from
  the base case. Ranked leadsTo entries add
  `{proved: true, completeness: "unbounded", proof: "ranking", decreases: ...}`.
- `reachable_failed`: each `unreached[]` has `classification`:
  `insufficient_depth` (target satisfiable as a state predicate, no witness by K)
  or `over_constrained` (target unsat under type bounds/invariants, with
  `blocking_requires` naming the blocking core).
- faithfulness diagnostics may add `faithfulness_class` and
  `recommended_action`: `partial_op_unguarded`, `frozen_only_invariant`,
  `intent_unexercised`, or `liveness_not_refined`.
- coverage diagnostic:
  `{covered: false, name, display_name?, blocking_requires: [{loc, text}], hint}`.
- leadsTo violation: `pending_since` + `loop_start` (lasso) or `stutter: true`.
- progress-preserving refinement failure: `refinement_failed`,
  `kind:"progress_lost"`, `violation_kind:"leadsTo"`, `impl_trace`,
  `progress:{leadsTo, actions}`, and `faithfulness_class:"liveness_not_refined"`.
- leadsTo ranking failure: `unknown_cti` / `violation_kind:"leadsTo_rank"` with
  `rank_failure` (`unbounded_below`, `deadlock`, `non_decreasing_action`, or
  `pending_not_preserved`).

### ⚠ Liveness scales differently from safety — verify it on a reduced model

`leadsTo` is a lasso search: the cost grows roughly **exponentially in the number
of concurrent entities** (the textbook BMC-liveness state explosion), because each
added entity multiplies the interleavings the loop search must consider. Safety
(`invariant` / `trans` / `reachable`) does **not** behave this way — it stays cheap
even at large depth. Observed shape: a single entity verifies in seconds even at
depth 16, but three concurrent entities with `leadsTo` can blow past minutes by
depth ~12. This is a known limit, not a pathological encoding.

Practical strategy:
- Verify **liveness on the smallest model that still exhibits the interleaving** —
  shrink the entity-count range (e.g. `0..1` instead of `0..3`) and use a shallow
  `--depth`. One entity is often enough to find a real `leadsTo` bug.
- Verify **safety separately on the full-size model** at the depth you need.
- Use `--property <leadsToName>` to run a single liveness property in isolation
  while iterating (see §7), so a slow `leadsTo` does not gate the safety checks.

## 8. Idioms (reuse them as-is)

```fsl
// stock-decrement guard (prevents type_bound)
requires stock[i] > 0
// extract from an Option and compare
requires cart[u] is some(i)
requires stock[i] > 0
// queue processing (two forms that prevent partial_op)
action take()  { requires q.size() > 0  x = q.head()  q = q.pop() }
action drain() { if q.size() > 0 { x = q.head()  q = q.pop() } }
// invariant talking about a Seq (index guard, range derived from const)
invariant I { forall i in 0..CAP-1 { i < q.size() => jobs[q.at(i)].st == Queued } }
// folding a Seq (index domain type)
type Idx = 0..3
invariant B { balance == sum(i: Idx of log.at(i) where i < log.size()) }
// 2D data: Maps cannot be nested -> flatten into a single product domain and recover the axes with / %
const SLOTS = 4
type Cell = 0..ROOMS*SLOTS-1               // the type's upper bound can be a constant expression
state { holder: Map<Cell, Option<UserId>> }
reachable Room1Full { forall c: Cell { c / SLOTS == 1 => holder[c] != none } }
// history ("ever did X") is a ghost variable
state { ever_locked: Map<UserId, Bool> }   // set to true on lock
// duplicate-free queue (the classic auxiliary invariant for induction proofs)
invariant NoDup { forall i in 0..CAP-1 { forall j in 0..CAP-1 {
  (i < j and j < q.size()) => not (q.at(i) == q.at(j)) } } }
// state-tag-dependent refinement mapping (mapping file only)
map seats[s: SeatId] = if slots[s].st == Sold then slots[s].holder else none
```

## 9. Implementation connection (the testgen Adapter contract)

Wire the generated file's `Adapter` to the implementation:
- `reset()`: bring the implementation to the same initial state as init
- `step(action, params)`: execute one action (in composition, `"alias.action"` names
  also arrive)
- `observe() -> dict`: project the implementation state onto the spec's logical-state
  form (keys are state-variable names / composition uses `alias.var`; enum = name
  string, Option = None|value, Seq = list, Map = dict with string keys, struct = dict)

The random-walk test uses the Monitor (the spec's concrete interpreter) as the
oracle, stepping through the implementation one step at a time. A failure = a
divergence between implementation and spec (read the trace to decide which one is
correct).

`--target` chooses the harness; the scenario-collection core is shared, so both
emit the same scenarios:
- `pytest` (default): Python tests; the random walk imports `fslc.runtime.Monitor`
  and runs the fixed-seed walk live as the oracle. Output defaults to `test_<spec>.py`.
- `vitest`: a self-contained TypeScript (Vitest) file with the same `Adapter`
  contract (`reset`/`step`/`observe`). Deterministic and forbidden scenarios map
  directly; the random walk is **baked at generation time** (the Python Monitor
  runs the seed-fixed walk and the `(action, params, expected_state)` trace is
  embedded as a static fixture), so the tests need no `fslc`/Python at runtime.
  Until `makeAdapter()` is wired the suite is skipped. Output defaults to
  `<spec>.test.ts`.
- `swift`: a self-contained Swift Testing file (`import Testing` / `@Test` /
  `#expect`; not XCTest), same `Adapter` contract and same baked walk. Dynamic
  state is `[String: Any]` with a bundled deep-equality + partial-match helper;
  Option `None` bakes as the `FSLNull.instance` sentinel (no Foundation). Tests
  are disabled via `@Test(.enabled(if: isAdapterWired()))` until `makeAdapter()`
  is wired. Output defaults to `<SpecName>ConformanceTests.swift`.
- `kotlin`: a self-contained kotlin.test file (multiplatform; JVM delegates to
  JUnit), same `Adapter` contract and same baked walk. Dynamic state is
  `Map<String, Any?>` — Kotlin's `==` is deep on `List`/`Map` and distinguishes
  `Int`/`Double`, so the partial-match helper is a plain recursion. No portable
  runtime skip, so an unwired `makeAdapter()` returns `null` and each test
  returns early. Output defaults to `<SpecName>ConformanceTest.kt`.
- `dart`: a self-contained `package:test` file (also runs under `flutter test`),
  same `Adapter` contract and same baked walk. Dynamic state is
  `Map<String, dynamic>`; Dart's `==` is reference-based on collections, so
  `assertPartial` recurses by the expected keys and compares leaves with the
  `equals` matcher (the only dependency stays `package:test`). A top-level probe
  sets `skip:` on each `test()` until `makeAdapter()` is wired. Output defaults
  to `<spec_name>_conformance_test.dart`.
- `phpunit`: a self-contained PHPUnit file (PHP 8.1+ / PHPUnit 10+,
  `strict_types`), same `Adapter` contract and same baked walk. Dynamic state is
  an associative `array`; leaves compare with `assertSame` (`===`) so int/float,
  bool and null never coerce (loose `==` would conflate `0 == "0"`).
  `assertPartial` recurses by the expected keys (maps order-independent; lists
  pin length). `setUp()` skips every test until `makeAdapter()` is wired. Output
  defaults to `<SpecName>ConformanceTest.php`.

If a `reachable` target is not witnessed at the requested depth, `testgen` still
generates tests for the scenarios it did witness and returns `warnings[]` with a
message such as `reachable SoldOut not witnessed at depth 3; try --depth >= 4`.
Use `--strict` to restore all-or-nothing `reachable_failed`.

## 10. Three-layer dialects (consulting / requirements / design)

The layers chain via refinement: business ⊒ requirements ⊒ design ⊒ implementation
(testgen/replay). Every dialect expands as AST into the kernel, so all the commands
in §7 work as-is.

### Declaration tags (common to all layers)

`"ID: source"` immediately before the `{` of an invariant / trans / reachable /
leadsTo / action:
`invariant PaidLedger "REQ-3: ledger consistency" { ... }` →
`requirement: {id, text}` in violated / unknown_cti / coverage diagnostic / scenarios.

### Authoring specs as readable documentation (requirements + design)

The spec source IS the documentation: a rule you can read is also the rule that is
verified, so it never drifts. In the requirements and design (kernel) layers:

1. **Tag every invariant/action/property** with `"ID: one-sentence intent"` — the only
   in-source prose that flows into all output (explain / html / counterexamples). It is
   NOT verified, so keep it a faithful paraphrase of the expression, not a rival truth.
2. **Quarantine verification scaffolding by ID prefix.** Domain rules: `REQ-…` / `INV-…`.
   Verification-only artifacts (k-induction CTI auxiliary invariants, ghost-counter
   relations): `MODEL-…` / `ASSUME-…`, so a reader can skip them.
3. **Prefer member-quantification** `forall x in coll { P(x) }` over the index idiom
   `forall i in 0..N { i < coll.size() => P(coll.at(i)) }` — but ONLY (a) in expression
   position (invariant/property bodies; NOT action/init `forall` *statements*, which
   reject collection binders) and (b) for element-wise properties. Keep explicit indices
   for position, ordering, adjacency, or no-duplicates.
4. **Separate domain from verification bound.** Declare `entity X` / `number X` and put
   sizes in `verify { instances/values }` instead of `type X = lo..hi`. Allowed in a
   kernel `spec` too (desugars to `type`), so `type Claim = 0..2` no longer has to read
   as a false domain fact.
5. **Multi-line transitions** (requirements): `with` / `when` / `set` / `covers` each on
   their own indented line.
6. **Order:** domain content first, proof scaffolding last.

`fslc explain --readable` then renders the whole spec (state, tagged actions,
properties) as a structured digest — a view of the source, not a separate document.

### business (the consulting layer)

```fsl
business ReturnHandling {
  actor Customer, Manager            // roster (validates `by`)
  entity Return                      // identity sort; size set by verify below
  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager   // -> fair action approve(c: Return)
    transition reject  Requested -> Rejected by Manager
    transition refund  Approved  -> Refunded by Manager
  }
  kpi refunded = count Return in Refunded     // -> metadata projection count(c: Return where stage(c) == Refunded)

  control CTRL-DECISION "Every return must preserve adjudication control"
    owner Manager
    severity high
    applies_to Return

  policy PAY-2 "every request is adjudicated"
    satisfies CTRL-DECISION
    every Return in Requested must eventually be Approved or Rejected or Refunded
  goal AllSettled "all cases can be settled"
    all Return can be Refunded or Rejected
}

verify {
  instances Return = 3
}
```

`stage(c)` expands from the type of the bound c into the process's state Map
(`return_stage[c]`).
The natural business forms above are aliases for `responds { forall ... ~> ... }`
and `goal { forall/exists ... }`; the explicit expression forms remain available
for policies that cannot be written as a simple stage progression.

`control` declarations are metadata only. Attach them to checkable business
rules with `policy ... satisfies CTRL` or `goal ... satisfies CTRL`. Unknown
control references are type errors, unused declared controls are warnings, and a
violated satisfied policy/goal reports `requirement.controls` in JSON.

For cross-business or enterprise-level controls, use a standalone `governance`
catalog. `fslc check governance.fsl` verifies that each delegated business spec
exists, each `require CTRL` is satisfied by business-side `satisfies` metadata or
an explicit `CTRL is satisfied_by policy|goal ID` mapping, and each
`preservation` block runs its declared refinement at depth 8.

### requirements (the requirements layer)

```fsl
requirements ExpenseRequirements {
  implements ExpenseToBe from "1_business.fsl" { }

  number Amount
  const AUTO_LIMIT = 1

  process Claim with amount: Amount {
    stages Draft, Submitted, Approved, Rejected, Paid
    initial Draft
    transition submit       Draft     -> Submitted by Employee with a: Amount when a > 0 set amount = a covers REQ-1 "The applicant submits an expense claim by entering an amount"
    transition auto_approve Submitted -> Approved  by System  when amount <= AUTO_LIMIT covers REQ-2 "Claims at or below AUTO_LIMIT are auto-approved by the system"
    transition mgr_approve  Submitted -> Approved  by Manager when amount >  AUTO_LIMIT covers REQ-3 "Claims above AUTO_LIMIT are approved by a manager"
    transition reject       Submitted -> Rejected  by Manager when amount >  AUTO_LIMIT covers REQ-3 "Claims above AUTO_LIMIT may be rejected by a manager"
    transition pay          Approved  -> Paid      by Finance covers REQ-4 "Only approved claims are paid"
  }

  kpi paid_claims = count Claim in Paid

  acceptance AC-1 "Approval flow: a low-amount claim is auto-approved and paid" {
    submit(0, 1) auto_approve(0) pay(0)
    expect Claim 0 in Paid
  }
  acceptance AC-2 "Rejection flow: a high-amount claim ends in manager rejection" {
    submit(1, 2) reject(1)
    expect Claim 1 in Rejected
  }
}
verify {
  instances Claim = 3
  values Amount = 0..3
}
```

- The process+data profile is the default requirements form for a single-entity
  lifecycle. `process E with f: T { ... }` creates the entity stage map and
  carried fields; transition clauses add input (`with a: T`), guards (`when`),
  field updates (`set f = expr`), and traceability (`covers REQ-n "text"`).
- `kpi NAME = count ENTITY in STAGE` is a declarative projection in both
  business and requirements; it does not create a ghost counter or an automatic
  `_kpi_*` invariant.
- When `implements Abs from "file" { }` is present and process/action/stage names
  match, fslc synthesizes the identity refinement mapping. `maps auto` is also
  allowed inside an `implements` block for same-name kernel-wrapper state/actions,
  and explicit `map` / action correspondences override it. Auto-mapped process
  transitions are statically actor-checked; an actor mismatch is a check-time
  type error.
- `acceptance` is replay-checked at check time with the concrete Monitor (failure is
  `kind: "acceptance"`). It supports the readable stage form
  `expect <Entity> <id> in <Stage>` alongside `expect <expr>`, is output to
  scenarios as `acceptance_<ID>`, and flows to testgen.
- `forbidden FB-1 "source" { <steps> expect rejected }` is must-forbid (the dual of
  acceptance). The premise steps (all but the last) are all ok, and it succeeds if
  **the last step is rejected** (not-enabled, or an
  invariant/type_bound/partial_op/ensures violation). If accepted,
  `kind: "forbidden"` (detection of under-constraint = a missing guard that a safety
  invariant stays silent about); if the premise is not enabled,
  `kind: "forbidden_setup"`. Output to scenarios as `forbidden_<ID>` (with
  `rejected_by` — anything other than `requires_failed` means the spec itself is a
  verify violation).
- The kernel-wrapper form remains for hard cases: multi-entity requirements,
  conservation rules, SLA/time, history that is not expressible as a carried
  field, or any behavior that needs explicit kernel state. In that form, use
  kernel `struct` / `state` / `init`, `fair action`, `branches`, and explicit
  `maps` where needed. The display of a branches split action is
  `submit[a <= AUTO_LIMIT]`; diagnostics keep the internal name (`submit__b1`)
  and add `display_name`.
- Elements inside a requirement automatically get {id, text} metadata.

### Drawing the layer boundary

The majority of NFRs are handled (§11). What stays outside FSL: probabilities,
percentiles, real time (wall-clock ms), usability, and prose rationale (write those
in each layer's documents).

## 11. Non-functional requirements (NFR)

| NFR | How to write it |
|---|---|
| Permissions | role check in requires + ghost invariant |
| Audit completeness | cross-cutting invariant (the bank_system pattern) |
| Capacity | bounded types, Seq capacity, count invariant |
| Reliability behavior | fault-injection action + mode state + fair recover + recovery leadsTo |
| SLA/timeout | requirements `time { urgent ...  age m[x: T] while P }` + `deadline m <= K` |
| Probability/%/real time | out of scope (put in documents) |

### time / deadline rules (placement, semantics)

- **Placement**: `time { ... }` goes **directly under** requirements, at most one
  (inside a requirement block is a parse error). `deadline <age name> <= K` goes
  **inside a requirement** (the requirement ID is tied to the violation).
- **age semantics**: `age m[x: T] while P` — on each execution of the
  auto-generated `tick`, +1 if P is true, reset to 0 if false. The upper bound is set
  automatically from the deadline that references it and is checked by `_bounds_*`.
  **age is readable from guards as an ordinary state variable** (`requires m[c] >= K`).
- **urgent semantics = time freeze**: while any of the listed actions is enabled,
  `tick` cannot fire.

### ⚠ The vacuous-SLA trap and the deadline-urgency pattern

If you make an action that can be enabled at all times (e.g. the response itself)
`urgent`, **time never advances at all and the deadline is vacuously verified for
any K** (even `deadline <= 0` is green). `fslc verify --vacuity` emits
`kind:"urgency_freeze"` when this freeze is proven by the generated `tick` guard
being initial and inductive. The correct form is to **make only a guarded action
that becomes enabled only at the deadline `urgent`**:

```fsl
time {
  urgent respond_due                       // <- make only the deadline-reached handler urgent
  age resp_age[c: CaseId] while cases[c] == Accepted
}
requirement REQ-3 "first response within 3 ticks of acceptance" {
  fair action respond_due(c: CaseId) {
    requires cases[c] == Accepted
    requires resp_age[c] >= SLA_TICKS      // enabled only at the deadline = time flows until then
    cases[c] = Responded
  }
  deadline resp_age <= SLA_TICKS
}
```

How to confirm non-vacuity: change to `deadline <= K-1` and confirm it becomes
violated (evidence the boundary bites exactly). Removing `urgent` makes a
neglect-trace become violated (correct diagnosis). BMC works immediately. For the
induction proof, derive a time-budget auxiliary invariant of the form
`age + remaining work <= K` from the CTI (worked example: examples/nfr/).
