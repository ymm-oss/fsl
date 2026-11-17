Unified (#1874): bounded model checking asks an action instance's guard and
body definedness (guard partial, guard non-partial, body partial, body
non-partial) in one function in `fsl-verifier`, `probe_instance_definedness`.
The search's typed check and the definedness pass both call it and pass their
own filter, which stay different: the search skips an instance with no typed
partial-operation candidate and otherwise asks all four probes; the pass asks
a partial probe only when the evaluation has a partial operation, and the
non-partial probes, with the range lemmas, only after its disjunctive probe
answered `sat`. The solver sees the same queries in the same order; command
output and exit codes are unchanged. A test-only recording solver pins the
query sequence of eight specs with partial operations in guards, bodies and
`ensures`. The search's SMT term owners retain their original lifetimes;
a full-evidence regression detects witness drift from early term destruction.
