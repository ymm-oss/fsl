Changed (#1547, breaking removal): native `check` rejects pattern variables
outside their true continuation, including disjunctive/comparison/implication
escapes and non-conjunctive `requires` exports. All nine scope consumers share
one operator policy; legal `and`/`=>` RHS bindings and conjunctive guard exports
remain supported. Errors retain `kind: "semantics"`, exit 2 and the containing
clause/arm location (action location inside `requires`/`ensures`). For a policy
allowing `none` but requiring payload 1 when present, use
`(o is some(v)) => v == 1`, equivalently
`o == none or (o is some(v) and v == 1)`; `and` alone instead rejects `none`.
The own-tree `<binary> check <path>` sweep from
`1844fa74a717ba9229e0b292d9421c0939016e5e` to
`b47b72f06ec214038f57205699830e91a11b058a`, repeated twice with complete
stdout/stderr/exit comparison, found zero removals/output changes in 597 corpus
files and 1,713 existing complete static Rust-test literal occurrences. The 47
new authored sources / 94 direct-compose routes have 56 removals (28 cells,
both routes), enumerated in
[the scope design](../docs/design/DESIGN-pattern-binding-scope.md#3-true-continuation-enforcement-1547),
plus two existing-error diagnostic changes. No acceptance expansions or repeat
differences occurred. Another 1,737 interpolated/incomplete document candidates
were inventoried, not executed; runtime-generated fixture graphs are not covered.
