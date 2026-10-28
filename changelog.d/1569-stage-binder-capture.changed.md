Changed (#1569, breaking): legacy requirements transition guards such as
`exists c: Amount { stage(c) == Approved and amount == c }`, with a process field
`amount`, now fail `check`, `kernel`, and `verify` with exit 2, `kind: "semantics"`,
and `stage(c) refers to type 'Amount', which has no process`. A field named `c`
is not required. Previously, `check` accepted the guard and `verify` reported
`verified`: capture-avoiding substitution renamed the non-entity binder without
renaming the stage operand, so `stage(c)` silently read the transition's own
instance. The same invalid guard consistently using binder `d` already failed.
No checked-in spec or example has this removed shape; the 597-form base
`1844fa74` / head `7b9b34b3` sweep showed no corpus acceptance transitions.
Migration: `stage(...)` takes an entity parameter or binder. To read the
transition's own stage, use
`exists n: Amount { stage(c) == Approved and amount == n }`, keeping `c` for the
entity and a different name for the non-entity binder; the rewritten guard passes
`check`.
