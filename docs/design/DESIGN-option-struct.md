# FSL v2.1 — implementation design for `Option<scalar>` struct fields

This feature legalizes the natural reservation model
`struct Res { item: Option<ItemId> }` without a meaningless sentinel value. The
design approach is a **composition** of existing lowerings; no new semantics are
introduced.

## 1. Language specification changes

- Types allowed for struct fields: scalar (Int / Bool / domain type / enum) **plus
  `Option<scalar>`**. Set / Map / Seq / struct continue to be rejected at check
  time (the hint is updated to reflect "or use Option<scalar>" added to the
  current wording). At v2.1, `Option<Option<…>>` was also rejected; that
  rejection is superseded by the accepted #841 recursive nested-Option design.
- Expressions and statements use the same vocabulary as scalar Option:
  `s.v == none` / `!= none` / `s.v == some(e)` / `!= some(e)` /
  `s.v is some(x)`, `s.v = some(e)` / `= none`, and literals
  `S { v: none }` / `S { v: some(e) }`. Equality is structural and does not bind;
  `is some(x)` retains its lexical binding role.
- Whole-struct `==` / `!=`: Option fields use **logical equivalence**
  (both present and equal ∧ present ⇒ values equal). Same convention as `_logical_eq`.

### Repeated fields in a declaration (#1692)

**Breaking removal (#1692).** `SpecItem::Struct` and the kernel kept both copies of a repeated field.
`check` admitted a value by the first type and the bounds refused the same value, so
`struct P { x: K, x: Bool }` passed `check` and `verify` reported `p = {"x": 1}` out of `_bounds_p`,
which `x: K` alone accepts. Which copy the bound reads was inferred from that and not traced in the code.
`build_model` now refuses the first
repeated field before it reads the field types, with `kind: "name"` and `loc` at the repeated name, as
`duplicate enum member` and `duplicate state variable` do. `SpecItem::Struct` gains `field_spans`,
parallel to `fields`, for that location. Two copies of one type are refused too: they lose nothing, and
that matches the other repeated declarations (#1686). A struct nothing reads is refused as well.

Routes on one document, `FSLC_CACHE=off`, base `b2cdb394fedcba015261df05587c4721583329e3`, head `66ca55474a05a8aefc9d92b29095505f1f1742af`. The literal is `P { x: 1 }`
and the invariant `p.x == 1`.

| Route | Base, `x: K, x: Bool` | Base, `x: Bool, x: K` | Base, `x: K, x: K` | Head, any |
|---|---|---|---|---|
| `check` | exit 0, `ok` | exit 2, `Int` not assignable to `Bool` at 5:10 | exit 0, `ok` | exit 2, `kind: "name"`, at 3:20, or 3:23 for `x: Bool` first |
| `verify` on bmc | exit 1, `violated` | exit 2 | exit 0, `verified` | exit 2 |
| `verify` on explicit, induction | exit 1, `violated` | exit 2 | exit 0, `proved` | exit 2 |
| `fsl-bfs` | exit 0, a `type_bound` violation of `_bounds_p` | exit 2 | not run | exit 2, the message on stderr with no position |
| `sweep --depth 0..1` | exit 1, `sweep_failed` | exit 2 | exit 0, `sweep_passed` | exit 2 |
| `mutate`, `html`, `ledger` | exit 1 | exit 2 | exit 0 | exit 2 |
| `scenarios`, `testgen` | exit 2, `SMT term sort mismatch` | exit 2 | exit 0 | exit 2 |
| `replay --from-log`, a log with `x` 1 | exit 1, `nonconformant` | exit 2 | exit 0, `conformant` | exit 2 |
| `refine`, either side, against the one-field spec | exit 2, `type 'P' is declared differently` | exit 2 | exit 2, the same | exit 2 |
| `document generate` | exit 2, `no requirement IDs` | exit 2 | exit 2, the same | exit 2 |
| `fmt --check` | exit 1, `format_check` | exit 2 | exit 1, `format_check` | exit 2, `kind: "semantics"`, no `loc` |
| `fmt` | exit 0 | exit 2 | exit 0 | exit 2, `kind: "semantics"`, no `loc` |
| `kernel` 1 and 2, `typestate`, `lint`, `migrate`, `explain`, `analyze`, `conformance`, `document claims`, `diff`, `replay --trace` | exit 0 | exit 2 | exit 0 | exit 2 |

`fmt` and `fsl-bfs` give a repeated `enum` member the same way, so that is how those two report a model
failure, not something this refusal adds.

| Other file or dialect | Base | Head |
|---|---|---|
| Component, action lifted, synchronized or internal, or struct unreferenced | exit 0, `ok` | `component "piece.fsl" failed to check (duplicate struct field 'P.x' at piece.fsl:3:20) at root.fsl:2:3`, `kind: "semantics"`, `loc` at the `use` |
| Inline `implements`, the other file a `spec` or `domain` | the next error of that root | `implements "abs.fsl" failed to check (duplicate struct field 'P.x' at abs.fsl:3:20)`, or `(duplicate struct field 'C.value' at absdom.fsl:3:34)` for a `domain`, `kind: "type"`, `loc` on the `implements` |
| `value_object`, `check`, `domain check`, `testgen`, `generate` | exit 2 at the `init` for a different type, exit 0 for the same type | `duplicate struct field 'C.value' at doc.fsl:3:34`, `kind: "name"`, `loc` there |
| `value_object`, `domain expand`, `domain analyze` | exit 0 | the same message without the file, and `expand` writes nothing |
| `requirements` with a struct, `fsl 5` `spec`, literate document | exit 0, `ok` | as a `spec`, at the repeated field |

The component's check runs before names are prefixed, so the report names `P` and not `part__P`. A
`value_object` is refused where `domain_kernel_source` and lowering both start, because `domain expand`
and `domain analyze` build no model.

An input that already failed keeps exit 2 and now reports the repeat, so its kind changes:
`x: Set<K>, x: K` from `kind: "type"`, `struct field 'P.x' has non-scalar type`, to `kind: "name"` at
3:25, and `x: Nope, x: K` from `kind: "type"`, `unknown type 'Nope'`, to `kind: "name"` at 3:23. A
reserved field name (`__x`) keeps its own refusal, which runs first.

In `fslc-lsp` the refusal is a diagnostic at the repeated field, `kind` `name`, `code` `FSL-NAME`
(`rust/fsl-lsp/tests/issue_1692_struct_field_declared_twice.rs`). The Worker was not run. Rust API:
`SpecItem::Struct` gains `field_spans`.

Migration: keep the one field you mean. There is no automatic migration.

On-disk transition set (empty). Base `b2cdb394fedcba015261df05587c4721583329e3`, head `66ca55474a05a8aefc9d92b29095505f1f1742af`, each binary built from its own tree in
its own target directory with Rust 1.98.0. Every `.fsl` under `specs/`, `examples/` and
`rust/fslc/tests/fixtures/` (650 forms: 23/195/432) ran with `FSLC_CACHE=off` and its own tree as cwd,
as `fslc check <path>` stable and with the next edition enabled, and as `fslc kernel <path>
--kernel-version 1|2`, twice per side. All 2600 pairs had equal complete stdout, stderr and exit with
no field excluded, and no run differed from its repeat: 0 ok-to-error and 0 error-to-ok transitions.
That includes `specs/order_system.fsl`, whose component `specs/payment.fsl` declares a struct. A pattern
scan, on the earlier base `1559e8eba`, of the repository's `.fsl`, `.rs`, `.md`, `.py`, `.ts`, `.js`, `.json` and `.html` files found 930
`struct` bodies and no FSL declaration repeating a field; the only hits were Rust structs. No required
test's inline fixtures were rewritten.

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

- Update LANGUAGE.md §2's table and whitelist, and the former sentinel workaround
  in the §9 idiom collection, to "writable directly from v2.1".
- Add a note on lifting the Option-field restriction in the "no struct nesting"
  paragraph of DESIGN-v1.md §3.4.
