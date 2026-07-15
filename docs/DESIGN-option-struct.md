# FSL v2.1 — implementation design for `Option<scalar>` struct fields

A feature whose real-world need was confirmed in DOGFOOD-1 F3. It legalizes
`struct Res { item: Option<ItemId> }`. The design approach is a **composition** of
existing lowerings; no new semantics are introduced.

## 1. Language specification changes

- Types allowed for struct fields: scalar (Int / Bool / domain type / enum) **plus
  `Option<scalar>`**. Set / Map / Seq / struct / `Option<Option<…>>` continue to be
  rejected at check time (the hint is updated to reflect "or use Option<scalar>" added
  to the current wording).
- Expressions and statements use the same vocabulary as scalar Option:
  `s.v == none` / `!= none` / `s.v == some(e)` / `!= some(e)` /
  `s.v is some(x)`, `s.v = some(e)` / `= none`, and literals
  `S { v: none }` / `S { v: some(e) }`. Equality is structural and does not bind;
  `is some(x)` retains its lexical binding role.
- Whole-struct `==` / `!=`: Option fields use **logical equivalence**
  (both present and equal ∧ present ⇒ values equal). Same convention as `_logical_eq`.

## 2. Lowering (composition of physical splits)

| Logical | Physical |
|---|---|
| Scalar `s: S` (where S has `v: Option<K>`) | `s__v__present: Bool`, `s__v__value: K` (other fields as before, `s__f`) |
| `m: Map<K2, S>` | `m__v__present: Map<K2, Bool>`, `m__v__value: Map<K2, K>` |

- Add a "struct field is option" branch to `expand_phys_var` / `phys_z3_sort`.
- `field` evaluation in `eval_expr`: an option-typed field returns
  `('option_val', present-expr, value-expr)` (merging with the existing path for
  scalar Option).
- Assignment (`compute_updates`): `s.v = some(e)` → two physical writes (present/value).
  The bulk assignment `s = S { v: none, ... }` also updates both physicals. Double-assign
  detection is per logical field (assigning to `s.v` twice is an error; present/value
  are internal, so invisible to the user).

## 3. Automatic bounds (`_bounds_*`)

Bounds of an option field: `present => lo <= value <= hi` (when K is bounded). Should
be writable as a composition of the existing `bounds_invariant_expr` for scalar Option.
It also automatically enters the induction step premise (via invariants).

## 4. Display

- Inside the struct dict, `"v": null` or `"v": value` (same convention as scalar Option).
- changes path: from/to in `res[0][item]` are null / value.
- witness / CTI / scenarios / violating_bindings / Monitor.state all follow
  (add an option branch to `logical_state_values` and the runtime's struct
  reconstruction).

## 5. Ripple sites (handle all of them)

1. **model.py**: relax the check_spec whitelist, expand_phys_var, bounds generation.
2. **bmc.py**: eval_expr's field evaluation / struct literal / struct equality
   (logical equivalence of option fields in `_struct_compare`) / compute_updates'
   field assignment and bulk assignment / logical_state_values / `_logical_eq` (the §2.3
   logical equivalence for leadsTo also compares option fields logically).
3. **runtime.py**: struct reconstruction / field read-write / equality in the concrete
   evaluator.
4. **refine.py**: α's physical-level construction correctly assembles the new physical
   names (`m__v__present`, etc.) (including the case where a struct value mapped
   per-map-element has an option field).
5. **compose.py**: type-reference rewriting should pass through the existing mechanism
   (regression check only).
6. **grammar**: no change needed (the type syntax was already parseable; check was
   merely rejecting it).

## 6. Verification (tests + rewrite of the real-world spec)

1. `tests/test_option_struct.py`:
   - For both scalar struct and Map<_, struct>: init literal, field assignment
     (some/none), `is some(x)` guard, `== none` requires, whole-struct `==`, automatic
     bounds (only when present), structural `s.v == some(e)`, JSON display
     (null / value, no leakage of `__present`/`__value`).
   - induction: an option-field-bearing spec becomes proved (no ghost CTI =
     confirmation of the bounds premise).
   - runtime Monitor: same spec, step → state display matches (add one new spec to the
     verify-witness diff-replay test).
2. **Rewrite `specs/inventory_reservation.fsl` into a natural form** (resolving F3):
   `struct Res { st: RState, item: Option<ItemId>, qty: Qty }`, init is `item: none`,
   `some(i)` on hold, returns to `none` on release. Add the invariant
   `FreeHasNoItem { res[r].st == Free => res[r].item == none }`, and change the
   `res[r].item == i` condition in Conservation to the form
   `res[r].item is some(j) and j == i`.
   **After the rewrite it must remain verified (depth 5) + induction proved** (if it is
   no longer proved, report the CTI — I will decide whether auxiliary invariants are
   needed).
3. The existing 115 tests are green without modification (diff tests referencing
   inventory_reservation should automatically follow the spec rewrite — report the cause
   if they break).

## 7. Documentation

- Update LANGUAGE.md §2's table and whitelist, and the description of the F3 workaround
  in the §9 idiom collection, to "writable directly from v2.1".
- A note on lifting the Option-field restriction in the "no struct nesting" paragraph of
  DESIGN-v1.md §3.4. A "resolved" note on DOGFOOD-1 F3.
