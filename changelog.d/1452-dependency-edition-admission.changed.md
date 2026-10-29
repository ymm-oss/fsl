Changed (#1452): compose no longer accepts a child whose own edition rejects a
legacy colon quantifier. The former `ok`/exit 0 becomes the complete child
`unsupported_in_edition` refusal/exit 2 with finding message
`legacy colon quantifier syntax is deprecated`. Its child `loc.file` keeps the
reference spelling, so the migration refusal matches standalone checking with
that spelling; canonical filesystem identity is only the deduplication key. The braced canonical twin remains
accepted. This breaking consumer removal is recorded in the migration design and
both language references, not a grammar removal or stronger verification claim.
The materialized-tree check comparison (`bc14d885` → corrected Rust source/test tree `a65eddd5`)
covered 603 corpus forms in two parser configurations and 368 required-test
inline/included occurrences per tree (3148 checks). Its 6 `ok`→`error` occurrences
are only this compose-child form: the
`edition_dialect_controls::actual_dialects_and_valid_sidecars_are_preserved_twice`
context and `edition_dispatch_inventory` contexts
`dialect_controls::actual_dialects_and_valid_sidecars_are_preserved_twice` and
`deferred_modes_have_live_disagreements_across_the_complete_native_adapters`, each
twice. Other transitions: 0; corpus transitions: 0. Canonical twins stay accepted.
