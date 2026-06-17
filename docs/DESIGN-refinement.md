# FSL v2.0 — Refinement Checking Implementation Design

The refinement side of DESIGN-v1.md §10 v2.0 "Composition of multiple specs and
refinement." It verifies that "a detailed spec (impl) does not deviate from the
behavior of an abstract spec (abs)" via **bounded simulation checking using a
refinement mapping**.

Use case: first get the abstract spec to proved, then mechanically verify that
the detailed spec close to the implementation (including caches, intermediate
states, optimizations) is **faithful** to the abstract spec. In the LLM
workflow this becomes a division of labor: "a human/LLM reviews abs → the LLM
freely refines impl → the refine check guarantees faithfulness."

## 1. The Mapping File

A third file (it pollutes neither the impl nor the abs spec):

```fsl
refinement CartImplRefinesCart {
  impl CartImpl                       // spec name of the detailed spec (file passed on the CLI)
  abs  ShoppingCart                   // spec name of the abstract spec

  // For each abstract state variable, give the mapping expression from the impl state (all variables required)
  map stock[i: ItemId] = impl_stock[i] - reserved[i]
  map cart[u: UserId]  = impl_cart[u]
  map revenue          = ledger

  // Correspondence of impl action → abs action (all impl actions required)
  action impl_checkout(u) -> checkout(u)     // parameters may be expressions
  action rebalance(i)     -> stutter          // internal action (nothing happens in abs)
}
```

- `map <abs_var> = <expr>` — a scalar abstract variable. The expression
  references the impl's state variables and consts.
- `map <abs_var>[<binder>] = <expr>` — per-element mapping of a Map/Seq etc.
  The binder ranges over the abs-side key type (Seq is limited to the
  whole-mapping form `map q = <impl Seq expr>` only — in v2.0, Seq is limited to
  an isomorphic mapping (there is a Seq on the impl side too, passed as an
  expression)).
- `action <impl_action>(<formal parameter list>) -> <abs_action>(<expr list>) | stutter`
  The formal parameters are the parameter names of the impl action (matching
  order). The abs-side arguments are expressions using them and the impl state.
- The grammar does not coexist with existing `.fsl` files (an **independent
  file** with `refinement` at the top level. Parsing adds `refinement_def` to
  the same Lark grammar).

### 1.1 Requirements-layer action-level `maps stutter`

The requirements dialect can embed the action correspondence that would
otherwise be written in the separate refinement mapping file. For an unbranched
requirements action, `maps` may appear directly on the action declaration:

```fsl
spec AbsTick {
  type K = 0..1
  state { x: K }
  init { x = 0 }
  action tick() { x = 1 }
}

requirements ImplTick {
  implements AbsTick from "refine_abs.fsl" {
    map x = y
  }
  type K = 0..1
  state { y: K }
  init { y = 0 }
  requirement REQ-TICK "tick is internal" {
    fair action tick() maps stutter {
      y = y
    }
  }
}
```

This expands to a kernel action plus an inline action correspondence equivalent
to `action tick() -> stutter` in a refinement file. The `stutter` rule is the
same as §2: the abstract state after mapping must be unchanged by the impl
step.

Observed result: `fslc check refine_impl.fsl` returned `result:"ok"` with
`implements:{abs:"AbsTick", result:"refines"}`, and
`fslc verify refine_impl.fsl --depth 1` returned `result:"verified"` with the
same implements result.

## 2. Checking Semantics (Bounded Forward Simulation)

α(s) := the mapping that defines the impl state → abs state mapping.

1. **init correspondence**: for the impl's initial state s₀, α(s₀) satisfies the
   abs init constraints. Counterexample: `refinement_failed` / `at: "init"`.
2. **transition correspondence**: for a reachable impl transition
   s →[a, params] s':
   - `a -> stutter` case: **α(s') == α(s)** (logical equality reuses leadsTo's
     `_logical_eq`).
   - `a(p…) -> b(e…)` case: the instance of abs action b (arguments = the
     evaluated values of e) is **enabled** in α(s) (its requires hold), and the
     result of applying b's update to α(s) is **logically equal to α(s')**.
3. The check is done over the impl's BMC expansion (depth K): at each step t,
   for each impl instance, if "the choice is that instance ∧ the negation of the
   correspondence condition" is sat, it is a violation. The impl trace + the abs
   states before and after α are returned as the counterexample.

The abs-side invariants are not checked (verifying/proving abs separately is the
premise. However, if α(s₀..s_K) breaks an abs invariant, that usually manifests
as a transition-correspondence violation itself). The abs-side automatic bounds
(_bounds_*) are likewise out of scope — however, it is not necessarily the case
that α's value escaping the abs type range is detected naturally as a
transition-correspondence violation (because it cannot match b's update result),
so **the type-bounds check of α(s_t) alone is performed additionally**
(`map_out_of_bounds` violation; this can directly point out the typical mapping
expression bug).

**Check order**:
- **Between steps (t>0)**: the transition-correspondence check (s_{t-1}→s_t) is
  done **before** the type-bounds check of α(s_t). When transition
  correspondence and a bounds violation occur simultaneously, e.g. due to guard
  weakening, prioritize reporting the root cause `abs_requires_failed`. The
  bounds check is applied only to α(s_t) after the previous step's transition
  correspondence has held.
- **Initial state (t=0)**: the type-bounds check (`map_out_of_bounds`) is done
  **before** the init-correspondence check. Because init correspondence nearly
  implies that α(s₀) is within the abs type range (if out of range, init
  correspondence usually also fails), reporting a range escape as
  `map_out_of_bounds` rather than as a general "init mismatch" can directly
  point out the mapping-expression bug (the typical one that produces an
  out-of-range initial value), which is more useful. A mismatch that is in
  range but with a different value is reported as before as an
  init-correspondence violation (`abs_state_mismatch`).

## 2.5 Conditional Expressions in Mapping Expressions (v2.2 — Resolving DOGFOOD-3 F9)

**Only in the expressions of the mapping file** is a conditional expression
allowed:

```fsl
refinement SeatImplRefinesBooking {
  impl SeatBookingImpl
  abs  SeatBooking

  // A mapping of an Option value that depends on a state tag can be written
  map seats[s: SeatId] =
    if slots[s].st == Sold then slots[s].holder else none

  action sell(s, u)    -> book(s, u)
  action hold(s, u)    -> stutter
  action expire(s)     -> stutter
  action confirm(s)    -> book(... )   // etc.
}
```

- Syntax: `if <expr> then <expr> else <expr>` (else required; nesting allowed.
  `then`/`else` are keywords only inside the mapping-expression grammar). It
  **cannot be used in ordinary .fsl spec files** (grammatically, it appears only
  in expressions inside refinement).
- Typing rule: both arms of then/else are the same logical type. Option vs
  Option (including none), enum vs enum, Int/domain vs Int/domain, Bool vs Bool,
  struct vs struct are allowed. A type mismatch is `kind: "type"` at check time.
- Semantics (lowering): Z3's `If`. When the arms are Option, present/value are
  each composed with ite (`If(c, p1, p2)` / `If(c, v1, v2)`); a struct is ite
  per field (the same convention as the existing merge of an if statement). The
  value of a none arm is don't care (a free variable is fine — if present is
  false it is not read).
- Allowed positions: the right-hand side of `map`, and the argument expressions
  of `action ... -> b(<expr list>)`.
- An AST node `("ite", c, a, b)` is added to eval_expr (a general-purpose
  implementation), but it is not generated from the body-spec grammar. If in the
  future this is opened to general expressions, additional design such as the
  path condition of partial_op would be needed, so it is not opened in this
  release (this limitation is also stated in LANGUAGE.md).

## 3. CLI / JSON

```
fslc refine <impl.fsl> <abs.fsl> <mapping.fsl> [--depth K]
```

Success:

```json
{ "fsl": "1.0", "result": "refines", "impl": "CartImpl", "abs": "ShoppingCart",
  "checked_to_depth": 8,
  "action_map": { "impl_checkout": "checkout", "rebalance": "stutter" } }
```

Violation:

```json
{ "fsl": "1.0", "result": "refinement_failed",
  "impl": "CartImpl", "abs": "ShoppingCart",
  "at": "init" | "step",
  "violated_at_step": 3,
  "impl_action": { "name": "rebalance", "params": {...}, "loc": ... },
  "kind": "abs_requires_failed" | "abs_state_mismatch" | "stutter_changed_abs"
        | "map_out_of_bounds",
  "impl_trace": [ ...existing trace format... ],
  "abs_before": { ...logical state of α(s)... },
  "abs_after_expected": { ...after applying b... } | null,
  "abs_after_actual": { ...α(s') ... },
  "mismatch": ["stock[1]", ...],            // logical paths where equality broke (as far as known)
  "hint": "the impl step does not correspond to the mapped abs action; fix the map expressions, the action correspondence, or guard the impl action" }
```

exit: refines = 0, refinement_failed = 1, error = 2/3.

Static checks (`kind: "type"` error, exit 2):
- An abs state variable that is not mapped / a nonexistent variable or action
  name
- An impl action with no correspondence
- A type mismatch of a mapping expression or argument expression (matched
  against the expected type on the abs side)
- When abs has ensures: since the correspondence check is done with "requires +
  body update," ensures is **assumed to be separately verified on the abs side**
  (stated in the note)

## 4. Implementation Notes

- The two specs are handled in the same Z3 context. The abs-side state does not
  create concrete variables; **α(s_t) is built as an expression** (a dict
  associating the map expressions, evaluated over the impl state variables, to
  the abs logical variables). The abs action's requires/update work as is if you
  pass "state dict = α's expression dict" to the existing
  `eval_expr` / `compute_updates` (build α at the **physical level** so that
  physical variable names match: Option is present/value, struct is field-split,
  Seq is data/len).
- The per-element map mapping `map stock[i] = expr` is **substituted on the read
  side** rather than as a Lambda/Store construction over the abs's physical Map
  variable: hold α as "physical variable name → (Z3 expression or keyed
  expression template)" so that `Select(stock, k)` during abs expression
  evaluation can be replaced by `expr[i := k]`, and add a hook to eval_expr's var
  resolution… that is invasive, so instead **enumerate the keys boundedly and
  build a concrete Array expression with a chain of Z3 K(ArraySort) + Store** (the
  keys are bounded, so a sequence of Stores can write it exactly). This one
  avoids touching existing code.
- The stutter / correspondence check expressions are push/pop per step t and per
  instance. They run on top of PERF1's shared expansion and expression cache.
- The counterexample's `abs_before/after` display applies `logical_state_values`
  to the values obtained by evaluating α's expression dict in the model.

## 5. Test Plan (tests/test_refine.py) + Sample

Sample: `specs/cart_impl.fsl` (a refinement of ShoppingCart. Example: it has
reserved stock `reserved`, and `reserve` (an internal state change equivalent to
a stutter, but the map absorbs it with `impl_stock - reserved` so as not to
change abs's stock) → `impl_checkout` consumes the reserved stock) +
`specs/cart_refines.fsl` (the mapping).

1. **Positive case**: cart_impl refines ShoppingCart (refines / exit 0).
2. **stutter violation**: a modification where an internal action changes the
   post-map abs state → stutter_changed_abs, with the variable path in mismatch.
3. **requires violation**: a modification where impl weakens a guard (impl allows
   a situation corresponding to abs's checkout `stock[i] > 0`) →
   abs_requires_failed.
4. **update mismatch**: a bug in the map expression (sign error etc.) →
   abs_state_mismatch.
5. **init mismatch** → at: "init".
6. **static checks**: missing map / unknown action / missing correspondence →
   kind: type, exit 2.
7. **bounds**: the mapping value is out of the abs type range → map_out_of_bounds.
8. No regression of existing features (refine is a completely independent CLI
   path).

## 6. Documentation Reflection

- A "refinement" section in LANGUAGE.md (mapping syntax, check content,
  workflow).
- A note in DESIGN-v1.md §10. Add the command to the README.

## 7. Chain Checking (Mapping Composition / v2.x)

For a layer chain business ⊒ requirements ⊒ design …, check end-to-end
faithfulness by composing adjacent mappings to **directly check lowest ⊒
highest**. Previously you could only `refine` adjacent pairs individually, and
ensuring the bottom preserves the top's contract meant either implicitly trusting
transitivity or hand-writing the composed mapping.

CLI:

```
fslc refine <low> <mid> <map_lm> <top> <map_mt> [<next> <map> ...] [--depth K]
```

After the first `(impl abs map)`, each appended `(abs map)` extends the chain by
one layer. `mappings[i]` treats `specs[i]` as impl and `specs[i+1]` as abs.

**Soundness**: fslc's refine is a step-local check (it maps each impl transition
to an abs transition/stutter at the same step), so bounded refinement is
transitive at the same depth K. If both low→mid and mid→top refine at depth K,
then since stutter does not increment the step number, low→top also refines at
depth K. Therefore the chain check is **equivalent** to "all adjacent links hold
at depth K" (demonstrated by `examples/refinement_chain`; a spike confirmed
3 layers mid/bot/top + indexed map + parameterized action).

**Implementation** (`refine_chain`):

- The state mapping is **composed at the Z3 level**: α_AC(s) =
  `build_alpha(build_alpha(s, map_AB, A, B), map_BC, B, C)`. The output of
  `build_alpha` (a dict expressing B's physical state in Z3 expressions over A)
  is passed as is to the input state of the next `build_alpha`. Since AST
  substitution is avoided, indexed map, Option, struct, and Seq are also composed
  with the existing `eval_expr` unchanged.
- The action correspondence is composed by folding: `a -> stutter` is stutter,
  `a -> b -> c` binds b's formal parameters to a's mapping argument expressions
  and composes. It is unsupported only when the argument expression reads the
  **intermediate-layer state** (`kind: type` error; in practice, arguments are
  mostly parameter references).
- The check body runs the existing `refine()` with the composed α (`alpha_fn`)
  and the composed action correspondence (the check loop is unmodified).
- On failure, the adjacent links are re-checked in order, and the first broken
  link is returned as `failed_link: {from, to, kind}` (the cause is easier to
  pinpoint than from a composed end-to-end trace).

**Propagation premise (liveness is separate)**: even if the chain is `refines`,
only safety propagates. The top-level liveness (`leadsTo`/`responds`) is re-
verified at each layer (see the note in `DESIGN-layers.md` §6,
`examples/refinement_liveness`).
