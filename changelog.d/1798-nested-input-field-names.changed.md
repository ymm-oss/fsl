Changed (#1798, breaking removal): in an FSL 5 event's `only when` condition
or `set` expression, a bare name that is both the event's input and a field of
its entity is `ambiguous_input_field` (exit 2) inside every expression form,
as LANGUAGE.md already states; inside `abs(...)`, `min`/`max`, a method call,
an `is` test, a quantifier or `count`/`sum` it used to read the input silently.
Write `input amount` or `Claim.amount`. Nested in those forms, both used to fail
with a `semantics` error and now work: `Claim.amount` reads the stored field
and `input amount` reads the input, except inside a quantifier or aggregate
binder named `amount`, where it is refused with the new code
`captured_by_binder` because the binder would capture it. A concept condition
whose quantifier range bound is a field (`exists x in amount..amount { ... }`)
is now refused with `binder bound is not a static integer`: it used to pass
only by reading a same-named input. A decision table's `assuming` now expands
a concept named inside a quantifier or aggregate, which used to fail with a
`semantics` error; a binder named like something that concept reads (a
resource, say) is `captured_by_binder`. A binder named like a concept still
hides the concept inside its scope, as before. Checking all 655 `.fsl` files
under `specs/`, `examples/` and `rust/fslc/tests/fixtures/` with binaries from
`995146f45` and from this change gives the same result, kind and exit code for
every file.
