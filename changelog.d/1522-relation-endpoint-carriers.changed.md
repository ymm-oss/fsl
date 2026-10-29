Changed (#1522): Different-bound relation values in conditionals, Set/Seq items,
action/init/let assignments now fail check/verify/replay (located semantics, exit 2),
even narrow→wide, due to engine disagreement. Equal-bound aliases, scalar/Map
compatibility, nominal enums and self-relation identity are preserved. Numeric
`Set<A>` → `Set<B>` widening still checks; symbolic disagreement remains #1627.
Equal-bound relation literal engine support (#1524) stays with #1467. Corpus
removals: 0; inline acceptance removals: 10; one #1459 error changes message/location.
See `docs/design/DESIGN-relation-endpoint-compatibility.md`.
