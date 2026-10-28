Changed (Part of #1451): check and ordinary verify reject reads of an init
may-assigned root before full-root definite assignment with explicit's
`init references state variable '<name>' before it is assigned`, semantics/exit 2
and a statement location. This conservatively rejects dead RHS reads,
already-written components, and free component/path reads in
`struct-other-partial`, `if-one-branch-read`, and #1481 N2. Never-assigned roots
(issue-exact/free and choose, #493), permitted same-forall current-key reads
(#1520 X1/X2, only for unresolved binder values), and unread omitted/legacy
partial init remain accepted. Resolved partial-domain loops still require
whole-root coverage. Binder-keyed filtered/collection writes, including nested
ones, do not establish full coverage for a following read; rewrite with a
resolved full-domain binder before reading. Explicit's
free-value support gap remains in #1451. Rewrite by reading after complete
assignment or explicitly initializing the whole root; the complete 13-case
migration table plus N2 is in
[DESIGN-bridge](../docs/design/DESIGN-bridge.md#assigned-root-read-order-1451).
Historical evidence before the resolved-binder correction:
`fslc check <form>` in each own materialized tree,
base `9e8ed724bdbd070868af00d5160e5f6198738c49` versus head
`5bc59416fa42c3132e33e3e3264ed29c0d77701b`, all 597 `.fsl` files under `specs/`,
`examples/`, `rust/fslc/tests/fixtures/` unchanged in full output/exit (400 exit 0,
191 exit 2, 6 exit 1); corpus removal set empty. Separately, 37 inline sources
direct/composed (74 forms) have 28 `ok`→`error` transitions: 13 read-order cases
× 2 and N2 × 2, with no other output changes.
