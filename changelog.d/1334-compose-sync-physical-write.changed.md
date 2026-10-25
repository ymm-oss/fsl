Changed (#1334, breaking removal): sync additional statements assigning component
state through physical names such as `alias__x` now produce an explicit located
error. Those spellings previously passed `check`; alias-qualified writes were
already rejected. The corpus transition set is recorded by the orchestrator's
base/head sweep, not inferred from this fragment.
