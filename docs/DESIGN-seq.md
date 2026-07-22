# FSL v1.1 — `Seq<T, N>` (capacity-bounded sequence) Implementation Design

Status: adopted feature design with migration-era implementation notes. The type
and operation rationale remains useful, but current language semantics are
governed by
[`LANGUAGE.md`](LANGUAGE.md#5-semantics) and native transition/failure behavior by
[`DESIGN-kernel-contract.md`](DESIGN-kernel-contract.md#transition-and-failure-semantics).
Where this record describes the original solver/Python evaluation strategy, it is
migration evidence rather than current implementation authority.

The last v1.1 item in DESIGN-v1.md §10. It expresses "ordered bounded
collections" such as FIFO queues and append-only logs. The design principles are
the same as the existing ones: G1 (easy to generate), G5 (structural elimination
of pitfalls = implicit checking of partial functions), and the same assignment
idiom as Set.

## 1. Syntax and types

```fsl
const CAP = 3
state {
  queue: Seq<JobId, CAP>,    // capacity is a const or an integer literal
  log:   Seq<Qty, 5>
}
```

- `Seq<T, N>`: T is **a scalar type only** (domain type / enum / Bool / Int).
  N is a positive constant expression (an integer literal or a const name).
- Allowed positions: **the type of a state variable only**. It cannot be used in
  a struct field (added to BUG11 checking), a Map value, a Set element, or a Seq
  element — rejected at the `check` stage with `kind: "type"` + a hint.
- Literals: `Seq {}` / `Seq { 1, 2 }` (number of elements ≤ N; exceeding it is a
  type error at check time). As with Set, it can only be written on the
  right-hand side of an assignment.

## 2. Operations (all pure, the same "reassignment" idiom as Set)

| Expression | Meaning | Partiality |
|---|---|---|
| `q.size()` | current length | none |
| `q.push(e)` | a new sequence with e appended at the end | **type_bound** when full (§4) |
| `q.pop()` | a new sequence with the head removed (FIFO dequeue) | **partial_op** when empty (§5) |
| `q.head()` | the head element | **partial_op** when empty (§5) |
| `q.at(i)` | the i-th element (0-based) | **partial_op** when out of range (§5) |
| `q.contains(e)` | ∃i < size: at(i) == e | none |
| `q == q2` / `!=` | equal length and all elements equal (prefix comparison) | none |

- v1.1 is the minimal set needed for FIFO. Stack operations such as `pop_last`
  come once a real example arises (the DESIGN-v1.md policy).
- Use it via reassignment such as `q = q.push(x)`, `q = q.pop()` (the same form
  as Set's `add`/`remove`).
- `q = q.push(a).push(b)` within a single action is legal as a method chain.

## 3. Lowering

Physical variables (the existing `__` split scheme):

- `q__data`: `Map<0..N-1, T>` (Z3 Array)
- `q__len`: Int

Each operation:

- `size()` → `q__len`
- `push(e)` → data' = Store(data, len, e), len' = len + 1 (unconditional; see §4)
- `pop()` → data' = ∀i < N-1: data'[i] = data[i+1] (shift), len' = len - 1
- `head()` → `Select(data, 0)`
- `at(i)` → `Select(data, i)` (i is an expression. No range clamping — the check of §5 protects it)
- `contains(e)` → `Or(And(0 < len, data[0] == e), ..., And(N-1 < len, data[N-1] == e))`
  (bounded unrolling by N)
- `==` → `len1 == len2 ∧ ∀i ∈ [0, N-1]: i < len1 => data1[i] == data2[i]`
- literal `Seq { a, b }` → data[0]=a, data[1]=b, len=2 (the rest are don't-care)

Tail values beyond len are **don't-care** (not constrained, not read, not displayed).

## 4. Automatic bounds check (`_bounds_q`)

```
0 <= q__len <= N
∧ ∀i ∈ [0, N-1]: i < q__len => (lo <= q__data[i] <= hi)   // when T is a bounded type
```

- A `push` when full makes len = N+1 and violates `_bounds_q` → the usual
  `violated` / `violation_kind: "type_bound"`. The repair hint is obvious from
  the bindings and last_action (add a requires `q.size() < N`). This is identical
  to the existing design "do not assume bounds, check them" (the design decision in §3).
- It also enters the step premise of induction like the other `_bounds_*` (to prevent ghost CTIs).

## 5. Implicit checking of partial operations (`partial_op` — a new violation_kind)

`pop()` / `head()` / `at(i)` are partial functions. When they appear **inside an
action body / requires / ensures**, the well-definedness of that operation is
attached to the transition as an implicit check:

- Check content: in the transition where the action fires,
  `pop`/`head` → `q.size() > 0`, `at(i)` → `0 <= i < q.size()`.
- On violation, `violated`, `violation_kind: "partial_op"`, the `invariant` field
  is `"_partial_<action name>"`, loc is the expression in question, hint:
  `"guard the action with requires q.size() > 0 (or bound the index)"`.
  With a trace (it can ride on the same mechanism as ensures violations).
- **Historical solver treatment inside requires** (evaluation order): since the evaluation of the
  requires conjunction does not short-circuit, when `requires q.size() > 0` and
  `requires q.head() == x` appear side by side, the partial_op check is performed
  only on **the transition where all requires hold** (= in the branch where the
  guard fails, head()'s garbage is treated as not read). This makes the standard
  idiom (writing the guard requires first) pass correctly.
  This paragraph records the original lowering; it does not define current guard
  evaluation. Current guard and transition-outcome behavior must be read from the
  maintained language and Kernel contracts linked above.
- **Inside invariant / reachable**: no implicit check is attached (a state
  property has no "firing"). The value of an out-of-range read is **unspecified**
  (don't-care). Present the guarded idiom
  `forall k in 0..CAP-1 { k < q.size() => P(q.at(k)) }` as the standard form in
  the LANGUAGE document. An invariant that reads garbage without a guard can
  yield a spurious violation, but looking at the trace makes it clear (a v1.1
  pragmatic call; no warning is emitted).

  **Inter-engine difference (known)**: an invariant containing an unguarded
  partial Seq operation (`head`/`pop`/`at`) reads the don't-care value
  **symbolically** in `verify`/`prove` (BMC) (searching for a counterexample with
  any value), whereas in the runtime `Monitor` (the concrete interpreter /
  conformance test) it becomes a concrete out-of-range read and returns
  `partial_op`. don't-care is essentially "symbolic = any vs concrete = single,"
  and agreement of both engines on an unguarded invariant cannot be guaranteed in
  principle. **If written with the guarded idiom, the results of both engines
  agree** (verified). Therefore, when using a partial Seq operation in an
  invariant / reachable, attaching a size guard is strongly recommended.

## 6. JSON display

- State display: `"queue": [1, 2]` (up to len, as an array of logical values; empty is `[]`).
- diff (`changes`): the from/to of the whole sequence (`"queue": {"from": [1], "to": [1, 2]}`).
  No per-element diff (because a shift moves all elements).
- `violating_bindings` / CTI / scenarios use the same display (all output lines up
  by just adding a seq branch to the existing `logical_state_values`).

## 7. Strengthened verification at the check stage (generalization of BUG11)

Verify the type of a state variable with a **whitelist** (including the fix for
the problem where `Map<K, Set<K>>` currently passes through check and produces a
wrong message at verify):

```
scalar   := Int | Bool | domain | enum
legal as a state variable:
  scalar | Option<scalar> | struct (all fields scalar)
  | Map<bounded-scalar, scalar | Option<scalar> | struct>
  | Set<bounded-scalar>
  | Seq<scalar, N>
```

- Anything other than the above (Set/Map/Seq as a Map value, struct/Option as a
  Set element, etc.) is rejected at check with `kind: "type"` + a hint showing
  "the type combinations legal in v1."
- Regression test: `Map<K, Set<K>>` becomes a type error at the check stage.

## 8. Test plan

1. **FIFO basics**: push×2 → head is the first element, pop → head is the second,
   size consistent. Confirm a witness via reachable.
2. **push when full**: capacity 2 with 3 pushes → `violated` / `type_bound` / `_bounds_*`.
3. **empty pop / empty head**: an unguarded action → `violated` / `partial_op` /
   loc and hint. With a guard (requires q.size() > 0) it is verified.
4. **guard idiom for head inside requires**: the two clauses `requires q.size() > 0`
   + `requires q.head() == 0` → in the empty state the action merely becomes
   disabled and there is no partial_op violation.
5. **at + forall guard idiom invariant**: an append-only-log type
   `forall k in 0..N-1 { k < log.size() => log.at(k) <= k }` is verified.
6. **Seq ==**: a requires of the form `q.push(1) == q2` works correctly
   (isomorphic to the struct == regression).
7. **contains**.
8. **JSON display**: the state is an array, and internal names (`__data`/`__len`) do not appear.
9. **check rejection**: a Seq in a struct field, `Map<K, Seq<...>>`, `Map<K, Set<K>>`,
   and a literal with more than N elements.
10. **induction**: the FIFO spec becomes proved (confirming that _bounds_q enters the premise).
11. **scenarios**: cover_* and reach_* are generated for the FIFO spec.

## 9. Reflecting into documentation

- Add a Seq subsection to DESIGN-v1.md §3 and a v1.1 completion mark to the §10 roadmap.
- Add the `Seq` type and literal to the grammar EBNF (§4).
- Add `"partial_op"` to the `violation_kind` enumeration (§7.2).
