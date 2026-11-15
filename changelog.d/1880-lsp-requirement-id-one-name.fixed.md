Fixed (#1880): the language server indexes a hyphenated requirement-style ID
(`policy POL-WAIT-001`, and likewise `goal`, `control`, `requirement`,
`acceptance`, `forbidden`, `rule`, `decide` and `scenario` IDs and their uses
in `satisfies`, `covers`, governance items and a refinement's `respond`) as
one name, exactly as the parser reads it, instead of as its first token.
Hover, definition, references and rename on any part of the ID act on the
whole ID, and IDs sharing a prefix (`POL-HOLD-002`, `POL-WAIT-001`) no longer
resolve to each other, also from `respond` in another file. Plain identifiers
are unchanged; an ID written across lines is still indexed token by token.
