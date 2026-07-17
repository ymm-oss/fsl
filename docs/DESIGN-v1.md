# FSL v1 Language Design Document

This document defines the v1 design of FSL (AI-Native Formal Specification Language).
It carries forward the concepts of the v0 prototype ([`LANGUAGE.md`](LANGUAGE.md))
and answers the limitations made explicit in v0 (impoverished types, safety only,
bounded only, no bridge to implementation). **v1 is a complete superset of v0**:
existing `.fsl` files verify unchanged.

---

## 1. Restating the Concept and the Evaluation Criteria

The primary goal of FSL is to be **written, verified, and repaired by generative AI**.
Every v1 feature addition or rejection was judged against the following 5 criteria.

| # | Criterion | Meaning |
|---|---|---|
| G1 | **Generation probability** | Maximize the probability that an LLM writes it correctly on the first try. Choose syntax close to the training distribution (TS/Python/Rust-like), and as a rule provide exactly one way to write a given thing |
| G2 | **Repairability** | When it fails, "where, why, and how to fix it" is mechanically determined from the output JSON alone. Every diagnostic carries location information and a repair hint |
| G3 | **Verification responsiveness** | Returns in seconds. Bounded and small-scope by default. The latency of the write→verify→repair loop comes first |
| G4 | **Semantic simplicity** | Do not add semantics beyond "one step = atomic execution of one action" and "simultaneous assignment." Every new construct must be explainable as sugar over existing semantics or as bounded expansion |
| G5 | **Structural elimination of pitfalls** | Remove "spec bugs LLMs tend to make" — sentinel values (-1), unbounded quantification, vacuous specs, implicit range escapes — from the language itself, via types and automatic checks |

v0 itself contains an exhibit of a G5 violation: the sample's `cart: Map<Int, Int>`
uses a sentinel value where "-1 = empty," and `NoNegativeStock` hand-writes as an
invariant what is really a **fact of the type** — "stock quantity is a non-negative
amount." The v1 type system absorbs both of these on the language side.

---

## 2. What Changes in v1 (Overview)

| Area | v0 | v1 |
|---|---|---|
| Domain | Write `const MAXU = 1` + `u in 0..MAXU` every time | Declare `type UserId = 0..1` and reference via `u: UserId` |
| Absence of value | Sentinel values such as `-1` | `Option<T>` (`none` / `some(e)` / `is some(x)`) |
| Vocabulary of state | Magic numbers in Int | `enum Status { Draft, Placed, ... }` |
| Entities | Manually manage parallel maps | `struct Order { status: Status, qty: Qty }` |
| Collections | Hand-build a characteristic function with `Map<Int, Bool>` | `Set<T>` (`contains` / `add` / `remove` / `size`) |
| Range escape | Hand-written invariant | **Automatic bounds checking** of bounded types (implicit invariant) |
| Action body | Assignment and forall only | `let` / `if-else` / `ensures` (postcondition) |
| Aggregation | Inexpressible | `count(...)` / `sum(...)` (bounded expansion) |
| Properties | invariant (safety) only | + `reachable` (reachability = scenario check) |
| Automatic checks | action coverage, init satisfiability | + type bounds, deadlock check |
| Output JSON | Trace of full states only | + per-step **state diff**, `loc` on every diagnostic, schema version |
| CLI | `verify` only | + `check` (for the fast syntax/type-only loop) |
| Proof | Bounded only | Specifies the k-induction engine and the CTI repair protocol (implementation in v1.1) |

**Rejected proposals** (rationale in each section): string types, unbounded
quantification, full LTL, qualified enum references (`Status.Paid`),
modules/imports, user-defined functions.

---

## 3. Type System

### 3.1 Domain Types (Bounded Subrange)

```fsl
type UserId = 0..2        // the 3 values 0,1,2
type Qty    = 0..5
```

- Names a bounded integer subrange. Both endpoints of the range are
  compile-time integers (literals, `const`, or their arithmetic).
- **Becomes a domain for quantification**: `forall u: UserId { ... }`. This
  eliminates the repeated `forall u in 0..MAXU:` of v0 (G1: structurally
  prevents the typical generation error of a mismatched range).
- **Automatic bounds checking**: state variables of a domain type (including
  map values and struct fields, recursively) get an implicit invariant
  `_bounds_<variable name>` generated, which is checked just like a user
  invariant. If a range escape occurs in any reachable state including init,
  the result becomes `violated` (§7.4).
  - Design decision: bounds are **checked, not assumed**. Treating them as
    assumptions would hide range-escape bugs by declaring "that state does not
    exist" (G5).
  - Consequence: the v0 sample's `NoNegativeStock`, if you use
    `type Qty = 0..N`, is **detected automatically without being written**.

### 3.2 enum

```fsl
enum Status { Draft, Placed, Paid, Shipped, Cancelled }
```

- Member names must be globally unique across the whole spec (duplicates are a
  `name` error). References are unqualified (`Placed`) only.
  - Rejected: coexistence of the `Status.Paid` qualified form. Having two ways
    to write it makes the LLM's output waver and pollutes diffs (G1). The
    uniqueness check can detect collisions.
- The trace and counterexample JSON show the **member name verbatim**
  (`"status": "Paid"`). The numeric encoding is never shown to the LLM (G2).

### 3.3 Option

```fsl
state { cart: Map<UserId, Option<ItemId>> }
```

- Literals `none` / `some(expr)`.
- Testing and extraction are done with the `is` pattern:
  - `cart[u] is none` — is empty
  - `cart[u] is some(i)` — a value is present, and **binds `i`**. When written
    in `requires`, the binding is usable throughout the rest of that action body
    (subsequent requires, assignments, ensures)
  - Structural comparisons `cart[u] == none` / `!= none` and
    `cart[u] == some(e)` / `!= some(e)` are allowed when no binding is needed.
    Two present values compare their payloads; two absent values are equal and
    their internal payload storage is ignored.
  - Equality never binds a name. Use `is some(v)` when later expressions need
    the contained value (G1 "one way to write a given thing").
- Partial functions (an unwrap like `value(x)` / `x!`) are **not provided**, so
  as not to create an entry point to the undefined behavior of a guardless
  unwrap (G5). Extraction always passes through `is some(x)` = total.
- In JSON it is shown as `null` or the value.

### 3.4 struct

```fsl
struct Order { status: Status, qty: Qty }
state  { orders: Map<OrderId, Order> }
```

- Field access `orders[o].status`, per-field assignment
  `orders[o].status = Shipped`, literal
  `orders[o] = Order { status: Draft, qty: 0 }`.
- Equality `==` is field-by-field equality. From v2.1, an `Option<scalar>`
  field can be written directly (`struct Res { item: Option<ItemId> }`).
  Equality of an Option field treats two `none`s as equal and compares values
  only when present — a logical equality. Nested structs (a struct containing a
  struct), Set/Map/Seq fields, `Option<Option<...>>`, and
  `Option<Set/Map/Seq/struct>` are not allowed.

### 3.5 Set

```fsl
state { shipped: Set<OrderId> }
```

- The element type is restricted to a bounded type (domain type or enum).
- Operations are method-style (G1: matches the LLM's habits):
  - `s.contains(e)` : Bool
  - `s.add(e)` / `s.remove(e)` : a new set (an expression; use on the
    right-hand side of an assignment)
  - `s.size()` : Int
- Literals: `Set {}` (empty), `Set { 0, 2 }`.
- In JSON it is shown as a **sorted array** (`"shipped": [0, 2]`).

### 3.6 Seq (v1.1)

```fsl
const CAP = 3
state {
  queue: Seq<JobId, CAP>,
  log:   Seq<Qty, 5>
}
```

- A FIFO sequence with capacity `N`. The element type `T` must be a scalar
  type. `N` is a positive constant expression (an integer literal or a `const`
  name).
- Usable **only as the type of a state variable** (not as a map value, set
  element, or sequence element; not as a struct field either, and
  `Option<Seq<...>>` is also disallowed — `check` reports `kind: "type"` + hint).
- Operations (pure, re-assignment idiom): `size()` / `push(e)` / `pop()` /
  `head()` / `at(i)` / `contains(e)` / `==` / `!=`.
- Literals: `Seq {}` / `Seq { 1, 2 }` (element count ≤ N).
- `pop()` / `head()` / `at(i)` are partial functions. Inside an action body,
  `requires`, or `ensures`, an **implicit well-definedness check**
  (`partial_op`) is attached. Use it together with a guard idiom such as
  `requires q.size() > 0` (G5).
- A `push` when full violates the implicit bounds invariant `_bounds_<variable>`
  (`violation_kind: "type_bound"`).
- In JSON it is a length-prefixed array (`"queue": [1, 2]`, empty is `[]`).
  The diff is a `{from, to}` over the whole sequence.

### 3.7 Map

- `Map<K, V>`: **K must be a bounded type (domain type or enum)**. This makes
  the trace display total and keeps quantification and aggregation always
  bounded.
- v0 compatibility: `Map<Int, ·>` is still accepted, but `fslc check` /
  `verify` puts a deprecation warning and a mechanical rewrite hint
  ("declare `type K = 0..N` and replace") in `warnings`.

### 3.8 Int / Bool

- Kept as is. `Int` is unbounded (a Z3 integer). Use it for aggregate values
  (such as total revenue). Unbounded variables do not get automatic bounds
  checking.

### 3.9 No String Type (Design Decision)

It is rare for the contents of a string to carry meaning at the spec level;
almost always "a finite number of distinguished values" suffices. Represent
this with an `enum` or an opaque domain type. Z3's string theory is slow and
violates G3, and LLMs are prone to mistakes from notational variation in string
comparison (G5). Error messages steer toward this policy.

---

## 4. Syntax

### 4.1 Grammar (EBNF)

```ebnf
spec          ::= "spec" NAME "{" item* "}"
item          ::= const_def | type_def | enum_def | struct_def
                | state_def | init_def | action_def
                | invariant_def | reachable_def

const_def     ::= "const" NAME "=" const_expr
type_def      ::= "type" NAME "=" const_expr ".." const_expr
enum_def      ::= "enum" NAME "{" NAME ("," NAME)* ","? "}"
struct_def    ::= "struct" NAME "{" field ("," field)* ","? "}"
field         ::= NAME ":" type

state_def     ::= "state" "{" var_decl ("," var_decl)* ","? "}"
var_decl      ::= NAME ":" type
type          ::= "Int" | "Bool" | NAME            // NAME = domain/enum/struct
                | "Map" "<" type "," type ">"
                | "Set" "<" type ">"
                | "Seq" "<" type "," const_expr ">"
                | "Option" "<" type ">"

init_def      ::= "init" "{" stmt* "}"

action_def    ::= "action" NAME "(" (param ("," param)*)? ")" "{" action_item* "}"
param         ::= NAME ":" NAME                     // bounded type
                | NAME "in" const_expr ".." const_expr   // v0 compatibility
action_item   ::= "requires" expr
                | "ensures" expr
                | "let" NAME "=" expr
                | stmt

stmt          ::= lvalue "=" expr
                | "if" expr "{" stmt* "}" ("else" "{" stmt* "}")?
                | "forall" binder ":"? "{" stmt* "}"
lvalue        ::= NAME ("[" expr "]")? ("." NAME)?
binder        ::= NAME ":" NAME ("where" expr)?
                | NAME "in" const_expr ".." const_expr   // v0 compatibility

invariant_def ::= "invariant" NAME "{" expr "}"
reachable_def ::= "reachable" NAME "{" expr "}"
```

Expressions:

```ebnf
expr        ::= quant | imp
quant       ::= ("forall" | "exists") binder ("{" expr "}" | ":" expr)
imp         ::= or_e ("=>" imp)?                    // right-associative
or_e        ::= and_e ("or" and_e)*
and_e       ::= not_e ("and" not_e)*
not_e       ::= "not" not_e | is_e
is_e        ::= cmp ("is" pattern)?
pattern     ::= "none" | "some" "(" NAME ")"
cmp         ::= sum (("==" | "!=" | "<" | "<=" | ">" | ">=") sum)?
sum         ::= product (("+" | "-") product)*
product     ::= unary ("*" unary)*
unary       ::= "-" unary | postfix
postfix     ::= atom ("[" expr "]" | "." NAME
                     | "." ("contains" | "add" | "remove" | "push" | "pop"
                            | "head" | "at" | "size")
                       "(" (expr ("," expr)*)? ")")*
atom        ::= INT | "true" | "false" | "none"
              | "some" "(" expr ")"
              | "Set" "{" (expr ("," expr)*)? "}"
              | "Seq" "{" (expr ("," expr)*)? "}"
              | NAME "{" NAME ":" expr ("," NAME ":" expr)* "}"  // struct literal
              | "count" "(" NAME ":" NAME "where" expr ")"
              | "sum" "(" NAME ":" NAME "of" expr ("where" expr)? ")"
              | "min" "(" expr "," expr ")"
              | "max" "(" expr "," expr ")"
              | "abs" "(" expr ")"
              | "old" "(" expr ")"                  // inside ensures only
              | NAME
              | "(" expr ")"
```

Reserved words: `spec state init action requires ensures invariant reachable
const type enum struct let if else forall exists in where is and or not
true false none some old count sum min max abs Int Bool Map Set Seq Option`

### 4.2 Quantification and Aggregation

- `forall u: UserId { expr }` / `exists i: ItemId { expr }` — bounded
  quantification over a domain type or enum. The v0 form
  `forall i in 0..MAXI: expr` also remains valid.
- The `where` form is sugar: `forall x: T where p { q }` ≡
  `forall x: T { p => q }`, `exists x: T where p { q }` ≡
  `exists x: T { p and q }`.
- `count(o: OrderId where predicate)` — the number of individuals satisfying
  the predicate.
- `sum(o: OrderId of expr [where predicate])` — the sum of the expression
  (over individuals satisfying the predicate only).
- All are **bounded expansions** over a declared domain; unbounded
  quantification cannot be written (G3/G5: excluded at the syntax level).

### 4.3 Style Conventions (Canonical Form for Generating LLMs)

The canonical way to write a spec is defined as part of the language
specification. If the notation wavers on every generation, diff-based repair
breaks (G2).

- Indentation 2 spaces, one statement per line.
- Naming: action = `snake_case` verb phrase; invariant / reachable / type /
  enum / struct = `PascalCase`; const = `UPPER_SNAKE`.
- Group `requires` at the head of the action body (interleave only when a
  `let` or `is some` binding is needed). Put `ensures` at the end.
- In new v1 specs, use domain types; do not use `const` + `in lo..hi`.

---

## 5. Action Semantics

A spec defines a transition system (S, I, →). S is the set of all assignments
to state variables, I is the set of states satisfying `init`, and → is the
transition relation defined below. It extends v0 semantics without changing them
at all (G4).

1. **Action instance** = action name × parameter values (enumerating all
   combinations of the declared bounded domains).
2. **enabled**: every `requires` is true in the current state σ (`is some(x)`
   means "a value exists" is true, and binds x to that value).
3. **One step** = **any one** of the enabled instances executes atomically
   (interleaving). Nondeterminism is in this choice only; the update of an
   enabled instance is deterministic.
4. **Simultaneous assignment**: the right-hand sides of the body, `if`
   conditions, and `let` values all read the **old state** σ. Variables not
   assigned do not change (the frame condition is automatic).
5. **`let x = expr`**: binds the value evaluated in the old state. Usable in
   requires, statements, and ensures from the declaration onward.
6. **`if c { ... } else { ... }`**: c is evaluated in the old state. Variables
   in the branch not executed remain unchanged per the frame condition.
7. **Write conflicts**:
   - Assigning the same **scalar variable** (or the same struct field) twice on
     the same execution path → `semantics` error (almost certainly a bug).
   - **Maps** compose writes to the same cell in textual order (last wins).
     The normal use of writing to a different cell in a `forall` loop is
     non-interfering. If the same index is syntactically written twice, `check`
     warns.
8. **`ensures p`**: postcondition. When enabled and a transition σ → σ' occurs,
   it checks that p is true in σ'. `old(expr)` inside p is evaluated in σ. A
   violation is reported with the same counterexample format as an invariant
   violation (§7.2).
   - Reason for adoption: it strongly exists in the LLM's training distribution
     in the form of Dafny / contract programming (G1), and lets a "miswritten
     update" be reported localized to that action's line (G2).

---

## 6. Properties and Automatic Checks

### 6.1 invariant (Safety)

Same as v0. Requires it to hold in all reachable states, including the initial
state.

In v0, having no invariant at all was an error, but in v1 the implicit invariant
of type bounds is always present, so it is **not an error** (a warning is
issued).

### 6.2 reachable (Reachability = Scenario Check)

```fsl
reachable FullLifecycle {
  exists o: OrderId { orders[o].status == Shipped }
}
```

An assertion that "there **exists a path that can reach** this state." It
corresponds to checking the happy path "the user can complete a purchase" in an
app spec.

- In BMC it is the satisfiability decision itself within depth K, so the added
  cost is nearly nil (G3).
- On success it returns a **witness trace** (the execution sequence leading to
  that state) in JSON. This becomes the input for the v2 implementation bridge
  (generating integration-test templates).
- Failure is a sign of "guards too strong / init wrong," and corresponds to a
  generalization of action coverage (the vacuity check).
- Design decision: full LTL or `eventually` is not adopted. Under unfair
  interleaving + bounded checking, "eventually always" carries no meaning and
  misleads both LLMs and humans (G4). Liveness is introduced in v2 along with
  fairness annotations (§10).

### 6.3 Automatic Checks (Always Run, Even Without Being Written in the Spec)

| Check | Content | Report |
|---|---|---|
| init satisfiability | Whether an initial state exists | `error` / `kind: "vacuous"` (same as v0) |
| type bounds | Whether all state variables of bounded types stay in range in all reachable states | `violated` / `invariant: "_bounds_<var>"` |
| action coverage | Whether each action becomes enabled at least once within depth K | `action_coverage` inside `verified` + warning (same as v0) |
| deadlock | Whether there is a reachable state with no enabled instance | Default is warning + reach trace. `--deadlock=error` promotes to `violated`; `--deadlock=ignore` suppresses (for specs with intentional terminal states) |

---

## 7. Verifier Interface

### 7.1 CLI

```
fslc check  <file.fsl>                          # syntax/name/type check only (for the fast loop)
fslc verify <file.fsl> [--depth K]              # BMC (default K=8)
                       [--engine bmc|induction] # induction: §9
                       [--k N]                  # max induction depth (default 1, induction only)
                       [--deadlock warn|error|ignore]
                       [--property Name]         # select one invariant
                       [--exclude-property Name] # repeatable; skip invariant/trans/leadsTo/reachable
```

- Output is always a **single JSON object to stdout** (same as v0).
- Exit codes: `0` = verified / proved (including all reachable satisfied),
  `1` = violated / reachable_failed, `2` = spec error (parse/type/…),
  `3` = verifier internal error.
- Because BMC checks in order from depth 0, the returned counterexample is the
  **shortest** (stated explicitly as a guarantee: the shorter the trace handed
  to the LLM, the higher the repair accuracy).
- `--property` is an invariant-only selector. `--exclude-property` resolves
  names across invariants, transition properties, `leadsTo`, and `reachable`;
  excluded properties are omitted from checking and from `*_checked`,
  `leads_to`, and `reachables` outputs. If both flags name the same invariant,
  exclusion wins.

### 7.2 Output JSON Schema v1

All output has a common envelope:

```json
{ "fsl": "1.0", "result": "...", "spec": "OrderWorkflow", ... }
```

**verified (bounded verification success):**

```json
{
  "fsl": "1.0",
  "result": "verified",
  "spec": "OrderWorkflow",
  "depth": 8,
  "invariants_checked": ["ShippedWasPaid", "RevenueConsistent", "_bounds_orders"],
  "reachables": {
    "FullLifecycle": { "witnessed_at_step": 3, "witness": [ /* trace */ ] }
  },
  "action_coverage": { "place": true, "pay": true, "ship": true, "cancel": true },
  "deadlock": { "found": false },
  "warnings": [],
  "note": "bounded verification: no violation within depth 8"
}
```

**violated (violation of invariant / ensures / type bounds):**

```json
{
  "fsl": "1.0",
  "result": "violated",
  "spec": "ShoppingCart",
  "violation_kind": "invariant",        // "invariant" | "ensures" | "type_bound" | "partial_op" | "deadlock"
  "invariant": "_bounds_stock",
  "loc": { "line": 8, "column": 5 },    // location of the violated property (or ensures)
  "violated_at_step": 4,
  "violating_bindings": [ { "i": 0 } ],
  "last_action": { "name": "checkout", "params": { "u": 1 },
                   "loc": { "line": 24, "column": 3 } },
  "trace": [
    { "step": 0,
      "state": { "stock": { "0": 1, "1": 1 }, "cart": { "0": null, "1": null } } },
    { "step": 1,
      "action": { "name": "add_to_cart", "params": { "u": 0, "i": 0 } },
      "changes": { "cart[0]": { "from": null, "to": 0 } },
      "state": { "stock": { "0": 1, "1": 1 }, "cart": { "0": 0, "1": null } } },
    ...
  ]
}
```

Differences from v0:

- `changes`: the **state diff** of each step. Keys are flattened strings of
  projection paths (`"stock[0]"`, `"orders[2].status"`), values are
  `{from, to}`. The LLM can track "which action broke what" without reading the
  whole state (G2).
- `last_action`: the action executed just before the violation and **its
  definition location**. It points directly at the first repair candidate
  (where to add a requires).
- Value display: enum is the member name, Option is `null`/value, Set is a
  sorted array.
- `violating_bindings` generalizes to nested forall (the form `[{"u":1,"i":0}]`).

**reachable_failed (scenario unreachable):**

```json
{
  "fsl": "1.0",
  "result": "reachable_failed",
  "spec": "OrderWorkflow",
  "unreached": [ { "name": "FullLifecycle", "loc": { "line": 40, "column": 3 } } ],
  "depth": 8,
  "action_coverage": { "place": true, "pay": false, "ship": false, "cancel": true },
  "hint": "within depth 8 no trace satisfies the property; guards may be too strong (see action_coverage), or increase --depth",
  "faithfulness_class": "intent_unexercised",
  "recommended_action": "add a single-shot reachable for the action / raise --depth"
}
```

**error (syntax / name / type / semantic error):**

```json
{
  "fsl": "1.0",
  "result": "error",
  "kind": "type",            // "parse" | "name" | "type" | "semantics" | "io" | "internal"
  "loc": { "line": 12, "column": 18 },
  "message": "map key type must be a bounded type (domain or enum), got Int",
  "expected": "a declared domain type, e.g. `type ItemId = 0..N`",
  "hint": "declare `type K = 0..<max>` and use `Map<K, ...>`"
}
```

The error classification is a fixed closed set, and the fields each
classification has are guaranteed by the schema (`parse`/`name`/`type`/
`semantics` always have `loc`).

### 7.3 Value Display and the Invisibility of Lowering

Internal encodings (the integer-ization of enums, the presence bit of Option,
the field splitting of structs) are **never leaked into the JSON**. The
vocabulary the LLM sees is made to match the vocabulary of the spec (G2). This
is a schema-level guarantee and is treated as a test target.

---

## 8. Repair Protocol (Behavioral Guide for the LLM)

The recommended mechanical repair procedure for each `result`. Provided as fslc
documentation and as system-prompt material.

| result / kind | Fields to read | Recommended action |
|---|---|---|
| `error` / `parse` | `loc`, `expected` | Fix the syntax on that line per `expected`. Re-`check` |
| `error` / `name`/`type` | `loc`, `hint` | Add a declaration or change the type. Re-`check` |
| `violated` / `invariant`/`type_bound` | `last_action`, `changes`, `violating_bindings` | First suspect a missing `requires` on `last_action` (the most frequent bug). If `changes` in the trace is as intended, suspect an error on the invariant side |
| `violated` / `ensures` | `last_action`, `changes` | Decide which of the action's update expression and the ensures is the spec intent, and fix the other |
| `violated` / `partial_op` | `last_action`, `hint`, `trace` | Add a guard with `requires q.size() > 0` etc., or strengthen the requires so it does not fire in the empty state |
| `violated` / `deadlock` | `trace` | If the terminal state is intentional, use `--deadlock=ignore`; otherwise weaken a guard or add an escape action |
| `reachable_failed` | `action_coverage`, `hint` | Suspect the `requires` and `init` of the action whose coverage is false. Next, increase `--depth` |
| `verified` but `action_coverage` has false | `warnings` | Vacuity. Fix the contradiction between requires and init |
| `unknown_cti` (v1.1, §9) | `cti` | Add an auxiliary invariant that excludes the CTI state and re-run |

Machine consumers can route the same table through `faithfulness_class`:
`partial_op_unguarded`, `frozen_only_invariant`, `intent_unexercised`, or
`liveness_not_refined`, each paired with `recommended_action`. These fields are
derived from existing `result` / `kind` / `violation_kind` diagnostics and do
not replace them.

Design implication: that **this table is closed** (every output has a next move)
is a design requirement of the v1 output schema. When adding a new diagnostic,
the recommended action must be defined at the same time.

---

## 9. Inductive Proof Engine (v1.1)

For the detailed algorithm, soundness conditions, and test plan, see
`docs/DESIGN-induction.md`.

`--engine induction` attempts an **unbounded-depth proof** of invariants by
k-induction.

- **base**: ordinary BMC (depth `--depth`). If violated, an ordinary `violated`.
- **step**: over a free state sequence σ₀..σₖ (no init), assuming "all
  invariants hold and the transitions are consecutive," decide with Z3 whether
  the target invariant breaks at σₖ. All invariants unsat → `result: "proved"`.
  sat → `unknown_cti` (a CTI that is not necessarily reachable).
- induction output does not include a `deadlock` field.

**proved:**

```json
{
  "fsl": "1.0",
  "result": "proved",
  "spec": "OrderWorkflow",
  "engine": "induction",
  "k_used": { "ShippedWasPaid": 1, "RevenueConsistent": 2 },
  "base_depth": 8,
  "invariants_checked": ["ShippedWasPaid", "RevenueConsistent", "_bounds_orders"],
  "action_coverage": { "place": true, "pay": true },
  "reachables": { "FullLifecycle": { "witnessed_at_step": 3, "witness": [ ... ] } },
  "warnings": []
}
```

**unknown_cti:**

```json
{
  "fsl": "1.0",
  "result": "unknown_cti",
  "spec": "OrderWorkflow",
  "invariant": "RevenueConsistent",
  "k": 2,
  "cti": {
    "states": [
      { "step": 0, "state": { ... } },
      { "step": 1, "state": { ... }, "action": { ... }, "changes": { ... } },
      { "step": 2, "state": { ... }, "action": { ... }, "changes": { ... } }
    ],
    "violated_at": 2
  },
  "hint": "this state sequence satisfies all invariants but leads to a violation; the start state may be unreachable — add an auxiliary invariant that excludes it, then re-run"
}
```

CTI → "proposing an auxiliary invariant" is the kind of inductive
generalization an LLM is good at, and puts the invariant-strengthening loop that
human experts perform directly onto the write→verify→repair loop. This is the
central bet of FSL's AI-native design in v1.

---

## 10. Roadmap

- **v1.0 (the core of this document)**: the type system (domain/enum/Option/
  struct/Set), `let`/`if`/`is`, `ensures`, `reachable`, `count`/`sum`,
  automatic bounds checking, deadlock check, JSON v1 (diff, loc, schema
  version), `fslc check`.
- **v1.1**: the k-induction engine (§9, implemented), `fslc scenarios`
  (generate integration-test template JSON from reachable witnesses and
  coverage traces = the first step of the implementation bridge),
  `Seq<T, N>` (capacity-bounded sequence, encoded as array + length,
  **implemented**), unsat-core hint for "which requires blocks enabled."
- **v2.0**: fairness annotations (`fair action ...`) and bounded `leadsTo`
  (**implemented lite**: see `DESIGN-temporal.md`), the body of the
  implementation bridge (**implemented**: `DESIGN-bridge.md` —
  `fslc.runtime.Monitor` / `fslc replay` / `fslc testgen`), composition of
  multiple specs (**implemented**: `DESIGN-compose.md` — `compose` /
  synchronized actions / `internal` / `specs/order_system.fsl`), and
  refinement (**implemented**: `DESIGN-refinement.md` — `fslc refine`).
  **All items of the v2.0 roadmap are implemented.**

---

## 11. Migration from v0

v0 specs pass through the v1 verifier unmodified (a complete superset). However,
the following deprecation warnings appear in `warnings`, each accompanied by a
mechanical rewrite hint:

| v0 style | v1 recommendation |
|---|---|
| `const MAXI = 1` + `i in 0..MAXI` | `type ItemId = 0..1` + `i: ItemId` |
| `Map<Int, V>` | `Map<ItemId, V>` (bounded key) |
| Sentinel value (`-1` = empty) | `Option<T>` |
| Hand-written invariant "value is ≥ 0" | Automatic bounds checking of a domain type |

---

## 12. Implementation Approach (Lowering onto the Current Codebase)

Every new feature can be lowered onto the existing structure of `grammar.py` /
`model.py` / `bmc.py` (tuple AST → spec dict → Z3 bounded expansion) **without
changing semantics**:

| Construct | Lowering |
|---|---|
| Domain type | Int + range metadata. Quantification and parameter expansion use the existing `eval_const` machinery as is. Bounds are added to `spec["invariants"]` as an implicit invariant |
| enum | Int over `0..n-1`. Reverse-mapped to member names on display |
| `Option<T>` | A (present: Bool, value: T) pair. `Map<K, Option<V>>` is two maps. `is some(x)` → present constraint + binding |
| struct | Split into per-field variables/maps (`orders__status`, etc.). `==` is the conjunction of field equalities |
| `Set<T>` | `Map<T, Bool>` (characteristic function). `size()` is the bounded sum Σ ite(m[i],1,0) |
| `count` / `sum` | Bounded expansion (isomorphic to the existing forall expansion) |
| `if` | Compute pend per branch, and compose per variable with `ite(c, then, else)` |
| `let` / `is some` binding | Extension of the `binds` dict (the same mechanism as the current quantifier binding; allows binding Z3 expressions in addition to Int values) |
| `ensures` | An additional check against the post-state of each transition (same push/pop as the BMC invariant check) |
| `reachable` | A satisfiability decision at each depth (just the opposite polarity of the invariant check) |
| deadlock | A satisfiability decision of "the negation of the conjunction of requires over all instances" |
| `changes` diff | Just comparing the display values of adjacent states during trace construction (zero verification cost) |
| location info `loc` | Attaching metadata to the AST with Lark's `propagate_positions=True` |

---

## Appendix A: Shopping Cart in v1

The v1 version of the same model as v0's `cart_buggy.fsl` / `cart_fixed.fsl`.
The sentinel value is gone, and `NoNegativeStock` is absorbed into the type.

```fsl
spec ShoppingCart {
  type UserId = 0..1
  type ItemId = 0..1
  type Qty    = 0..3        // stock is 0..3 — if it goes negative it is automatically violated

  state {
    stock: Map<ItemId, Qty>,
    cart:  Map<UserId, Option<ItemId>>
  }

  init {
    forall i: ItemId { stock[i] = 1 }
    forall u: UserId { cart[u] = none }
  }

  action add_to_cart(u: UserId, i: ItemId) {
    requires cart[u] == none
    cart[u] = some(i)
  }

  action remove_from_cart(u: UserId) {
    requires cart[u] != none
    cart[u] = none
  }

  action checkout(u: UserId) {
    requires cart[u] is some(i)
    requires stock[i] > 0          // ← removing this line returns a _bounds_stock violation in a shortest 4 steps
    stock[i] = stock[i] - 1
    cart[u] = none
    ensures stock[i] == old(stock[i]) - 1
  }

  reachable SoldOut {
    forall i: ItemId { stock[i] == 0 }   // a path exists that sells out all stock (detects over-strong guards)
  }
}
```

Even without writing a single line of invariant, the buggy version (no stock
guard) is detected as a `_bounds_stock` violation. This is a concrete example of
§1 G5 "structural elimination of pitfalls."

## Appendix B: Order Workflow (a Showcase of enum / struct / Set / Aggregation)

```fsl
spec OrderWorkflow {
  type OrderId = 0..2
  type Qty     = 0..5

  enum Status { Draft, Placed, Paid, Shipped, Cancelled }

  struct Order { status: Status, qty: Qty }

  state {
    orders:  Map<OrderId, Order>,
    shipped: Set<OrderId>,
    revenue: Int
  }

  init {
    forall o: OrderId { orders[o] = Order { status: Draft, qty: 0 } }
    shipped = Set {}
    revenue = 0
  }

  action place(o: OrderId, q: Qty) {
    requires orders[o].status == Draft
    requires q > 0
    orders[o].status = Placed
    orders[o].qty = q
  }

  action pay(o: OrderId) {
    requires orders[o].status == Placed
    orders[o].status = Paid
    revenue = revenue + orders[o].qty
  }

  action ship(o: OrderId) {
    requires orders[o].status == Paid
    orders[o].status = Shipped
    shipped = shipped.add(o)
    ensures shipped.contains(o)
  }

  action cancel(o: OrderId) {
    requires orders[o].status == Placed or orders[o].status == Paid
    if orders[o].status == Paid {
      revenue = revenue - orders[o].qty
    }
    orders[o].status = Cancelled
  }

  invariant ShippedWasPaid {
    forall o: OrderId { shipped.contains(o) => orders[o].status == Shipped }
  }

  invariant RevenueConsistent {
    revenue == sum(o: OrderId of orders[o].qty
                   where orders[o].status == Paid or orders[o].status == Shipped)
  }

  invariant NonNegativeRevenue { revenue >= 0 }

  reachable FullLifecycle {
    exists o: OrderId { orders[o].status == Shipped }
  }
}
```

**Consistency of aggregates** such as `RevenueConsistent` corresponds to the
most common spec bugs in app development (missed refunds, double counting), and
is a class of property that v0 could not express.
