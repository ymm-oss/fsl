Fixed (#1134): the business dialect now checks its control catalog instead of
discarding it. `BusinessItem::Control` fell into the catch-all arm of
`lower_business`'s item loop, so a `control` declaration --- and every
`satisfies` naming one --- was parsed, accepted and dropped, and `check`
answered `ok` for a policy pointing at a control the document never declared.
An unknown reference is now a located error at the policy or goal that wrote
it, and a control no policy or goal satisfies raises the `unused_control`
warning `docs/DESIGN-dialects.md` already promised, located at the
declaration. Both were already implemented in the Python front end
(`src/fslc/dialects.py`), so this closes a port gap rather than narrowing the
language; a control confirms a declared-and-satisfied catalog still checks
clean, and `check` exit codes are unchanged for all 537 tracked `.fsl` files.
