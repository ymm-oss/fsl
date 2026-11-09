Changed (#1798, breaking removal): in an FSL 5 event's `only when` condition
or `set` expression, a bare name that is both the event's input and a field of
its entity is `ambiguous_input_field` (exit 2) inside every expression form,
as LANGUAGE.md already states; inside `abs(...)`, `min`/`max`, a method call,
an `is` test, a quantifier or `count`/`sum` it used to read the input silently.
Write `input amount` or `Claim.amount`; `Claim.amount` there, which used to fail
with a `semantics` error, now reads the field. A concept condition whose
quantifier range bound is a field (`exists x in amount..amount { ... }`) is now
refused with `binder bound is not a static integer`: it used to pass only by
reading a same-named input. Checking all 655 `.fsl` files under `specs/`,
`examples/` and `rust/fslc/tests/fixtures/` with binaries from `995146f45` and
from this change gives the same result, kind and exit code for every file.
