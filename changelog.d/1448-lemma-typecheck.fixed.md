Fixed (#1448): induction `--lemma` candidates now receive the same recursive
Boolean type check as invariants, for all candidates before any candidate proof
or evaluation. Ill-typed expressions are rejected and unused with a nested
semantics error and checker message, never proved or recommended. Later valid
candidates still run, and the target result owns the exit code. Lemma runs
continue to bypass the persistent verification cache.
