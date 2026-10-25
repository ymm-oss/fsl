Documented (#1347): a requirements transition's `by X` is a free label.
The requirements layer has no `actor` declaration and `check` does not look
the name up, unlike `business`, which rejects a transition whose actor is not
declared. The label is compared with the business actor only when a
refinement auto-maps the transition to the same-named business action (via
`maps auto`, or via the inline `implements` seam's implicit auto-mapping of a
transition no explicit inline `action` item names) and that refinement is
evaluated; a `verify` scoped with `--property` does not evaluate the inline
seam, and `fslc diff` without a mapping builds its own `maps auto`
refinement. A transition mapped by an explicit `action` item, or one the
inline seam maps to `stutter` (no same-named business action), is not
compared. `docs/manual/LANGUAGE.md` and `docs/manual/LANGUAGE.ja.md` §13.2,
`skills/fsl/references/layers.md`, and `skills/fsl-requirements/SKILL.md` say
so; behavior is unchanged.
