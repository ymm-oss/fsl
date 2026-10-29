Changed (#1547, breaking removal): native `check` rejects pattern variables
outside their true continuation, including disjunctive/comparison/implication
escapes and non-conjunctive `requires` exports. Ten logical scope consumers and
the DDD frontend share one operator policy. Collision reservations remain
separate and base-equivalent; #1573 remains separate work. Legal `and`/`=>` RHS
bindings and conjunctive guard exports remain supported. Earlier newly invalid
references can precede legacy collision diagnostics, still exit 2/semantics.
Expression-if arm inference can report unbound then/else/nested references at
`6:32`/`6:39`/`6:48` before the former condition collision `6:12`; renaming only
the arm reference restores the complete base packet `6:12`. Def-call capture
diagnostics also intentionally become located unbound-reference diagnostics,
with rejection preserved. Outer not/quantifier/aggregate escapes were already
refused, not new removals. For a policy
allowing `none` but requiring payload 1 when present, use
`(o is some(v)) => v == 1`, equivalently
`o == none or (o is some(v) and v == 1)`; `and` alone instead rejects `none`.
The own-tree `<binary> check <path>` sweep from
`c32e7f5a88b412056210ee66f0cff9765001fed9` to measured
`b9751830e76ba64ad402c982387232a39ff87c9a`, repeated twice with complete
stdout/stderr/exit comparison, found zero removals/output changes in 597 corpus
files and 1,750 existing complete static Rust-test literal occurrences. Authored
146 routes have 57 removals; reviewed 110 inputs have 24, totaling 81, enumerated in
[the scope design](../docs/design/DESIGN-pattern-binding-scope.md#3-true-continuation-enforcement-1547).
All 18 message changes are recorded: 15 collision-priority cases with mechanical
collision-return controls (seven tested location changes), plus three def-call
capture changes. No acceptance expansions or repeat differences were observed
in this bounded sweep. Another 1,766 interpolated/incomplete document candidates
were inventoried, not executed; runtime-generated fixture graphs are not covered.
