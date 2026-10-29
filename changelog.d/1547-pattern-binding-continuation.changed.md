Changed (#1547, breaking removal): native `check` rejects pattern variables
outside their true continuation, including disjunctive/comparison/implication
escapes and non-conjunctive `requires` exports. Ten logical scope consumers and
the DDD frontend share one operator policy. Collision reservations remain
separate and base-equivalent, including compose guard-name inventories so
accepted output retains authored names; #1573 remains separate work. Legal
`and`/`=>` RHS bindings and conjunctive guard exports remain supported. Earlier newly invalid
references can precede legacy collision diagnostics, still exit 2/semantics.
Expression-if arm inference can report unbound then/else/nested references at
`6:32`/`6:39`/`6:48` before the former condition collision `6:12`; renaming only
the arm reference restores the complete base packet `6:12`. When only an
out-of-continuation pattern would capture a free name in a def body, the
predicate-capture error becomes a located unbound-reference diagnostic. This
class preserves exit 2/kind semantics while changing message/possibly location;
it is not limited to the three examples measured below. Outer
not/quantifier/aggregate escapes were already refused, not new removals. For a policy
allowing `none` but requiring payload 1 when present, use
`(o is some(v)) => v == 1`, equivalently
`o == none or (o is some(v) and v == 1)`; `and` alone instead rejects `none`.
The own-tree `<binary> check <path>` sweep from
`e0b4cbae39569de8008a8eb049588aa84d8c1a42` to measured
`3ae20a93296d36089f7860957a0a0b601f96932a`, repeated twice with complete
stdout/stderr/exit comparison and no excluded fields, found zero removals/output
changes in 601 corpus inputs (598 baseline plus three new fixtures supplied to
both trees) and 1,777 existing complete static Rust-test literal occurrences.
Authored 147 routes have 57 removals; reviewed 110 inputs have 24, totaling 81, enumerated in
[the scope design](../docs/design/DESIGN-pattern-binding-scope.md#3-true-continuation-enforcement-1547).
The historical 18 message-change packets are unchanged: 15 collision-priority
cases with mechanical collision-return controls (seven tested location changes),
plus three def-call capture examples. A nineteenth occurrence is the authored
copy of the already-reviewed d06 source, with identical before/after packets,
not another diagnostic class. No acceptance expansions or repeat differences
were observed in this bounded sweep. Another 1,783 interpolated/incomplete
document candidates were inventoried, not executed; runtime-generated fixture
graphs are not covered.
