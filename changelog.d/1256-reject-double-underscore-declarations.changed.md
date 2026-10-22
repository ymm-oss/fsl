Changed (#1256, breaking): a name declared in a `spec`, `requirements`,
`business`, or `compose` document may no longer contain `__`. `fslc check`
(and every command that loads the document) rejects one with `kind: "name"`,
exit 2, located at the declaration; a compose component's own declaration is
reported at the parent's `use`. This covers specs, actions, parameters, `let`
bindings, state variables, properties, types, consts, enum members, struct
fields, processes, stages, actors, and `use` aliases. `__` is reserved for the
names lowering generates (a compose component's physical `alias__name`, a
`branches` split `submit__b1`), so each has one public spelling, `alias.name`
and `submit.b1`. Before, `action foo__bar` passed check and was published as
`foo__bar` by the Public Kernel and `foo.bar` by conformance, and
`use X as a__b` published its state `n` as `a.b__n`. Referencing a generated
name, as a refinement mapping onto `submit__b1` does, is still accepted. No
spec or example in the repository declares such a name. Migration: rename a
declaration that contains `__` (for example `foo__bar` to `foo_bar`) and the
references to it, including replay traces and stored outputs that name it.
