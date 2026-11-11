Documented (#1809, refs #1801): the `fsl-design`, `fsl-requirements` and
`fsl-verify` skills and both language references now say that two `only after`
prerequisite sets of one entity that would get the same history field name
each get a numbered suffix (`order_did_a_or_b_1`, `order_did_a_or_b_2`), and
that the number depends on the entity's other history fields and declared
fields, not on rule order, so an edit elsewhere in the document can renumber
the field names shown in counterexamples. No behavior change.
