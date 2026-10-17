Fixed (#1259): `x is some(v)` whose `v` repeats a state variable could make
bmc and induction disagree with explicit-state: the symbolic engines bound `v`
to the absent payload even when `x` was `none`, so `p is some(exact) or exact <= 2`
was `violated` (or "undefined") under bmc/induction and `proved` under
explicit. `check` now rejects a pattern variable that repeats a name already
in scope — a state variable, constant, enum member, action parameter,
quantifier binder, `let`, or an earlier pattern binding (#1157 item 1) — as a
located semantics error (exit 2), and the symbolic `is some` keeps the outer
meaning of the name where the operand is absent, matching the concrete
evaluator. Migration: a spec that used such a collision now fails `check` and
every `verify` with exit 2 and `pattern binding '<v>' ... rename the pattern
variable`; rename the pattern variable to a fresh name. No spec in this
repository's corpus is affected. The frozen Python reference still accepts
the collision at `check`.
