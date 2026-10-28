Fixed (#1569): A `stage(x)` inside a quantifier or aggregate whose binder is named
like the implicit entity parameter of a legacy transition, an fsl 5 event, or a
property now reads the binder rather than the outer instance. Legacy transitions
always use `c`; fsl 5 events and properties use, for example, `c` for `Claim`.
Previously this could hide violations in transition guards and `set` values, or
report spurious violations in `always ... has` properties. A process field named
`c` still leaves `stage(c)` referring to the transition's own entity instance.
