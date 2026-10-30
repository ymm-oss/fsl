Changed (#1522): Different-bound relation values in conditionals, Set/Seq items,
action/init/let assignments now fail check/verify/replay (located semantics, exit 2),
even narrow→wide, due to engine disagreement. At the #1522 measurement,
equal-bound aliases, scalar/Map compatibility, nominal enums and self-relation
identity were preserved, and numeric `Set<A>` → `Set<B>` widening still checked
with symbolic disagreement tracked as #1627. Subsequently #1472 requires equal
whole-container carriers for Set/Seq/Map (including Map keys and values), rejects
that Set widening statically, and resolves #1627. Scalar compatibility remains.
Equal-bound relation literal engine support (#1524) stays with #1467. Corpus
removals: 0; inline acceptance removals: 10; one #1459 error changes message/location.
See `docs/design/DESIGN-relation-endpoint-compatibility.md`.
