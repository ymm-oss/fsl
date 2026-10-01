Fixed (#1192): `check` accepted a spec that declared two properties with the
same name, and every result keyed by that name collapsed them — with two
`leadsTo L`, `verify --engine induction` reported `proved`/`unbounded`
although the second `L` is false. Property names (`invariant`, `trans`,
`unless`, `reachable`, `leadsTo`, `until`, and the `<name>_until_safety` trans
an `until` lowers to) now share one namespace; a reuse, within a kind or
across kinds, is a located `kind: "semantics"` error at the later declaration
that names the earlier one's location, so `check` and `verify` exit 2. No spec
under `specs/`, `examples/`, or the test fixtures reused a property name. The
frozen Python reference still accepts such specs.
Still accepted: a user invariant named `_bounds_<var>` shares its name with
the generated type-bound invariant for `<var>`. Both are still checked
separately (a false one is still reported `violated`), but the name appears
twice in `invariants_checked`.
