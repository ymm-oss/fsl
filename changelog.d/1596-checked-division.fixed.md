Fixed (#1596): dividing the minimum i64 value by `-1` or taking its remainder
now reports integer overflow instead of panicking in runtime evaluation and
constant folding; finite-domain evaluation defers the overflow like other
checked arithmetic. Init errors retain the containing statement location, and
zero-division behavior is unchanged, including totalized property contexts.
