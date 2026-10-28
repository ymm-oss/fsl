Fixed (#1425): Built-in mutation candidates are retained as edits and materialized
one at a time, so memory no longer scales with the number of mutants times the
full spec size. Mutate stdout, catalog order, cap accounting, and verdicts are
unchanged. No migration is required.
