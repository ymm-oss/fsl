Symbolic verification now evaluates scalar Set and Seq collection binders in
expressions and statements using shared membership-aware expansion. Seq binders
preserve their live prefix, position order and duplicates; Set binders preserve
distinct members in concrete value order, including dynamically valued literals.
Symbolic and explicit verification share the existing rejection of state-dependent
collection domains in init statement forall; check acceptance is unchanged.
