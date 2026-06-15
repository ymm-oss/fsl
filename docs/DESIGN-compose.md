# FSL v2.0 — spec composition (compose) implementation design

DESIGN-v1.md §10 v2.0 "composition of multiple specs". It combines multiple verified
component specs into a single system spec by **namespaced interleaving composition**.

Design core: composition is defined as a **front-end AST transformation** (prefixed
merge). After lowering it becomes an ordinary single spec, so BMC / k-induction /
scenarios / coverage diagnostics / leadsTo / runtime Monitor / replay / testgen / refine
all run against the composed spec **with no changes whatsoever**.

## 1. Syntax

```fsl
compose OrderSystem {
  use ShoppingCart as cart from "cart_v1.fsl"
  use Payment      as pay  from "payment.fsl"

  // additional composition-side state (optional)
  state { orders_linked: Int }
  init  { orders_linked = 0 }

  // synchronized action: execute actions of multiple components together in one step
  action checkout_and_pay(u: cart.UserId, p: pay.PayId) =
      cart.checkout(u) || pay.capture(p) {
    requires pay.payments[p].amount > 0
    orders_linked = orders_linked + 1
  }

  // remove a component's action from standalone execution (only fires via synchronization)
  internal cart.checkout
  internal pay.capture

  // cross-cutting invariant / reachable / leadsTo (referenced via alias.var)
  invariant LinkedNonNeg { orders_linked >= 0 }
  reachable PaidOrder {
    exists p: pay.PayId { pay.payments[p].st == Captured }
  }
}
```

- `use <SpecName> as <alias> from "<relative path>"`: the path is relative to the compose
  file's directory. The spec name must match the spec name in the file. The alias is
  unique within the compose. A compose inside a compose is **not allowed** (v2.0 is a
  single level).
- A component's types, state, and actions are referenced as `alias.Name`.
- **Synchronized action** `action <name>(<params>) = <a>.<act>(<arg exprs>...) [ || <b>.<act2>(...) ]* { additional items }`:
  - requires = the conjunction of each component action's requires + additional requires
  - body = the union of each action's body + additional statements (simultaneous
    assignment; since the write targets are disjoint across components and additional
    statements can only write composition-side state, there is no conflict —
    synchronizing two actions of the same component is not allowed)
  - ensures are also inherited from each action
  - argument expressions may use composition-side parameters and consts
- `internal <alias>.<action>`: excludes that component action from standalone
  interleaving (it is executed only via a synchronized action).
- An ordinary `action` (without `=`) may also be written (a glue action; reads and writes
  composition-side state and component state directly via `alias.var`).

## 2. Semantics (= definition of the transformation)

In the stage before `build_spec`, the compose is **expanded into the AST of a single spec**:

1. For each use, parse the target file, and prefix all declaration names (the type
   namespace including type names and enum members, state variables, actions, and
   invariant/reachable/leadsTo names) with `<alias>__`, rewriting references inside the
   body. Enum **member names** are not prefixed (since the type carries the namespace,
   there is no collision — but even if two components export enums with same-named
   members, member resolution is done in type context, so it is a check error only when
   ambiguous).
2. Rewrite `alias.x` references in the compose body to `alias__x`.
3. Expand synchronized actions into a flat action by the rules of §1 (the parameter →
   argument-expression substitution is injected as a let binding).
4. Remove `internal`-designated actions from the spec's action list, and use them by
   **copying** the body when expanding synchronized actions.
5. The components' invariants / automatic _bounds_ / reachable / leadsTo / fair survive
   as-is under prefixed names (all are checked).
6. Display metadata: a physical variable `alias__x` is displayed in JSON as `alias.x`
   (a display-name map in logical_state_values. Traces, witnesses, CTIs, scenarios, and
   the Monitor state all follow).

Static checks (check stage, `kind: "type"`):
- missing use file / spec-name mismatch / duplicate alias
- failure to resolve `alias.x` (unknown alias / variable / action)
- referencing multiple actions of the same component in a synchronized action
- type mismatch of synchronized arguments / nonexistent internal target
- a compose inside a compose

## 3. CLI

No new command. `fslc check / verify / scenarios / replay / testgen` accept a compose
file as-is (if the parse result is a compose, it is expanded before normal processing).
A compose can also be passed to the impl side of `fslc refine` (since after expansion it
is a single spec).

## 4. Implementation notes

- grammar.py: `compose_def` (use / internal / sync action). `alias.x` collides with the
  existing `field` syntax (same form as `o.st`) — during compose expansion, "field
  access to a name in the alias set" is rewritten as a namespace reference, and otherwise
  treated as a struct field as before (expansion runs only for compose, so there is no
  impact on existing specs).
- New module `src/fslc/compose.py`: `expand_compose(ast, base_dir) -> ast`. A pure
  transformation rewriting the tuple of parsed AST. Wire it from the CLI so `base_dir` can
  be passed to `parser.parse` (add a compatible `parse(src, base_dir=...)` library API).
- Display-name map: give the spec dict `display_names: {phys/logical: "cart.stock"}`, and
  look it up at the display sites such as `logical_state_values`. runtime.py uses the same
  map.
- File reading rides on the io error handling of check/verify (`kind: "io"`, pointing at
  which use via loc).

## 5. Test plan (tests/test_compose.py) + sample

Sample: `specs/order_system.fsl` (almost the same as §1: cart_v1 + payment,
synchronization of checkout and capture, cross-cutting reachable).

1. **Positive case**: order_system is verified (coverage all true, PaidOrder witness).
   With `--engine induction`, proved (the components' auxiliary invariants take effect
   as-is).
2. **Synchronization semantics**: after checkout_and_pay, the cart-side stock decrement
   and the pay-side Captured happen in the **same step** (confirmed via the witness
   changes).
3. **internal**: cart.checkout alone does not appear in coverage (it is not in the action
   list); removing internal makes it fire standalone too.
4. **Cross-cutting invariant violation**: a deliberately broken composition (a glue action
   making orders_linked negative) → violated, with trace state keys in the `cart.stock`
   form.
5. **Static checks**: duplicate alias / unknown alias / spec-name mismatch / two-action
   synchronization of the same component → kind: type. Missing file → kind: io.
6. **Display**: across all JSON output (witness / scenarios / Monitor.state) the
   `cart.stock` display and no `__` leakage.
7. **runtime**: the Monitor runs as-is on a compose file (including replay).
8. No regression of existing specs (files not containing compose take the previous path).

## 6. Documentation reflection

- LANGUAGE.md "composition" section (syntax / synchronized-action semantics / internal).
- README / DESIGN-v1.md §10 note.
