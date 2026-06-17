# FSL Language Reference Card (complete, condensed)

Read this entire file before writing a spec. This is the full syntax and full set
of rules as of v2.x.

## 1. Top-level structure

```fsl
spec <Name> {
  const <NAME> = <const expr>             // integer constant (expressions allowed: CAP - 1, etc.)
  type  <Name> = <lo>..<hi>               // domain type (bounded integer)
  enum  <Name> { <Member>, ... }
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
  leadsTo   <Name> { <expr> ~> <expr> }    // may be nested under an outer forall x: T { … }
  terminal  { <expr> }                     // intended terminal state (excluded from the deadlock check)
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
  map <abs_var> = <expr over impl state>          // scalar abstract variable
  map <abs_var>[<x>: <KeyType>] = <expr>          // per-element mapping of a Map
  // conditional expressions allowed only inside mapping/argument expressions: if <c> then <a> else <b> (else required)
  action <impl_act>(<formal params>...) -> <abs_act>(<expr>...) | stutter
  // formal params may be bare names or name: Type annotations matching the impl action
}
```

## 2. Types

| Type | How to write | Notes |
|---|---|---|
| Int / Bool | `n: Int` | Int is unbounded |
| Domain type | `type Qty = 0..5` | **automatic bound check** (violated/type_bound) |
| enum | `enum St { A, B }` | members are referenced and displayed by bare name |
| struct | `struct S { f: Qty, o: Option<K> }` | field = scalar or Option<scalar> only |
| Option<T> | `c: Option<ItemId>` | T is a scalar. `none` / `some(e)` |
| Map<K, V> | `m: Map<ItemId, Qty>` | K is a bounded scalar (Int keys give a deprecation warning) |
| Set<T> | `s: Set<OrderId>` | T is a bounded scalar |
| Seq<T, N> | `q: Seq<JobId, CAP>` | T is a scalar, N is a positive constant. FIFO |

Scalar = Int / Bool / domain type / enum.
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
  allowed), and the v0 form `forall i in lo..hi: expr` (range is a constant
  expression: `0..CAP-1` recommended)
- Aggregation: `count(x: T where expr)`, `sum(x: T of expr [where expr])`
- Option: `x == none` `x != none` `x is some(v)` (v is usable afterward within that
  formula). **`x == some(e)` and arithmetic/ordering on Option are type errors**
- struct: literal `S { f: 0, o: none }`, `s.f`, `==` (field-wise equality; for an
  Option field, presence matches ∧ present ⇒ values match)
- Set: `Set {}` `Set { 1, 2 }`, `.add(e) .remove(e) .contains(e) .size()`
- Seq: `Seq {}` `Seq { 1, 2 }` (element count ≤ N), `.push(e) .pop() .head() .at(i)
  .contains(e) .size()`, `==` (length + all elements)
- ensures/trans only: `old(expr)` / leadsTo only: `P ~> Q` / mapping-expression
  only: `if c then a else b`

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
action that should change it). `--vacuity error` gives `result:"error"`;
`--vacuity ignore` disables it.

## 7. CLI and JSON essentials

```
fslc check <f>                                  # syntax / names / types only
fslc verify <f> [--depth K=8] [--engine bmc|induction] [--k N=1]
               [--deadlock warn|error|ignore] [--vacuity warn|error|ignore]
               [--property <Name>]                  # check a single invariant only (for probing)
               [--strict-tags] [--requirements ids.txt]
fslc explain <f> [--depth K=8]                 # skeleton + counterfactual + witness narration
fslc mutate <f> [--depth K=8] [--by-requirement] [--max-mutants N=200]
fslc scenarios <f> [--depth K]                  # reach_* / cover_* / respond_* / deadlock_terminal
fslc replay <f> --trace <events.json>           # conformant | nonconformant
fslc testgen <f> [--depth K] [-o out.py]        # Adapter skeleton + conformance pytest
fslc refine <impl> <abs> <mapping> [--depth K]  # refines | refinement_failed
fslc typestate <f> [--ts]                       # state machine -> ghost-type applicability + TS skeleton
```

- `mutate` applies a deterministic single mutation to the kernel AST (requires
  deletion/negation, assignment deletion, enum swap, integer/type-bound ±1,
  then/else swap, fair deletion), re-runs `build_spec` on each mutant, and reports
  whether it is killed by BMC/acceptance/forbidden/refinement. exit is always 0.
  A survivor is not a failure but an equivalent mutant or a review candidate for
  under-constraint. If the baseline is not clean at depth K, no mutation is done and
  the baseline result is returned. `--by-requirement` aggregates by the requirement
  tag of the "killed property" and warns on zero kills as `empty_formalization`
  (a lower bound observed for this mutant set and depth).
- `explain` is deterministic formatting with no LLM. It enumerates
  state/action/requires/writes/properties/implicit checks by source loc and
  structural traversal, and attaches to each user invariant the shortest
  counterfactual trace that breaks it under requires/assignment/fair removal.
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
- `proved`: `k_used` (the k used per invariant); reachables/coverage come from the
  base case.
- coverage diagnostic: `{covered: false, blocking_requires: [{loc, text}], hint}`.
- leadsTo violation: `pending_since` + `loop_start` (lasso) or `stutter: true`.

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

## 10. Three-layer dialects (consulting / requirements / design)

The layers chain via refinement: business ⊒ requirements ⊒ design ⊒ implementation
(testgen/replay). Every dialect expands as AST into the kernel, so all the commands
in §7 work as-is.

### Declaration tags (common to all layers)

`"ID: source"` immediately before the `{` of an invariant / trans / reachable /
leadsTo / action:
`invariant PaidLedger "REQ-3: ledger consistency" { ... }` →
`requirement: {id, text}` in violated / unknown_cti / coverage diagnostic / scenarios.

### business (the consulting layer)

```fsl
business ReturnHandling {
  actor Customer, Manager            // roster (validates `by`)
  case Return = 0..2                 // -> domain type
  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager   // -> fair action approve(c: Return)
    transition reject  Requested -> Rejected by Manager
    transition refund  Approved  -> Refunded by Manager
  }
  kpi refunded counts Return in Refunded      // -> ghost + auto-consistency invariant (+1 on an inflow transition; an outflow transition is a type error)
  policy PAY-2 "every request is adjudicated"
    every Return in Requested must eventually be Approved or Rejected or Refunded
  goal AllSettled "all cases can be settled"
    all Return can be Refunded or Rejected
}
```

`stage(c)` expands from the type of the bound c into the process's state Map
(`return_stage[c]`).
The natural business forms above are aliases for `responds { forall ... ~> ... }`
and `goal { forall/exists ... }`; the explicit expression forms remain available
for policies that cannot be written as a simple stage progression.

### requirements (the requirements layer)

```fsl
requirements ReturnSystemReq {
  implements ReturnHandling from "return_policy.fsl" {   // mapping to the upper layer (optional)
    map return_stage[c: CaseId] = if sys[c].st == New then Requested else ...
    map refunded = paid_count
  }
  // types / state / init are kernel syntax as-is
  requirement REQ-1 "amounts at or below the threshold are auto-approved" {
    fair action submit(c: CaseId, a: Amount) {
      requires sys[c].st == New
      requires a > 0
      branches {                                          // data-dependent branch correspondence, auto-split
        when a <= AUTO_LIMIT { sys[c] = ... } maps approve(c)
        when a > AUTO_LIMIT  { sys[c] = ... } maps stutter
      }
    }
  }
  requirement REQ-3 "payment only after approval" {
    fair action pay(c: CaseId) maps refund(c) { ... }     // maps = correspondence to an upper-layer action
    fair action tick() maps stutter { ... }               // action-level internal/stutter correspondence
    invariant PaidLedger { ... }
  }
  acceptance AC-1 "small amounts get paid" { submit(0, 1)  pay(0)  expect sys[0].st == Paid }
  forbidden FB-1 "cancellation after shipping is rejected" { submit(0, 1)  ship(0)  cancel(0)  expect rejected }
}
```

- When `implements` is present, `fslc verify` simultaneously runs the upper-layer
  refine → `implements: {abs, result}` in the result JSON.
- `maps` may be on each branch or directly on an unbranched action. The
  declaration-level form `fair action tick() maps stutter { ... }` parses and
  refines as an internal upper-layer step; a repro with `map x = y` and
  `tick { y = y }` returned `result:"ok"` / `implements.result:"refines"` from
  `fslc check`, and `result:"verified"` with the same implements result from
  `fslc verify --depth 1`.
- `acceptance` is replay-checked at check time with the concrete Monitor (failure is
  `kind: "acceptance"`). It is output to scenarios as `acceptance_<ID>` and flows to
  testgen.
- `forbidden FB-1 "source" { <steps> expect rejected }` is must-forbid (the dual of
  acceptance). The premise steps (all but the last) are all ok, and it succeeds if
  **the last step is rejected** (not-enabled, or an
  invariant/type_bound/partial_op/ensures violation). If accepted,
  `kind: "forbidden"` (detection of under-constraint = a missing guard that a safety
  invariant stays silent about); if the premise is not enabled,
  `kind: "forbidden_setup"`. Output to scenarios as `forbidden_<ID>` (with
  `rejected_by` — anything other than `requires_failed` means the spec itself is a
  verify violation).
- The display of a branches split action is `submit[a <= AUTO_LIMIT]`, but **when
  referenced from a downstream refinement mapping use the internal names
  `submit__b1`/`submit__b2`** (a current limitation).
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
any K** (even `deadline <= 0` is green). The correct form is to **make only a
guarded action that becomes enabled only at the deadline `urgent`**:

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
