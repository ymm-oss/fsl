Breaking removal (#1679): a model expression whose container carriers symbolic evaluation cannot coerce now fails `check` with the existing located assignability error (`semantics`, exit 2) instead of `cannot coerce symbolic value` on bmc and induction.

These forms passed `check` before and are now semantic errors:

- A Map whose value is `Int` or `Option<Int>` against one with a range: `m0 == m1`, `m0 != m1`, `m0 = m1`, `m0 == (if c then m1 else m0)` for `m0: Map<K, Int>`, `m1: Map<K, A>`, also inside a literal (`some(m0) == some(m1)`, `Seq { m0 } == Seq { m1 }`, `Set { m0 } == Set { m1 }`), as a method argument (`Seq { m0 }.contains(m1)`) and read back out of one (`Seq { m0 }.head() == m1`).
- A conditional arm that only an `Int` sibling related to the expectation: `a == (if c then t else b)` with `let t = Set { 0 }`, `a: Set<A>`, `b: Set<B>`; the same with a `Seq` literal or a declared `q: Seq<Int, 2>`, or a differing capacity; and such an arm inside `some(...)`, `Seq { ... }`, `Set { ... }` or a method argument.

Migration: declare the two values with one carrier, or compare the entries (`m0[k] == m1[k]`). There is no automatic migration.

Forms that return a verdict on every engine are unchanged, including `a == t`, `q == a` with a differing range or capacity, `some(t) == some(a)`, `Option<Int>` against `Option<A>`, and Maps of equal carriers or equal-range aliases. So are a refinement map and a scenario `expect`, which are evaluated concretely. Explicit gave a verdict on 67 of the 198 removed forms in the audit matrix, and `fsl-bfs` answered on all 198; bmc and induction gave none.

The 617-form on-disk corpus has an empty transition set: base `e3c19a39c9c3bf5c315c6002fd18114aa22cd654`, head `0622a2a18362255845bb143fcd47232dacdf20e1`, `fslc check` stable and FSL5 opt-in, and `fslc kernel` versions 1 and 2, complete output compared. The one inline change is `f1` in `issue_1517_conditional_arm_admission`, ok to error.
