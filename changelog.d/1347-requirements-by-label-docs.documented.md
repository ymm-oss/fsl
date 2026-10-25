Documented (#1347): a requirements transition's `by X` is a free label.
The requirements layer has no `actor` declaration and `check` does not look
the name up, unlike `business`, which rejects a transition whose actor is not
declared. The label is compared with the business actor only when a
refinement maps the transition automatically to the same-named business
action: the inline `implements` seam for a transition no explicit inline
`action` item names (empty body or `maps auto`), and a standalone refinement
file under `maps auto` (`fslc refine`, a `fslc chain` refine layer). A
transition mapped by an explicit `action` item, or with no same-named
business action (mapped to `stutter`), is not compared.
`docs/manual/LANGUAGE.md` and `docs/manual/LANGUAGE.ja.md` §13.2,
`skills/fsl/references/layers.md`, and `skills/fsl-requirements/SKILL.md` say
so; behavior is unchanged.
