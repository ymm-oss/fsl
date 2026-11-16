Documented (#1871): `fslc html --help`, both language references, the HTML report
design and the command reference now explain that verification follows
`--deadlock`, while witnesses always use `warn`: a detected `deadlock_terminal`
witness remains visible with `ignore`, and `error` still builds witnesses.
A CLI regression test pins this existing behavior; runtime behavior is unchanged.
