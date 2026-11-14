Unified (#1169): the weakenings `explain`/`html` try in their counterfactual
phases have one enumerator, `fsl_tools::enumerate_builtin_mutants`. The
reachable phase stops enumerating its own list (`weakening_candidates` and
`statement_removals` are removed) and, like the invariant phase, takes the
owner's `assignment_remove` / `requires_remove` / `fair_remove` mutants in
the owner's order, printed through one mapping to the public `op` and
`target`, and both phases read their document through one function. The two
lists were equal in order, targets and weakened models on every `specs/` and
`examples/` spec; the one difference, which mutants count as init
weakenings, stays an argument: the reachable phase marks a mutant of the
`init` block (`BuiltinMutant::edits_init`), the invariant phase every mutant
of something named `init`, so an action named `init` keeps its labels.
Command output and exit codes are unchanged.
