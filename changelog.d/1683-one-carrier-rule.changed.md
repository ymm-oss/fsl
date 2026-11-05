Breaking removal (#1683): `check` and the symbolic engines now answer from one carrier rule, `fsl_core::CarrierCoercion`, whose answers are `Retag`, `RebuildInt`, `MapMismatch` and `NotAssignable`, and a literal under `old(...)` is typed against what surrounds it in `check` and in evaluation, as it is outside `old(...)`.

Removed: a Map a literal holds under `old(...)`, for `m0: Map<K, Int>`, `m1: Map<K, A>`: `some(m0) == old(some(m1))`, `some(m0) != old(some(m1))`, `Set { m0 } == old(Set { m1 })`, `Seq { m0 } == old(Seq { m1 })`, now fail `check` with the located assignability error (`semantics`, exit 2). Where such a form is evaluated it reported `cannot coerce`, so no verdict goes with it. Under a position that is never evaluated, such as `forall i: E { ... }` over `type E = 1..0`, it returned a verdict on bmc and induction, and that verdict is lost, as for the forms #1679 refused.

Now returns a result: a range-typed literal under `old(...)`, the third route #1679 recorded. `a == old(Set { vb })`, `sa == old(Seq { vb })` and `sa == old(Seq { va })` for `vb: 0..2`, `a: Set<A>`, `sa: Seq<A, 2>` reported `cannot coerce` on bmc and induction and now agree with explicit. Two routes still report it: a Map comparison in a refinement map under `preserve progress`, and a rejected `--lemma` candidate.

Migration: declare the two values with one carrier, or compare the entries (`m0[k] == m1[k]`).

The 646-form on-disk corpus has an empty transition set, base `1559e8eba90f73615d4efd00fcdccc30cf525b42`, head `ab1cacbc6fbd19363a4c1203a13fcddb306a4d82`, and bmc and induction return the same result on all 425 forms it accepts. The matrix and the positions it does not cover are in `docs/design/DESIGN-finite-container-carriers.md`.

Rust API: `CarrierCoercion` gains `Retag` and `NotAssignable`, and loses `Other`.
