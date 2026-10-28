Changed (Part of #1451): check and ordinary verify reject reads of an init
may-assigned root before full-root definite assignment with explicit's
`init references state variable '<name>' before it is assigned`, semantics/exit 2
and a statement location. This conservatively rejects dead RHS reads,
already-written components, and free component/path reads in
`struct-other-partial`, `if-one-branch-read`, and #1481 N2. Never-assigned roots
(issue-exact/free and choose, #493), permitted same-forall current-key reads
(#1520 X1/X2), and unread omitted/legacy partial init remain accepted. Explicit's
free-value support gap remains in #1451. Rewrite by reading after complete
assignment or explicitly initializing the whole root; the complete 13-case
migration table plus N2 is in
[DESIGN-bridge](../docs/design/DESIGN-bridge.md#assigned-root-read-order-1451).
Breaking-removal evidence: `fslc check <form>` in each own materialized tree,
base `e64d3fb148ca94d179e755e541de9b2ecd7fade7` versus head
`eb6e2a46f94ed9826b02237ac7cb9ea0af4d5330`, all 597 `.fsl` files under `specs/`,
`examples/`, `rust/fslc/tests/fixtures/` unchanged in full output/exit (400 exit 0,
191 exit 2, 6 exit 1); corpus removal set empty. Separately, 37 inline sources
direct/composed (74 forms) have 28 `ok`→`error` transitions: 13 read-order cases
× 2 and N2 × 2, with no other output changes.
