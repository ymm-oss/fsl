Fixed (#1425): Built-in mutation candidates are retained as edits and materialized
one at a time, avoiding a full spec clone per candidate. Residual: each init
mutant retains a rewritten copy of its entire init block, and whole top-level
`if` statement mutants retain a rewritten copy of that entire statement;
their retained memory still scales with that block or statement's size times
its mutant count. Mutate stdout, catalog order, cap accounting, and verdicts
are unchanged. No migration is required.
