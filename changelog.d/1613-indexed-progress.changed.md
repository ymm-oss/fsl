Changed (#1613): `refine` progress pulls now instantiate indexed mappings
simultaneously in implementation scope, avoiding identity/swap stack overflow
and an indexed chain's false `refines`. **Breaking removal:** with per-element
maps, free abstract whole-map reads in pulled progress properties are now
`error/type` (exit 2) at `respond`, even when an unrelated implementation state
shares the name. The `whole-same` witness changes `refines`/0 to type/2;
already-rejected renamed forms gain location/classification, not new rejection.
Legal source binders, inserted implementation whole-map expressions, scalar
maps and no-progress simulation stay supported. Migration must preserve P/Q's
quantifiers and temporal meaning; moving `forall` across `~>` is not generally
equivalent. The named f8→2aab own-tree audit has an empty check-removal set over
610 corpus + 54 inline forms, separately one admitted→error refinement case;
all 710 check/refine routes ran twice per SHA. Full SHAs, commands, counts and
the separate crash/verdict corrections are recorded in DESIGN-refinement §4.
