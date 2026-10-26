Fixed (#1469): `html --no-timing` omits environment-dependent solver statistics
from the presentation copy on stdout and in `-o`/`--output` files, preventing
ambient Z3 memory differences from breaking byte-stable report comparisons.
The exact omitted fields are `verification.cost.elapsed_s`,
`verification.cost.solver.check_elapsed_s`,
`verification.cost.properties[].elapsed_s`,
`explained.counterfactuals[].violation.cost.elapsed_s`,
`verification.cost.solver.memory_mb`, `verification.cost.solver.conflicts`,
`verification.cost.solver.decisions`, and `verification.cost.solver.propagations`.
Solver check counts and property identities/counts remain; default HTML,
ordinary verify JSON, exit codes, and approval normalization are unchanged.
One CI failure showed `memory_mb` differing (17.91 versus 17.92); 396 local
macOS runs showed no solver-statistic variation, including memory. The three
search counters are classified as ambient based on their Z3-internal source
and the existing approval normalization of the same four solver statistics,
not on measured local drift.
