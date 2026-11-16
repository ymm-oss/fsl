Changed (#1888, performance): native bounded verification asks one combined
unranked `leadsTo` lasso query per binding and skips enumeration only on
`unsat`; `sat` and `unknown` retain the existing ordered search. Solver-work
and timing statistics under `cost` change; verdicts and diagnostic evidence
retain their output-preservation contract. Deadlock-stagnation checks remain
per pending step.
