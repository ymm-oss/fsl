Fixed (#1109): the business dialect now rejects `with`, `when` and `set` on a
transition instead of silently dropping them. `lower_business` never read a
transition's inputs, guard or assignments, so `check` answered `ok` for a
guard that was never applied and an assignment to a field that was never
declared, and `verify` then answered for that other model. The error is
positioned at the transition and names the `requirements` dialect, whose
`process` gives the three clauses meaning; a control confirms the same
process without them, and every existing business example, still passes.
