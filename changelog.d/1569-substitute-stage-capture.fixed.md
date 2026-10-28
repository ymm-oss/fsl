Fixed (#1569): A `stage(x)` inside a quantifier or aggregate whose binder is named
like an event's or property's implicit entity parameter (for example `c` for
`Claim`) now reads the binder rather than the outer instance. Previously this
could hide violations in transition guards and `set` values, or report spurious
violations in `always ... has` properties.
