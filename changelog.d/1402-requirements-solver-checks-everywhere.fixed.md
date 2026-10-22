Fixed (#1402): a requirements decision table or rule guard whose check the
solver decides (because its fields take more than 1000000 values together,
or because its condition reads a resource or a relation role) was checked
only by `fslc check`. `verify`, `kernel`, `testgen`, `document`,
`scenarios`, `ledger`, `explain`, `mutate`, `typestate` and the other
commands that load the spec accepted it and exited 0. They now reject it
with exit 2 and the diagnostic `check` reports (`decide_not_exhaustive`,
`decide_overlap` or `rule_conflict_unsatisfiable_guard`). The language
server now shows that error, and the browser Worker's `check` and `verify`
report it too.
