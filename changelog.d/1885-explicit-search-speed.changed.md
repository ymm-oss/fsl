Changed (#1885, refs #1851, #1247): `verify --engine explicit`, the explicit
half of `--engine auto`, the concrete boundary probe that runs before BMC, and
the vacuity reachability probe cost less per state; what they explore and
report is unchanged (stdout, stderr, and exit code byte-identical apart from
the wall-clock and memory `cost` fields on every `.fsl` under `specs/`,
`examples/`, and `rust/fslc/tests/fixtures/`). A stepped action's guard is no
longer evaluated a second time, a successor is no longer copied out of and
back into a scratch monitor for every candidate, and the visited states and
parent links are kept packed (one canonical byte string per state, the store
the refinement searches use since #1247) instead of as full state copies. On
`specs/order_system.fsl` `--engine auto` explores about 16,000 instead of
8,100 states per second, `examples/named_predicate.fsl` about 16,700 instead
of 12,000, and `specs/order_workflow.fsl` about 23,900 instead of 12,500; the
default `verify specs/order_system.fsl` reports `cost.elapsed_s` 1.7 s instead
of 2.6 s (medians of three release runs on a shared machine). No migration is
needed.
