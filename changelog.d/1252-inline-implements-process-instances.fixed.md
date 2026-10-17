Fixed (#1252): `fslc verify --instances` now reaches the abstract side of an
inline `implements` whose abstract is a `requirements` spec declaring only a
`process` (no `entity` of that name). The override was dropped for it, so a
shrunken impl was compared with the full-size abstract and failed with a
false `refinement_failed` / `map_out_of_bounds` (exit 1). No migration is
needed: runs that failed this way now report the seam's real verdict.
