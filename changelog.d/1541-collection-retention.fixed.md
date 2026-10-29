Fixed (#1541): Retain computed out-of-domain members added to bounded Sets and
relations through symbolic consumers, state transitions and counterexample
projection, exposing bound violations when storing the add result back instead
of false unbounded proofs.
Relation bounds now appear consistently in selection and proof/report inventories;
Public Kernel v1/v2 aggregate export retains candidates through collection let
aliases and shadowed bindings without changing JSON shapes. The existing
`--exclude-property` limitation for implicit bounds is unchanged.
Whole-collection assignment from a wider carrier remains unsupported by symbolic
BMC/induction (exit 2), as on base.
Relational init copies share complete introduced support, including copy chains,
conditions and relation pairs. Unchecked Set/relation roots compact only after
proving finite key support; optional closed-grid ranking preserves lasso search
when a Set/relation root bound is omitted. Other unselected scalar bounds are not
assumed by ranking. Static check and concrete init read-order policy are unchanged.
