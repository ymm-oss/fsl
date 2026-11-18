Changed (#1848, performance): `verify --engine bmc` decides the vacuity reachability probe on the walk of its concrete boundary pre-pass instead of walking the same states again while rendering.
Both walks start from the same initial state, step the same candidates in the same order, skip the same violating successors and stop at the same `CONCRETE_PROBE_BUDGET`, so the probe's verdicts are the ones a separate walk reports; when a `partial_op` or `type_bound` finding stops the pre-pass, the probe walks on its own as before.
Verdicts, warnings and witnesses are unchanged.
