Breaking removal (#1683): `check` and the symbolic engines now answer from one carrier rule, `fsl_core::CarrierCoercion`, whose answers are `Retag`, `RebuildInt`, `MapMismatch` and `NotAssignable`, and a literal under `old(...)` that can type itself is also typed against what surrounds it, in `check` and in evaluation.

Removed: a Map at any depth below `some`, Set and Seq literals and conditional arms under `old(...)`, for `m0: Map<K, Int>`, `m1: Map<K, A>`: `some(m0) == old(some(m1))`, `some(m0) != old(some(m1))`, `Set { m0 } == old(Set { m1 })`, `Seq { m0 } == old(Seq { m1 })`, `Set { m0 } == old(Set { if c then m1 else m1 })`, now fail `check` with the located assignability error (`semantics`, exit 2).

What goes with it: where such a form is evaluated, bmc and induction reported `cannot coerce`, but explicit and `fsl-bfs` returned a verdict, `violated` included, and that verdict is lost. Under a position that is never evaluated, such as `forall i: E { ... }` over `type E = 1..0`, bmc and induction lose theirs too, as for the forms #1679 refused. The refusal comes before property selection, so `--exclude-property` does not avoid it, and such a form hides another property's counterexample behind exit 2 until it is rewritten.

Unchanged: `a == old(Set {})`, `sa == old(Seq {})` and `oa == old(none)` are refused, though they pass outside `old(...)`.

Now returns a result: a range-typed literal under `old(...)`, the third route #1679 recorded. `a == old(Set { vb })`, `sa == old(Seq { vb })` and `sa == old(Seq { va })` for `vb: 0..2`, `a: Set<A>`, `sa: Seq<A, 2>` reported `cannot coerce` on bmc and induction and now agree with explicit. One past capacity, `sb != old(Seq { va, va, va })` for `sb: Seq<B, 2>`, can end in `model sequence length exceeds capacity` on induction instead, as the same form outside `old(...)` does. Two routes still report it: a Map comparison in a refinement map under `preserve progress`, and a rejected `--lemma` candidate.

Migration: declare the two values with one carrier, or compare the entries (`m0[k] == m1[k]`).

The 646-form on-disk corpus has an empty transition set, base `1559e8eba90f73615d4efd00fcdccc30cf525b42`, head `66a17a7f96278c4f5286261c5cf59ece43270618` before a rebase onto `156d9c8ee492ca86bc259bf03afbae96dc73ec33`, and bmc and induction return the same result on all 425 forms it accepts. The matrix and the positions it does not cover are in `docs/design/DESIGN-finite-container-carriers.md`.

Rust API: `CarrierCoercion` gains `Retag` and `NotAssignable`, and loses `Other`.
