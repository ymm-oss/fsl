Fixed (#1401): in the requirements groundwork, an event that names no
requirement with `covers` no longer gets a requirement made from its own name
and actor (`submit: by Employee`). Its action carries the requirement IDs the
document attaches to it: the rules that guard it, and the decision table and
the `covers` of the rows that name it. The kernel's `requirement` of the
action is the most specific of them (a row's `covers` before its table's ID),
or none when nothing names the event. `ledger`, `document generate`,
`explain` and `mutate --by-requirement` no longer show such a note as a
requirement, and the requirement a rule formalizes gets the same ledger status
as the same requirement written as a requirement block. Parsing, diagnostics,
and command output of published documents are unchanged.
