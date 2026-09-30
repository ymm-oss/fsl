Fixed (#1166): `fslc explain` now lists a `partial_op` auto-check for every
partial operation `verify` checks. It previously missed `Seq` index reads
(`s[0]`), operations inside quantifier and aggregate binders, and operations in
an indexed assignment target, although `verify` reported `_partial_<action>`
violations for all of them. The six partial operations are now defined once in
`fsl-core` and shared by `explain`, the verifier, the runtime and the Public
Kernel; the Public Kernel's `partial_operations` now also includes an indexed
assignment target's index expression (`m[s.head()] = …`). The runtime classifies
a partial-operation failure by a typed field rather than by its error message.
Verification verdicts are unchanged.
