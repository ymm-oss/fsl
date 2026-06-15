# FSL — `trans` (transition invariant / two-state safety) implementation design

Motivation: DOGFOOD-11 F24. In the self-spec we want to assert **transition prohibitions** such
as "after Reject it stays Nonconformant" or "after ToolFault it is unrepairable," but
conventionally the only way to express this indirectly was a ghost variable + a one-state
invariant, or embedding it into an action guard. `trans` is syntax for writing cross-action
two-state safety directly.

## 1. Syntax

```fsl
trans RejectIsSticky {
  old(status) == Nonconformant => status == Nonconformant
}
```

`trans <Name> [meta_tag] { <expr> }` is a top-level property declaration, the same as
`invariant` / `reachable` / `leadsTo`. `old(<expr>)` can be used with the same syntax as in
`ensures`.

## 2. Semantics

`trans P` evaluates, for every reachable transition `σ -> σ'`, the expression at `σ'`, with only
`old(e)` evaluated at `σ`. That is, it is a two-state predicate that generalizes an action's
individual `ensures` to the whole spec.

- At `t = 0` there is no prior state, so it is not checked.
- A violation gives `result:"violated"`, `violation_kind:"trans"`, `trans:"Name"` and the
  shortest trace.
- The success output keeps the existing `invariants_checked` and adds `transitions_checked`.

## 3. BMC

`_bmc_explore` evaluates all `trans` immediately after the existing invariant check, at each
step with `t >= 1`:

```
eval_expr(expr, states[t], {}, spec, old_state=states[t-1], in_ensures=True)
```

The gating of `old()` reuses the same evaluation path as `ensures`. The user-facing error is
phrased as "only inside ensures or trans," to avoid being misleading in a trans context.

## 4. Induction

The base case runs BMC as before, so a reachable `trans` violation is returned as an ordinary
`violated`.

The step case checks each `trans(σ0, σ1)` under `Inv(σ0)` and `T(σ0, σ1)`. If `¬trans` is
satisfiable, it is not `proved` but returned as `unknown_cti` with a two-state CTI. This is the
same reading as the existing CTI: "from an initial state that satisfies every invariant but may
be unreachable, there is a transition that breaks trans."

## 5. Temporal Hierarchy

- `invariant`: one-state safety. Holds in every reachable state.
- `trans`: two-state safety. Holds over every reachable transition, and `old()` can be used.
- `leadsTo`: liveness. Searches for a lasso / stagnation counterexample up to depth K.

"Once X holds, keep X for all following steps" is `trans`. "If X, then eventually Y" is
`leadsTo`.

## 6. Difference from `forbidden`

`forbidden` is a negative acceptance criterion that replays at check time that a concrete
operation sequence is rejected. It checks "this trace must not pass," such as a missing guard,
against a human-enumerated trace.

`trans` does not enumerate traces; it checks a property quantified over all actions, all
parameters, and all reachable transitions via BMC / induction. Concrete example:

```fsl
trans ToolFaultNotRepairable {
  old(status) == ToolFault => status == ToolFault
}
```

This forbids "any transition that exits ToolFault." Writing the same intent with `forbidden`
requires enumerating each escape-route candidate operation sequence individually, which is easy
to miss when a new action is added.
