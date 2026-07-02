# FSL Language Reference

FSL is a formal specification language for application development whose primary
design goal is to be **written, verified, and repaired by generative AI**. This
document is the language reference you consult when writing specifications (it
always tracks the latest implementation). The background to design decisions and
the implementation design of each feature can be reached from
[`README.md`](README.md) (the docs map).

## Design principles

| Principle | Existing languages (TLA+/Alloy) | FSL |
|---|---|---|
| Syntax | Mathematical notation (∀, □, ◇) | TypeScript/Python-like. Stays close to the LLM training distribution |
| Counterexamples | Human-oriented text | **Structured JSON** (with state diffs and the violated bound variables) |
| Errors | Human-oriented messages | Machine-readable (line, column, classification, repair hint) |
| Verification | Full verification assumed | Bounded and fast by default. **Unbounded-depth proofs via k-induction** are also possible |
| Vacuity | Found by an expert's intuition | Action reachability + **unsat-core diagnosis of blocking requires** |
| Pitfalls | Avoided by discipline | **Structurally eliminated** (automatic bounds checks, implicit checking of partial operations) |

## 1. Structure of a specification

```fsl
spec <Name> {
  const <NAME> = <constant expr>
  type  <Name> = <lo>..<hi>            // domain type (bounded integer)
  symmetric type <Name> = <lo>..<hi>   // domain whose values are interchangeable identities
  enum  <Name> { <Member>, ... }
  symmetric enum <Name> { <Member>, ... }
  struct <Name> { <field>: <scalar type | Option<scalar type>>, ... }

  state { <var>: <type>, ... }
  init  { <stmt>... }

  [fair] action <name>(<p>: <type name>, ...) {
    requires <expr>                     // guard. multiple allowed (conjunction)
    let <x> = <expr>                    // local binding
    <stmt>...                           // assignment / if-else / forall
    ensures <expr>                      // postcondition. reference the old state with old(expr)
  }

  invariant <Name> { <expr> }           // holds in all reachable states (safety)
  trans <Name> { <expr> }               // holds across all reachable transitions (two-state safety)
  reachable <Name> { <expr> }           // is reachable (returns a witness)
  leadsTo <Name> { <response property> }// bounded response, or ranked induction with decreases (see §1)
  terminal { <expr> }                   // intended terminal states (excluded from deadlock checking)
}
```

Any layer — including a kernel `spec` — may declare identity/number sorts whose
finite sizes come from a sibling top-level `verify` block instead of an inline
`type X = lo..hi` range. This keeps the domain declaration (what exists) separate
from the verification world size (how much is checked):

```fsl
spec <Name> {
  entity <Entity>          // identity sort; size from verify { instances <Entity> = N }
  number <Number>          // numeric sort; range from verify { values <Number> = lo..hi }
}
business <Name> {
  entity <Entity>
}
requirements <Name> {
  entity <Entity>          // optional explicit identity sort
  number <Number>
  process <Entity> with f: <Number> { ... }  // process also declares the entity kind
}
verify {
  instances <Entity> = <N>
  values <Number> = <lo>..<hi>
}
```

`entity`/`number` desugar to `type <Name> = lo..hi` before verification, so they
are exactly equivalent to writing the bounded type directly — the difference is
only readability (a design spec reads as documentation instead of asserting a
domain size that is really a model bound). See `docs/DESIGN-spec-domains.md`.

`fair` is a weak-fairness annotation: if that action instance remains
continuously enabled, the assumption is that it will eventually be executed.

The hierarchy of properties: `invariant` is one-state safety, `trans` is
two-state safety (the pre-transition state can be referenced with `old()`), and
`leadsTo` is response liveness. Without a ranking function, `leadsTo` is checked
boundedly. With `--engine induction` and `decreases <int expr>`, a `leadsTo` can
be proved unbounded by a well-founded ranking argument.

Response properties inside a `leadsTo` block:

```fsl
leadsTo <Name> {
  <expr> ~> <expr>                      // once P holds (including the same instant), Q eventually holds
  <expr> ~> within K <expr>             // Q must hold within K steps after P
  forall x: T { <expr> ~> <expr> }      // checked independently per binding (only an outer forall may nest)
  decreases <int expr>                  // optional; induction-only ranking measure
}
```

`~>` is **exclusive to leadsTo blocks** — it cannot be used in general expressions.
`within K` is a bounded deadline on a response; `K` must be a non-negative
constant expression. It is checked by BMC, not by the ranked induction proof.
`decreases` is optional and must be an integer-valued expression. Under
`fslc verify --engine induction`, the verifier proves the ranked response by
checking, under the proved invariants, that whenever `P` is pending and `Q` is
false: the measure is non-negative; some action is enabled; and every enabled
action either makes `Q` true or keeps `P` true while strictly decreasing the
measure. Ranked proof success is independent of `--depth`; `--depth` is still
used for the base BMC check and reachable/coverage evidence.

## 2. Types

| Type | Example | Description |
|---|---|---|
| `Int` / `Bool` | `count: Int` | Unbounded integer / boolean |
| Domain type | `type Qty = 0..5` | Bounded integer. **The range is checked automatically** (§6) |
| Inline state domain | `state { qty: 0..5 }` | Shorthand for a named domain type in a state-variable declaration |
| Entity kind | `entity Claim` / `process Claim ...` | Finite identity sort. Allowed in any layer incl. kernel `spec`; size set by `verify { instances Claim = N }`; desugars to `type Claim = 0..N-1` |
| Number kind | `number Amount` | Finite numeric sort. Allowed in any layer incl. kernel `spec`; range set by `verify { values Amount = lo..hi }`; desugars to `type` |
| enum | `enum St { Open, Closed }` | Members are referenced by their bare name in expressions |
| struct | `struct Order { st: St, item: Option<ItemId>, qty: Qty }` | Fields are scalars or `Option<scalar>` |
| `Option<T>` | `cart: Option<ItemId>` | `none` / `some(e)`. Used instead of a sentinel value |
| `Map<K, V>` | `stock: Map<ItemId, Qty>` | K is recommended to be a bounded scalar (domain type / enum / Bool) |
| `Set<T>` | `shipped: Set<OrderId>` | T is a bounded scalar |
| `Seq<T, N>` | `queue: Seq<JobId, 3>` | A sequence (FIFO) of capacity N. T is a scalar, N is a constant |

**Scalar** = Int / Bool / domain type / enum. In a `state` declaration,
`x: lo..hi` is accepted as an anonymous domain type and is equivalent to
declaring `type X = lo..hi` and writing `x: X`.

**Types legal as state variables** (anything else is rejected by `check` as a type error):
scalar | `Option<scalar>` | struct (scalar / `Option<scalar>` fields)
| `Map<bounded scalar, scalar | Option<scalar> | struct>`
| `Set<bounded scalar>` | `Seq<scalar, N>`

- Nesting structs, Set/Map/Seq inside a struct field,
  `Option<Option<...>>`, and `Option<Set/Map/Seq/struct>` are not allowed
  (rejected at check time with a hint). Optional scalar fields can be written
  directly inside a struct as of v2.1.
- `Map<Int, V>` works but emits a deprecation warning. Use a domain-type key.
- `symmetric type` and `symmetric enum` mark values as interchangeable entity
  identities for liveness symmetry reduction. During `leadsTo` lasso/stall
  search, fslc uses one canonical representative for per-entity rows built from
  `Map<SymmetricType, V>` and `Set<SymmetricType>` state; `V` is used only when
  it contains no symmetric identity type. This is intended for models such as
  `Map<TaskId, Status>` where no task identity is special.

## 3. Expressions

- Arithmetic: `+ - * / %`, unary `-`, `min(a, b)` / `max(a, b)` / `abs(a)`
  (since `a//b` would turn everything after `//` into a comment, write division
  as `a / b` with whitespace)
- Comparison: `== != < <= > >=`
- Logical: `and or not =>`
- Quantification (bounded): `forall x: T { expr }` / `exists x: T { expr }` (can be filtered with `where expr`),
  the v0 form `forall i in lo..hi: expr` is also allowed. Expression quantifiers
  can also range over a Set or Seq value: `forall x in active { ... }` /
  `exists x in queue { ... }`; for Seq this ranges over the live prefix values.
- Aggregation: `count(x: T where expr)`, `sum(x: T of expr [where expr])`
- Cardinality predicates: `unique(x: T where expr)` / `exactlyOne(x: T where expr)`;
  `x in set_or_seq [where expr]` is also allowed. `unique` means at most one
  matching binding, while `exactlyOne` means exactly one.
- Option: `x == none` / `x != none` / `x is some(v)` (v is bound within that expression).
  **`x == some(e)` is a type error** — extract with `x is some(v)` and compare
- struct: literal `Order { st: Open, qty: 0 }`, field reference `o.st`,
  `==` is field-by-field equality
- Set: `Set {}` / `Set { 1, 2 }`, `.add(e)` `.remove(e)` `.contains(e)` `.size()`
- Seq: `Seq {}` / `Seq { 1, 2 }`, `.push(e)` `.pop()` `.head()` `.at(i)`
  `.contains(e)` `.size()`, `==` is equality of length and all elements
- Inside ensures / trans only: read the pre-transition state with `old(expr)`
- Inside a leadsTo block only: `P ~> Q` (response property. not part of the operator hierarchy of general expressions);
  optional `within K` before Q for a bounded deadline, and optional
  `decreases <int expr>` after the response body for induction ranking

Top-level temporal sugar:

```fsl
unless Name { P unless Q }   // while P holds and Q is false, the next state must keep P or make Q true
until  Name { P until Q }    // unless safety plus a leadsTo P ~> Q progress obligation
```

Use this sugar for persistent workflow states such as "held until released" or
"pending until completed". For arbitrary history facts, use an explicit ghost
variable.

## 4. Statements (init / action bodies)

- Assignment: `x = expr`, `m[k] = expr`, `m[k].field = expr`, `o.field = expr`
- Updating a Set/Seq uses the **reassignment idiom**: `s = s.add(x)`, `q = q.pop()`
- `if expr { stmt... } else { stmt... }` is allowed in both `init` and action bodies
  (can be nested with an if inside the else)
- `forall x: T { stmt... }` (bulk initialization / bulk update)

## 5. Semantics

- **Transition system**: one step = exactly one action instance
  (action name × parameter values) is executed atomically.
- **Simultaneous assignment**: all right-hand sides in an action body read the
  **old state**. Variables that are not assigned do not change (the frame
  condition is automatic).
- **Double assignment is a semantics error**: assigning to the same variable (or
  field) twice on the same execution path is a semantics error. The then/else
  of an if are separate paths, so you may assign in both. Assigning to the same
  variable **after** an if is also an error (to prevent the writes inside the
  branches from being lost).
- For `Map<K, Struct>` values, field writes are tracked per field. Updating two
  different fields of the same element in one action, such as `m[k].f1 = 1`
  followed by `m[k].f2 = 2`, is allowed. Repeating the same field on the same
  path is a semantics error.

  ```fsl
  type K = 0..1
  type V = 0..3
  struct Pair { f1: V, f2: V }
  state { m: Map<K, Pair> }
  action update(k: K) {
    m[k].f1 = 1
    m[k].f2 = 2
  }
  ```

  Observed result: `fslc check struct_fields_ok.fsl` returned `result:"ok"`,
  and `fslc verify struct_fields_ok.fsl --depth 1` returned
  `result:"verified"`. Changing the action to assign `m[k].f1` twice returned
  `result:"error"`, `kind:"semantics"` from `fslc verify`, with message
  `double assignment to 'm' field 'f1' on the same path`.
- **requires**: enabled only when all hold.
- **ensures**: checked in the post-transition state. A violation is
  `violation_kind: "ensures"`.
- **trans**: checked in the post-transition state of each execution step, with
  `old(expr)` evaluated in the pre-transition state. A violation is
  `violation_kind: "trans"`.

## 6. Automatic checks (things checked even without being written)

| Check | Content | On violation |
|---|---|---|
| Type bounds | Every bounded-type state variable (including Map values, struct fields, and Seq elements) is within range | `violated` / `type_bound` / `_bounds_<var>` |
| Partial operations | At the time of `pop()`/`head()`/`at(i)`, the sequence is non-empty and the index is in range, and the divisor of `/` `%` is non-zero | `violated` / `partial_op` / `_partial_<action>` |
| action coverage | Each action is enabled at least once within depth K | diagnosis of the blocking requires in `action_coverage` |
| Deadlock | Reaching a state where all actions become disabled | warning (`violated` with `--deadlock error`) |
| trans | Whether the two-state predicate holds across all reachable transitions | `violated` / `trans` / `trans` + trace |
| leadsTo | A P ~> Q violation via a lasso up to depth K or via deadlock stagnation | `violated` / `leadsTo` / `bindings` + trace |

- A deadlock warning includes which state you got stuck in (e.g. `deadlock reachable at
  step 1 (state: status=ToolFault, ...)`). The full trace is also in the JSON `deadlock.trace`.
- **Intended terminal states** (states where stopping is correct, such as
  processing complete or a final result) are declared in a
  `terminal { <predicate> }` block. Stopping states satisfying the predicate are
  excluded from deadlock checking, while other unexpected deadlocks continue to
  be detected. Whereas `--deadlock ignore` uniformly ignores **all stopping
  states**, `terminal` lets you select **which stops are intentional**.
  Example: `terminal { status == Done or status == Failed }`.
- **Do not write** an invariant like "inventory is at least 0" — make it
  `type Qty = 0..N` and it is detected automatically.
- A full `push` into a Seq is also detected automatically as `type_bound`
  (to guard it, write `requires q.size() < N`).

## 7. The verifier `fslc`

```
fslc check     <file.fsl>                        # syntax / names / types only (fast)
fslc verify    <file.fsl> [--depth K]            # BMC (default K=8, counterexample is shortest)
               [--engine induction] [--k N]      # k-induction: unbounded-depth proof
               [--deadlock warn|error|ignore]
               [--vacuity warn|error|ignore]     # vacuity check (§15)
               [--property <Name>]               # check one named property in isolation —
                                                 #   invariant / trans / leadsTo / reachable (for probing)
               [--exclude-property <Name>]...    # skip named invariant/trans/leadsTo/reachable
               [--strict-tags] [--requirements ids.txt]  # tag matching (§15)
fslc scenarios <file.fsl> [--depth K]            # generate integration-test scaffold JSON
fslc replay    <file.fsl> --trace <events.json>  # conformance check of an event log (§12)
fslc testgen   <file.fsl> [--depth K] [--strict] [--target pytest|vitest|swift|kotlin|dart|phpunit] [-o out]  # implementation-conformance test scaffold (§12)
fslc refine    <impl> <abs> <mapping> [--depth K]# fidelity check of a detailed spec (§10)
fslc chain     [fsl-project.toml] [--keep-going] # manifest-driven cross-layer report (§10)
fslc mutate    <file.fsl> [--by-requirement] [--max-mutants N]  # spec mutation (§15)
fslc explain   <file.fsl> [--depth K] [--readable] # JSON by default; readable text review view (§15)
fslc html      <file.fsl> [--depth K] [-o report.html] # self-contained review report (§15)
fslc typestate <file.fsl> [--ts]                 # decide applicability of state machine → ghost type (§16)
```

In addition to `reachable` and action coverage, `scenarios` outputs, for each
`leadsTo P ~> Q`, a `respond_<Name>[_<binding>]` scenario. Each scenario has
`kind: "leadsTo"`, `pending_at`, `satisfied_at`, `bindings`, `steps`,
`initial_state`, and `expected_states`, representing the shortest trace from P
holding to Q holding within depth K. Bindings for which P never holds are not
turned into scenarios and appear in `warnings`.

`verify --property Name` resolves across invariant, `trans`, `leadsTo`, and
`reachable` declarations and checks only the named property kind in isolation.
`--exclude-property Name` is repeatable and acts as the cross-kind inverse:
it removes named invariants, `trans`, `leadsTo`, and `reachable` properties
from the run and from checked-property outputs (`invariants_checked`,
`transitions_checked`, `leads_to`, and `reachables`). When `--property` and
`--exclude-property` name the same property, exclusion wins.

Exit codes: `0` = verified / proved / scenarios/testgen generated / conformant / refines /
mutated / explained / typestate,
`1` = violated / reachable_failed / unknown_cti / nonconformant / refinement_failed,
`2` = spec error (parse / type / semantics / io / vacuous / acceptance / forbidden /
`--vacuity error`), `3` = internal error.

### Kinds of result

| result | Meaning | Next move |
|---|---|---|
| `verified` | No violation up to depth K (+ all reachable satisfied); `completeness:"bounded"` | To raise confidence, use `--engine induction` |
| `proved` | **The invariant holds in all executions** (unbounded depth); `completeness:"unbounded"` | Done |
| `violated` | A counterexample exists. Comes with `violation_kind` and the shortest trace | Read the trace and fix the spec |
| `reachable_failed` | reachable not reached within depth K | Read each `unreached[].classification`: raise `--depth` for `insufficient_depth`, or fix the blocking constraint for `over_constrained` |
| `unknown_cti` | The invariant is not violated but is not inductive | **Read the CTI and add an auxiliary invariant** (§8) |
| `error` | parse / type / semantics / io | Fix per `loc` / `expected` / `hint` |

`violation_kind`: `invariant` | `trans` | `ensures` | `type_bound` | `partial_op` | `deadlock` | `leadsTo`.

Diagnostics that identify a faithfulness/intent gap may also carry
`faithfulness_class` plus `recommended_action`. Current classes are:
`partial_op_unguarded`, `frozen_only_invariant`, `intent_unexercised`, and
`liveness_not_refined`. The tag is derived from existing `result` / `kind` /
`violation_kind` fields and is additive; consumers should keep reading the
original classification fields for detail.

Progress-preserving refinement failures are reported as `refinement_failed` with
`kind:"progress_lost"`, `violation_kind:"leadsTo"`, `impl_trace`,
`progress:{leadsTo, actions}`, and `faithfulness_class:"liveness_not_refined"`.

`verify` / `verify --engine induction` results include `checked_to_depth` and
`cost: {"elapsed_s": ...}`. BMC `verified` is explicitly bounded; when the final
depth first witnesses a reachable/vacuity/coverage fact during normal
exploration, `verified` also includes a `hint` that the state space is not
obviously saturated at that depth and suggests a larger `--depth` or induction.

When a leadsTo is declared and the result is `verified` / `proved`,
`leads_to: { "<Name>": { "checked_to_depth": K } }` is attached
(no counterexample is a bounded guarantee up to depth K, the same standing as a
`verified` invariant). If induction discharges a ranked `leadsTo`, that entry is
upgraded with `proved: true`, `completeness: "unbounded"`, `proof: "ranking"`,
and `decreases`. When a `trans` is declared, the success output carries
`transitions_checked: ["Name", ...]`.

### Coverage diagnosis (actions that never become enabled)

```json
"action_coverage": {
  "checkout": {
    "covered": false,
    "name": "checkout",
    "blocking_requires": [ {"loc": {"line": 27}, "text": "requires stock[i] > 0"} ],
    "hint": "never enabled within depth 8; blocking requires: requires stock[i] > 0; ...",
    "faithfulness_class": "intent_unexercised",
    "recommended_action": "add a single-shot reachable for the action / raise --depth"
  }
}
```

The blocking requires clause is identified by a minimized unsat core when that is
cheap. For requirements `branches`, a false coverage diagnostic keeps the
internal split-action `name` and adds `display_name`.

For `reachable_failed`, each `unreached` entry carries:

```json
{
  "name": "SoldOut",
  "classification": "insufficient_depth",
  "hint": "not witnessed within depth 3; try a larger --depth"
}
```

or, when the target predicate is unsatisfiable under type bounds/invariants:

```json
{
  "name": "TooHigh",
  "classification": "over_constrained",
  "blocking_requires": [{"kind": "type_bound", "name": "_bounds_x"}],
  "hint": "target predicate is unsatisfiable under type bounds/invariants (_bounds_x); ..."
}
```

## 8. Recommended workflow: make proved the standard

1. Write the spec → `fslc check` (the fast syntax/type loop)
2. `fslc verify --depth 8` → if violated, fix using the trace.
   Confirm that the intended scenarios are witnessed by reachable
3. `fslc verify --engine induction` → if `proved`, done
4. If `unknown_cti`, read the CTI (the k+1-state trace). The starting state of
   the CTI is a **ghost state** that "satisfies all invariants but is actually
   unreachable." Add an **auxiliary invariant** that excludes it (one that is
   itself a truth of the domain) and return to step 3

In practice, auxiliary invariants often converge in a single round
(real examples in `DOGFOOD-1.md` / `DOGFOOD-2.md`: "if attempts == 3, locked,"
"only Captured has a refund," "no duplicates in the queue").

## 9. Idiom collection

### Option instead of a sentinel value

```fsl
cart: Map<UserId, Option<ItemId>>      // do not use a sentinel like -1
struct Reservation { item: Option<ItemId>, qty: Qty }  // optional fields can be written directly too
action checkout(u: UserId) {
  requires cart[u] is some(i)          // i is bound here
  requires stock[i] > 0
  stock[i] = stock[i] - 1
  cart[u] = none
}
```

### A domain type instead of a hand-written bounds invariant

```fsl
type Qty = 0..5
state { stock: Map<ItemId, Qty> }      // do not write NoNegativeStock (automatic)
```

### Guarding partial operations (either the requires form or the if form)

```fsl
action take()  { requires q.size() > 0  x = q.head()  q = q.pop() }
action drain() { if q.size() > 0 { x = q.head()  q = q.pop() } }
```

Forgetting the guard is detected as a `partial_op` violation (it does not break silently).

### Talking about a Seq in an invariant: forall with an index guard

```fsl
invariant QueuedAreQueued {
  forall i in 0..2 {                   // 0..capacity-1
    i < queue.size() => jobs[queue.at(i)].st == Queued
  }
}
```

`at()` is total in property contexts (out-of-range yields an unspecified value),
so always guard it with `i < q.size()`.

### Aggregating over a Seq: the index / domain-type idiom

```fsl
type Idx = 0..3                        // a domain type covering up to capacity-1
invariant BalanceMatchesLog {
  balance == sum(i: Idx of log.at(i) where i < log.size())
}
```

`sum`/`count` range over a domain type, but restricting to the live prefix with
`where i < size` turns it into a **fold over the Seq**.

### Two-dimensional data (rooms × slots, etc.): flatten to a single key

**Nesting Maps** like `Map<RoomId, Map<SlotId, …>>` is not allowed (§2).
Flatten the two axes into a single product domain type, and recover the axes
with `/` and `%`:

```fsl
const SLOTS = 4
type RoomId = 0..2
type Cell   = 0..11                       // ROOMS*SLOTS - 1
state { holder: Map<Cell, Option<UserId>> }
// c / SLOTS = room, c % SLOTS = slot
reachable Room1Full {
  forall c: Cell { c / SLOTS == 1 => holder[c] != none }
}
```

When the axes are few and have names (e.g. a fixed 5-period weekday), there is
also the option of decomposing into struct fields, but if quantification is
needed, flattening is the default.

### Use a ghost variable to talk about history (the past)

```fsl
state { ever_locked: Map<UserId, Bool> }   // "was locked at least once"
// in the locking branch, ever_locked[u] = true
reachable RecoveredAfterLock {
  exists u: UserId { ever_locked[u] and session[u] }
}
```

Since reachable / invariant look only at state, to talk about "Y after X" as a
**fact of the state**, push the history into the state (a ghost variable).

### When to use a history ghost variable vs. leadsTo

| What you want to write | Means |
|---|---|
| "Was X at least once" (a fact of the state) | Ghost variable + invariant / reachable |
| "Keep X true until Y, and Y may or may not happen" | `unless Name { X unless Y }` |
| "Once it becomes X, eventually Y" (a response property) | `leadsTo` + `fair action` if needed |
| "Keep X true until Y, and Y must eventually happen" | `until Name { X until Y }` |

Example: in a FIFO mutex, "a process that has entered the wait queue eventually
obtains the lock" is
`leadsTo WaiterGetsLock { forall p: ProcId { waiters.contains(p) ~> ... } }`.
If progress depends on a specific action such as `release_handoff`, add `fair`
(see `specs/mutex_queue.fsl`).

### Auxiliary invariants from a CTI (strengthening induction)

Look at the starting state of the `unknown_cti` CTI and turn "a combination that
does not occur in reality" into an invariant:

```fsl
// CTI: queue = [0, 0, 0] (the same job tripled) → state that there are no duplicates
invariant NoDupQueue {
  forall i in 0..2 { forall j in 0..2 {
    (i < j and j < queue.size()) => not (queue.at(i) == queue.at(j))
  } }
}
```

## 10. Refinement (fidelity of a detailed spec)

After first `verify`-ing / `prove`-ing the abstract spec (abs), check with
**`fslc refine`** that a detailed spec (impl) close to the implementation does
not depart from the behavior of abs (see `DESIGN-refinement.md`).

Write the mapping in a **separate file** (do not pollute the impl/abs `.fsl`):

```fsl
refinement CartImplRefinesCart {
  impl CartImpl
  abs  ShoppingCart

  maps auto
  map stock[i: ItemId] = impl_stock[i] - reserved[i]
  map cart[u: UserId]  = impl_cart[u]

  action add_to_cart(u: UserId, i: ItemId) -> add_to_cart(u, i)
  action impl_checkout(u: UserId)          -> checkout(u)
  action reserve(i: ItemId)                -> stutter
}
```

- `map <abs var> = <impl expr>` — a scalar abstract variable.
- `map <abs var>[<binder>] = <expr>` — element-wise mapping of a Map (enumerate the key type, which is bounded).
- `maps auto` — optional identity defaults. For same-named compatible state
  variables with no explicit `map`, it synthesizes `map x = x`; for same-named
  compatible actions with no explicit correspondence, it synthesizes
  `action f(params...) -> f(params...)`. Explicit entries override the defaults.
  Incompatible same-name candidates are reported as `kind: "type"` errors.
- `action <impl>(<formal params>) -> <abs>(<expr>) | stutter` — required for every impl action.
  Formal params may be bare names or `name: Type` annotations matching the impl action declaration.
  `stutter` is an internal step in which the abstract state does not change.

Only in the expressions of a refinement mapping file may you use
`if <expr> then <expr> else <expr>`. This is valid only in the right-hand side
of `map` and in the argument expressions of `action ... -> abs(<expr list>)`,
and is not part of the expression grammar of an ordinary `.fsl` spec file. The
two arms of then/else must have the same logical type
(Bool, Int/domain/enum, Option, struct).

```bash
fslc refine specs/cart_impl.fsl specs/cart_v1.fsl specs/cart_refines.fsl --depth 8
```

Success: `result: "refines"` (exit 0). Violation: `refinement_failed` (exit 1)
with `kind` (`abs_requires_failed` / `abs_state_mismatch` / `stutter_changed_abs` /
`map_out_of_bounds`), `impl_trace`, and the post-mapping `abs_before` /
`abs_after_*`. A static error (a missing map, an unknown action, etc.) is
`kind: "type"` (exit 2).

### Chain checking (composition of mappings)

The end-to-end fidelity of a layer chain (business ⊒ requirements ⊒ design …)
can be checked **directly by composition** when you line up `(spec mapping)` in
sequence:

```bash
fslc refine bot.fsl  mid.fsl bot_refines_mid.fsl  top.fsl mid_refines_top.fsl --depth 6
#            ^impl    ^abs1   ^map(impl→abs1)      ^abs2   ^map(abs1→abs2)
```

It composes the adjacent mappings (state α_AC = α_BC ∘ α_AB, actions a→b→c /
stutter) and checks bottom ⊒ top. On success it returns the composed
`action_map` and the layer ordering `chain`; on failure it returns the first
broken link `failed_link: {from, to, kind}`. Bounded refinement is transitive at
the same depth, so a composition check is equivalent to all adjacent links
holding (`docs/DESIGN-refinement.md` §7, example `examples/refinement_chain`).
Only the case where an argument expression reads the state of an intermediate
layer is unsupported.

Recommended workflow: **a human/LLM reviews abs → the LLM elaborates impl →
`refine` guarantees fidelity**. The `ensures` / invariants of abs are not
re-checked in refine; it is assumed that they have been separately verified on
the abs side.

What `refine` guarantees is **inclusion of safety** (impl does not break the
guards/invariants of abs). **Liveness (`leadsTo`) does not propagate** — since
refine allows stutter, even if impl drops the progress that abs guaranteed with
`fair`, the result can still be `refines` (the mapping does not require fair
annotations). To opt into checking the abstract response on the lower layer,
write `preserve progress` in the refinement mapping:

```fsl
refinement DesignRefinesReq {
  impl Design
  abs  Req
  map st = ...
  action enqueue(c) -> stutter
  action answer(c)  -> answer(c)
  action refuse(c)  -> refuse(c)

  preserve progress {
    respond EveryRequestHandled by answer, refuse
  }
}
```

This pulls the named abstract `leadsTo` through the state mapping and checks
`P(α(impl_state)) ~> Q(α(impl_state))` on impl executions. If the lower layer can
spin forever or deadlock while the abstract response remains pending, the result
is `refinement_failed` with `kind:"progress_lost"` and
`violation_kind:"leadsTo"`. The `by` actions are review metadata and must name
impl actions; fairness still comes from lower-layer `fair action` declarations.
For unbounded proof, keep using a lower-layer `leadsTo ... decreases ...` and
`verify --engine induction`.

## 11. Composition (compose)

**Merge with namespaces** several verified component specs into a single system
specification. After expansion it becomes an ordinary single spec, so `verify` /
`prove` / `scenarios` / `Monitor` / `replay` / `testgen` / `refine` can be used
as-is (design: `DESIGN-compose.md`).

```fsl
compose OrderSystem {
  use ShoppingCart as cart from "cart_v1.fsl"
  use Payment      as pay  from "payment.fsl"

  state { orders_linked: Int }
  init  { orders_linked = 0 }

  // synchronized action: execute the actions of several components in the same step
  action checkout_and_pay(u: cart.UserId, p: pay.PayId) =
      cart.checkout(u) || pay.capture(p) {
    requires pay.payments[p].st == Authorized
    orders_linked = orders_linked + 1
  }

  // excluded from standalone execution (fires only via synchronization)
  internal cart.checkout
  internal pay.capture

  invariant LinkedNonNeg { orders_linked >= 0 }
  reachable PaidOrder {
    exists p: pay.PayId { pay.payments[p].st == Captured }
  }
}
```

- `use <SpecName> as <alias> from "<relative path>"` — the path is relative to
  the compose file. The spec name must match the in-file name. The alias must be
  unique within the compose. Nested compose is not allowed.
- A component's types/state/actions are referenced as `alias.Name`.
- **Synchronized action** `action <name>(...) = <a>.<act>(...) || <b>.<act2>(...) { ... }`:
  merges the requires / body / ensures of each component action, and the
  additional statements may only assign to the composition-side state
  (synchronizing two actions of the same component is not allowed).
- Fairness is not inherited through synchronization. If a fair component action
  is referenced by a non-fair synchronized action, `check` / `verify` emits a
  `fair_not_inherited` warning in JSON `warnings`; write
  `fair action <name>(...) = ...` when the synchronized action must be fair.
- Synchronized action arguments are structurally compatible by bounded integer
  domain, not by declared type name. Passing a `core.TaskId` value to an action
  parameter declared as `NoteId` is valid when the underlying value range fits
  the target type. This is the intended compose behavior, not an incidental
  naming accident. Repro: with `TaskId = 0..2` and `NoteId = 0..2`,
  `action sync(t: core.TaskId) = core.choose(t) || note.attach(t) { }`
  produced `result:"ok"` from `fslc check` and `result:"verified"` from
  `fslc verify --depth 1`. With `NoteId = 0..1`, the same compose still passed
  `check` but `verify --depth 1` returned `result:"violated"`,
  `violation_kind:"type_bound"`, invariant `"_bounds_note.last"`, for
  `sync(t: 2)`. Recommended idiom: use same-range component-local domain types
  for intentionally shared IDs; if a target domain is narrower, add an explicit
  `requires` guard on the synchronized action or model a conversion in one
  component.
- `internal <alias>.<action>` — excludes that action from interleaving.
- An ordinary `action` (without `=`) can also be written (a glue action).
- JSON display: the physical name `alias__x` is output as `alias.x` (state keys,
  action names, invariant / reachable names, traces, scenarios, and Monitor — all of them).

```bash
fslc check  specs/order_system.fsl
fslc verify specs/order_system.fsl --depth 8
fslc scenarios specs/order_system.fsl
```

## 12. The bridge to implementation

After proving a specification, there are three entry points for wiring it to the
implementation (see `DESIGN-bridge.md`).

| Means | Use |
|---|---|
| `fslc.runtime.Monitor` | A concrete interpreter of the spec (no Z3 needed). Embed it in the implementation for runtime checking |
| `fslc replay` | Check a real system's event-log JSON against the spec |
| `fslc testgen` | Generate a conformance-test scaffold — pytest (default), Vitest (`--target vitest`), Swift Testing (`--target swift`), kotlin.test (`--target kotlin`), Dart `package:test` (`--target dart`), or PHPUnit (`--target phpunit`) (wire the implementation into the Adapter) |

Recommended workflow: **`verify` / `prove` the spec → generate the scaffold with
`testgen` → wire the implementation into the `Adapter` → run the tests**. `Monitor`
is used as an oracle in random-walk testing.

`testgen` separates a language-independent scenario-collection core (`scenarios`)
from per-target emitters, so the same scenarios render to multiple harnesses:

- `--target pytest` (default): emits Python tests that import `fslc.runtime.Monitor`
  and drive the random walk live as the oracle.
- `--target vitest`: emits a self-contained TypeScript (Vitest) file. Deterministic
  scenarios and forbidden-rejection assertions translate directly; the random walk
  is **baked at generation time** — the Python `Monitor` runs the fixed-seed walk and
  the `(action, params, expected_state)` trace is embedded as a static fixture, so the
  generated tests need **no `fslc`/Python at runtime**. The output extension defaults
  to `<spec>.test.ts`.
- `--target swift`: emits a self-contained Swift Testing file (`import Testing`,
  `@Test`, `#expect`; **not XCTest**). Same baked-walk design as Vitest. Dynamic
  state is `[String: Any]` with a bundled deep-equality/partial-match helper; an
  Option `None` bakes as the self-contained `FSLNull.instance` sentinel (no
  Foundation). Until `makeAdapter()` is wired every test is disabled via
  `@Test(.enabled(if:))`. Output defaults to `<SpecName>ConformanceTests.swift`.
- `--target kotlin`: emits a self-contained kotlin.test file (multiplatform; the
  JVM delegates to JUnit). Same baked-walk design. Dynamic state is
  `Map<String, Any?>`, where Kotlin's structural `==` is deep on `List`/`Map` and
  distinguishes `Int` from `Double`, so the partial-match helper is a plain
  recursion. kotlin.test has no portable runtime skip, so until `makeAdapter()`
  is wired (it returns `null`) each test returns early. Output defaults to
  `<SpecName>ConformanceTest.kt`.
- `--target dart`: emits a self-contained `package:test` file (also runs under
  `flutter test`). Same baked-walk design. Dynamic state is `Map<String, dynamic>`;
  Dart's `==` is reference-based on collections, so the bundled `assertPartial`
  recurses by the expected keys and compares leaves/sequences with the `equals`
  matcher (re-exported by `package:test`, so the only dependency is `package:test`).
  A top-level probe sets `skip:` on each `test()` until `makeAdapter()` is wired.
  Output defaults to `<spec_name>_conformance_test.dart` (snake_case, the
  `_test.dart` suffix the runner expects).
- `--target phpunit`: emits a self-contained PHPUnit file (PHP 8.1+ / PHPUnit 10+,
  `declare(strict_types=1)`). Same baked-walk design. Dynamic state is an
  associative `array`; leaves are compared with `assertSame` (`===`), which keeps
  `int`/`float`, `bool` and `null` from coercing (PHP's loose `==` would conflate
  `0 == "0"` etc.). `assertPartial` recurses by the expected keys (maps match
  order-independently; list-shaped values also pin length). `setUp()` skips every
  test until `makeAdapter()` is wired. Output defaults to
  `<SpecName>ConformanceTest.php` (PSR-4 class = file name).

```python
from fslc import Monitor

mon = Monitor("specs/cart_v1.fsl")
mon.reset()
r = mon.step("add_to_cart", {"u": 0, "i": 0})   # ok / kind / state / changes
```

```bash
fslc replay specs/cart_v1.fsl --trace events.json   # conformant / nonconformant
fslc testgen specs/cart_v1.fsl -o test_cart_v1.py            # pytest (default); partial reachability warnings unless --strict
fslc testgen specs/cart_v1.fsl --target vitest -o cart.test.ts  # self-contained Vitest (TypeScript) scaffold
fslc testgen specs/cart_v1.fsl --target swift -o CartConformanceTests.swift  # self-contained Swift Testing scaffold
fslc testgen specs/cart_v1.fsl --target kotlin -o CartConformanceTest.kt  # self-contained kotlin.test scaffold
fslc testgen specs/cart_v1.fsl --target dart -o cart_conformance_test.dart  # self-contained package:test scaffold
fslc testgen specs/cart_v1.fsl --target phpunit -o CartConformanceTest.php  # self-contained PHPUnit scaffold
```

Since `replay` checks only finite logs, **`leadsTo` is out of scope** (stated
explicitly in the output `note`). `Monitor` requires init to be deterministic
(forall bulk assignment is allowed).

## 13. The three-layer dialects (consulting / requirements / design) and traceability

The background of the design is in `DESIGN-layers.md`, and the implementation
spec in `DESIGN-dialects.md`. There is a single kernel (§1–12 of this document),
and the per-layer dialects are a front-end that expands into the AST. The layers
are connected by refinement: **business ⊒ requirements ⊒ design ⊒ implementation
(testgen/replay)**.

### 13.1 Declaration tags (traceability common to all layers)

If you write `"ID: original text"` just before the `{` of an invariant /
reachable / leadsTo / action, then violations, CTIs, coverage diagnoses, and
scenarios carry `requirement: {id, text}`:

```fsl
invariant PaidLedger "REQ-3: the ledger matches the number of payments" { ... }
action submit(c: Case, a: Amount) "REQ-1: amounts at or below the threshold are auto-approved" { ... }
```

### 13.2 Requirements layer: `requirements` (the fsl-req dialect)

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

- The process+data profile is the primary requirements form for a single-entity
  lifecycle. `process E with f: T { ... }` creates the entity stage map and
  carried fields; transitions can add an input (`with a: T`), guard (`when`),
  field update (`set f = expr`), and traceability (`covers REQ-n "text"`).
- `number Amount` declares a value kind; the finite verifier range lives in
  `verify { values Amount = lo..hi }`. Entity sizes live in
  `verify { instances Entity = N }`.
- `kpi NAME = count ENTITY in STAGE` is a declarative projection in both
  business and requirements. It does not create a ghost counter or an automatic
  `_kpi_*` invariant.
- With `implements`, `fslc verify` **also runs the refine to the upper layer
  simultaneously**, and the result carries `implements: {abs, result}`. An empty
  body (`implements X from "..." { }`) auto-generates identity refinement when
  process/action/stage names match. Inside the `implements { }` block you write
  only state `map` entries, `maps auto`, and `preserve progress` — not `action`;
  action↔action correspondence is the `maps <abs_act>(...)` clause on the
  requirement-level action. `maps auto` covers same-name kernel-wrapper
  state/actions, and explicit maps override it. Auto-mapped process transitions
  are actor-checked; a transition whose actor differs from the business action's
  actor is a check-time error.
- `acceptance` is replay-verified at check time by the concrete Monitor (a
  failure is `kind: "acceptance"`). It supports `expect <Entity> <id> in
  <Stage>` alongside `expect <expr>` and flows directly into scenarios / testgen
  (= the acceptance criteria become conformance tests for the implementation).
  Action arguments in `acceptance`/`forbidden` steps accept enum member names
  and const names in addition to numeric literals (e.g. `answer(0, Triggered)`
  is equivalent to `answer(0, 1)` when `Triggered` is `Trigger`'s second
  member) — an undefined name is a check-time error.
- Use the kernel-wrapper form only for hard cases: multi-entity requirements,
  conservation rules, SLA/time, history not expressible as a carried field, or
  behavior that needs explicit kernel state. That fallback still supports
  `struct` / `state` / `init`, `fair action`, `branches`, and explicit `maps`.
  `branches` automatically splits an action by each when condition (displayed as
  `submit[a <= AUTO_LIMIT]`), and the `maps` clause provides the action
  correspondence to the upper layer.

### 13.3 Consulting layer: `business` (the fsl-biz dialect)

Write business processes, policies, and KPIs with zero implementation
vocabulary (syntax details in `DESIGN-dialects.md` §3). A process expands into
enum+Map+transition actions, a policy into invariant / leadsTo, and a kpi into a
declarative count projection recorded as metadata. A contradiction in the regulations =
an invariant violation, a dead process step = a coverage diagnosis, an
unreachable business goal = reachable_failed, and a case left unattended = a
leadsTo counterexample — all can be detected mechanically.

For PM/consulting-facing files, use the readable stage syntax for common
response policies and goals:

```fsl
business ReturnHandling {
  actor Customer, Manager
  entity Return
  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition refund Approved -> Refunded by Manager
  }

  kpi refunded = count Return in Refunded

  control CTRL-DECISION
    "Every return must preserve an adjudication control"
    owner Manager
    severity high
    applies_to Return

  policy PAY-2 "every request is eventually decided"
    satisfies CTRL-DECISION
    every Return in Requested must eventually be Approved or Rejected or Refunded
  goal AllSettled "all cases can be completed"
    all Return can be Refunded or Rejected
}

verify {
  instances Return = 3
}
```

The explicit forms remain available when the rule is not just stage progression:
`policy ... responds { forall c: Return { stage(c) == Requested ~> ... } }` and
`goal ... { exists c: Return { stage(c) == Refunded } }`.

Governance/control metadata can be kept inside `business` or lifted into a
standalone catalog. `control ID "text" owner NAME severity NAME applies_to Entity`
does not generate a property by itself; it is a catalog entry. A `policy` or
`goal` can declare `satisfies CTRL-ID`, and violations then carry both the
policy/goal requirement and the satisfied controls in JSON:

```fsl
policy PAY-2 "every request is eventually decided"
  satisfies CTRL-DECISION
  every Return in Requested must eventually be Approved or Rejected or Refunded
```

For controls reused across business specs, use a `governance` catalog:

```fsl
governance EnterpriseReturnControls {
  authority Operations owns CTRL-DECISION
  control CTRL-DECISION "Every return must preserve an adjudication control"

  delegates ReturnHandling from "return_policy.fsl" {
    require CTRL-DECISION
    // optional if the business policy already says `satisfies CTRL-DECISION`
    CTRL-DECISION is satisfied_by policy PAY-2
  }

  preservation ReturnReform {
    before AsIsReturn from "asis_return.fsl"
    after  ToBeReturn from "tobe_return.fsl"
    preserve CTRL-DECISION
    checked_by refinement "tobe_refines_asis.fsl"
  }
}
```

`fslc check governance.fsl` validates all referenced controls, business files,
policies/goals, and preservation files. Preservation blocks also run the declared
refinement at depth 8 and report results under `governance.preservations`.

### 13.4 How to write non-functional requirements (NFRs)

The majority of NFRs can be written with the same machinery as functional
requirements (details and demonstration in `DESIGN-nfr.md`):

| NFR | How to write it |
|---|---|
| Permission (only an admin does X) | `requires role[u] == Admin` + an invariant over the ghost `done_by_admin` |
| Audit completeness | A cross-cutting invariant (e.g. `audit.balance == cleared + pending + withdrawn`) |
| Capacity / upper bound | A bounded type / Seq capacity / a `count(...) <= N` invariant |
| Reliability behavior | A fault-injection action (`crash`) + a mode state + `fair recover` + a recovery leadsTo |
| **SLA / timeout** | A `time` block + `deadline` (below) |
| Probability, percentiles, real time in ms | **Out of scope** (write it in prose) |

An SLA is checked as safety in discrete time (inside `requirements`):

```fsl
time {
  urgent start, finish                    // while enabled, time (tick) does not advance
  age waitAge[r: Req] while pending[r]    // +1 per tick, 0 if the condition is false
}
requirement NFR-1 "complete within 4 ticks of acceptance" {
  deadline waitAge <= 4
}
```

- The tick is auto-generated, and the urgency discipline ("the system does not
  procrastinate work when idle") is its guard. If you do not specify urgent,
  most deadlines become violated by a starvation trace — which is a correct
  indication that "there is no scheduling assumption."
- Placement: at most one `time` directly under requirements, and `deadline`
  inside a requirement (the requirement ID is tied to the violation). An age is
  +1 per tick (reset to 0 if while is false) and can be read from guards as an
  ordinary state variable.
- `tick` is generated — do not declare your own `action tick` (it is a check
  error). It advances age counters only and auto-maps to `stutter` under
  refinement; reference it as `tick()` (e.g. in an `acceptance` scenario).
  Tick-side work (service time, etc.) needs the kernel-wrapper form.
- **Across layers, a `deadline` refines only over a shared clock.** A `deadline`
  is a safety property of the clock that declares it, so a design refines a
  *timed* requirements spec only when its `tick` mirrors the generated one; a
  design with a finer clock (a `tick` that also consumes service time) has no
  abstract image for those steps and fails `fslc refine` with
  `abs_requires_failed` — the same non-propagation as liveness. Verify a timed
  property at the clock-owning layer, or keep the upper contract time-less and put
  the clock in the design kernel (`tick → stutter`). See `docs/DESIGN-nfr.md` §6
  and `examples/nfr/sla_worker_design.fsl`.
- **⚠ The vacuous-SLA trap**: making an action that can always be enabled urgent
  freezes time, and any K satisfies the deadline vacuously (even `<= 0` is
  green). The correct form is **to make urgent only the guarded action that
  becomes enabled at deadline arrival (the respond_due form with
  `requires age >= K`)**. To confirm non-vacuity, lower it to `K-1` and check
  that it becomes violated. `fslc verify --vacuity` emits
  `kind:"urgency_freeze"` for the provable form of this trap (the urgent
  condition is initial and inductive); absence of the warning is not a proof of
  non-vacuity.
- The BMC check works immediately. An inductive proof often needs a time-budget
  auxiliary invariant (the `age + remaining work <= K` form) (derived from the
  CTI; real examples in `examples/nfr/`).

### 13.5 What is not handled (the boundary of the layers)

The majority of non-functional requirements (permissions, audit, capacity,
reliability behavior, discrete-time SLA) can be handled (§13.4). What remains
outside FSL is: **probability, percentiles (99.9% etc.), real time (wall-clock
ms), usability, and prose rationale** (write these in each layer's document).
FSL is responsible for the **checkable skeleton** of each artifact.

## 14. Library API

```python
from fslc import parse, build_spec, verify, prove, Monitor

spec   = build_spec(parse(src))
result = verify(spec, depth=8)            # BMC
result = prove(spec, k_ind=1, base_depth=8)   # k-induction
```

Returns a dict with the same structure as the CLI (the CLI wraps it with a
`"fsl": "1.0"` envelope).

## 15. Validation suite (the spec ≠ intent gap)

What `fslc` guarantees is "the internal consistency of the written spec," not
"whether the spec is faithful to the original intent." When you have AI write a
spec, errors concentrate in this validation layer. The following is the set of
checks that **surface those errors as mechanical mismatches** (the overall
picture of the design is roadmap issue #1; each feature has a corresponding
DESIGN-*.md).

- **`forbidden` (negative acceptance criteria)** — a requirements-dialect
  construct. Write an "operation sequence that should be rejected," and at check
  time it is replay-verified that the last step is rejected (not-enabled or a
  violation). If it is accepted, `kind:"forbidden"` (detection of
  under-constraint = a missing guard, which a safety invariant stays silent
  about). The dual of `acceptance` (must-allow). → [`DESIGN-forbidden.md`](DESIGN-forbidden.md)
- **Vacuity check (`--vacuity`)** — on the verified/proved path, warns about
  `vacuous_implication` (the antecedent of an implication is unreachable),
  `vacuous_leadsto` (the trigger is unreachable), `always_true_requires`
  (a guard that is always true under the context of preceding clauses),
  `tautology_over_frozen` (a dynamically tautological invariant over state no
  action changes), and `urgency_freeze` (a generated deadline `tick` proven dead
  because urgency freezes time). `error` exits 2. →
  [`DESIGN-vacuity.md`](DESIGN-vacuity.md)
- **`--strict-tags`** — warns on success results about untagged declarations
  (fabrication candidates) and unreferenced requirements (omission candidates,
  including empty requirement blocks). Existence-level matching. → [`DESIGN-strict-tags.md`](DESIGN-strict-tags.md)
- **`fslc mutate`** — mechanically mutates the spec and measures whether each
  mutant is killed by the existing net of checks. A surviving mutant = behavior
  constrained by no property = a place where an invariant is missing.
  `--by-requirement` flags "a requirement that kills no behavior mutant" as an
  `empty_formalization` warning (the semantic-level extension of
  `--strict-tags`). → [`DESIGN-mutate.md`](DESIGN-mutate.md)
- **`fslc explain --readable`** — a text view over skeleton enumeration (state,
  action who/when/what-changes, verification bounds, fairness, KPI projections,
  branch lowering, synthesized refinement mappings, automatic checks, tags) +
  counterfactuals ("without this rule, this procedure could break it") + witness
  narration. Moves human review from reading logical formulas to adjudicating
  concrete examples. JSON mode remains available without `--readable`. →
  [`DESIGN-explain.md`](DESIGN-explain.md)
- **`fslc html`** — a self-contained HTML report over the same explain/verify
  evidence: status summary, state/action/property tables, an action-to-state
  write graph, trace timelines, witness examples, counterfactuals, source, and
  raw JSON. It is meant for PRs, design reviews, and non-specialist project
  review without requiring the reader to run the CLI. →
  [`DESIGN-html-report.md`](DESIGN-html-report.md)

The discipline before writing (the formalization memo, the NL→syntax reverse
lookup, recommended practices) is in the AI-agent skills under `skills/`, with
the shared language reference in `skills/fsl/SKILL.md`; the real-run record is in
[`DOGFOOD-9.md`](DOGFOOD-9.md).

## 16. Promotion judgment to ghost types (typestate)

`fslc typestate <file.fsl> [--ts]` decides how soundly the state machine of a
design spec (enum-valued struct fields / state variables / `Option<_>` slots)
can be mapped to the host language's typestate (ghost types). It classifies each
`(entity, action)` as `derivable` (the from-state is the entity's own local
guard) / `branching` (data-dependent inside an `if`) / `relational` (no local
guard, and the precondition lives in an external structure — cannot be expressed
in the type and remains as a runtime/verification obligation). An entity's
`applicability` is `full` only when all transitions are derivable/branching.
With `--ts`, it outputs a TypeScript scaffold for the derivable portion.
→ [`DESIGN-typestate.md`](DESIGN-typestate.md)
