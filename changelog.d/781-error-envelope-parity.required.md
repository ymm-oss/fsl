Required (#781): native CLI error-envelope parity now derives every executed
failure-class cell from the command registry (including `ai compat` and
`causal verify-expectations`), validates location shape, and checks
self-retiring pins are executable.
