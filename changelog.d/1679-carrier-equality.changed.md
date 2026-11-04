Breaking removal (#1679): a model expression whose container carriers symbolic evaluation cannot coerce now fails `check` with the existing located assignability error (`semantics`, exit 2) instead of `cannot coerce symbolic value` on bmc and induction, wherever it is written, evaluated or not.

These forms passed `check` before and are now semantic errors:

- A Map whose value is `Int` or `Option<Int>` against one with a range, for `m0: Map<K, Int>`, `m1: Map<K, A>`: `m0 == m1`, `m0 != m1`, `m0 = m1`, `m0 == (if c then m1 else m0)`, `m0 == old(m1)`, `(if c then m0 else m1)[0]`; inside a literal (`some(m0) == some(m1)`, `Seq { m0 } == Seq { m1 }`, `Set { m0 } == Set { m1 }`) and between sibling elements (`Seq { m0, m1 }`, `Set { m0, m1 }`); as a method argument (`Seq { m0 }.contains(m1)`, `Set { m0 }.add(m1)`, `Set { m0 }.remove(m1)`, `Seq { m0 }.push(m1)`); and read back out of one (`Seq { m0 }.head() == m1`).
- A conditional arm that only an `Int` sibling related to the expectation: `a == (if c then t else b)` with `let t = Set { 0 }`, `a: Set<A>`, `b: Set<B>`; the same with a `Seq` literal, a declared `q: Seq<Int, 2>` or a differing capacity; `sa == old(if c then q else sb)`; and such an arm inside `some(...)`, `Seq { ... }`, `Set { ... }` or a method argument.

Verdicts taken away:

- A mismatch in a position that is never evaluated returned a verdict on bmc, explicit, induction and `fsl-bfs`, and is refused: `forall i: E { m0 == m1 }` and `exists` over `type E = 1..0`, an arm under such a binder, an action with a parameter over an empty range, and a property left out by `--exclude-property` or `--property`. There are 11 such forms in the audit grids.
- Other removed forms returned no verdict on bmc or induction. Explicit answered 82 of them, such as an unused `let u = m0 == m1`, and `fsl-bfs` answered 234.

Migration: declare the two values with one carrier, or compare the entries (`m0[k] == m1[k]`). There is no automatic migration.

Other forms keep their verdicts, including `a == t`, `q == a` with a differing range or capacity, `some(t) == some(a)`, `Option<Int>` against `Option<A>`, and Maps of equal carriers or equal-range aliases. A let-bound collection holding `Int` below an element, such as `let t = Set { some(0) }` against `Set { p }`, used to check and then report `cannot coerce` on bmc, explicit and induction; it now returns a result.

A refinement map and a scenario `expect` keep the prior rule. An `expect` is evaluated concretely, and so is a map on the refinement step, but `preserve progress` evaluates a map symbolically: `map same = m == y` for those Maps gives `cannot coerce`, exit 2, under `preserve progress`, and refines without it. A `--lemma` candidate with such a mismatch stays `rejected` with `cannot coerce`, `used` false and the main result unchanged, and one whose mismatch is never evaluated stays usable.

The 617-form on-disk corpus has an empty transition set: base `e3c19a39c9c3bf5c315c6002fd18114aa22cd654`, head `644278436fe0a94af236bd8aaa1207c58fedbe4f`, `fslc check` stable and FSL5 opt-in, and `fslc kernel` versions 1 and 2, complete output compared. The one inline change in a rewritten test is `f1` in `issue_1517_conditional_arm_admission`, ok to error.

Rust API: `fsl_core::CarrierCoercion` and `KernelModel::carrier_coercion` are new; `check` and `coerce` answer from them.
