Documented (#1347): a requirements transition's `by X` is a free label.
The requirements layer has no `actor` declaration and `check` does not look
the name up, unlike `business`, which rejects a transition whose actor is not
declared. The label is compared with the business actor only through
`implements`, for a transition mapped to the same-named business action
automatically (one no explicit inline `action` item names; an empty
`implements { }` body and `maps auto` map this way). `docs/manual/LANGUAGE.md`
and `docs/manual/LANGUAGE.ja.md` §13.2 and `skills/fsl/references/layers.md`
say so; behavior is unchanged.
