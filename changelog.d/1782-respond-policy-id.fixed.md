Fixed (#1782, refs #1276): `preserve progress { respond <ID> by … }` can name a
business or `fsl 5` requirements policy's progress property by its policy ID as
declared (`respond POL-WAIT-001 by promote`), so a refinement carries an upper
`leadsTo` without the upper spec being rewritten. `respond` reads its name
with the same rule as the `policy` declaration and matches the abstract Kernel
`leads_to` name exactly; a flow that loses the progress is
`refinement_failed`/`progress_lost` (exit 1) and one that keeps it refines
(exit 0). A quoted name stays a parse error, accepted plain identifiers are
unchanged, and `fmt` prints the ID as written. An unknown `respond` name is
still `error/type` (exit 2) at `respond`; its message now lists the
abstraction's `leadsTo` names (`… declares leadsTo 'POL-WAIT-001'`, or
`declares no leadsTo`). A non-ASCII `respond` name in an `fsl 5` refinement is
still refused at the same location (exit 2, `kind: "parse"`), now as
`non_ascii_requirement_id`, as a non-ASCII policy ID is, instead of
`canonical_name_not_ascii`.
