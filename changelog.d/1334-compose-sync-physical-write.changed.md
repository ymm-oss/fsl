Changed (#1334, breaking removal): sync additional statements assigning component
state through physical names such as `alias__x` now produce an explicit located
error. Those spellings previously passed `check`; alias-qualified writes were
already rejected. This removal affects only physical-name component-state
assignments in sync additions. Before/after binaries, each in its own
materialized tree, checked all 579 `.fsl` files under `specs/`,
`examples/`, and `rust/fslc/tests/fixtures/`: recorded exit codes, results, and
the first 160 characters of each message agree, with zero ok-to-error transitions
(this write form is absent from that corpus). No pre-existing required tests were
rewritten, renamed, deleted, or stopped; only new regression tests were added.
