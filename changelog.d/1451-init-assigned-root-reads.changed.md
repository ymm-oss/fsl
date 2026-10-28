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
assignment or explicitly initializing the whole root; the 19 measured-inline
migration examples (original 13, N2, two subsets and three post-loop cases) are in
[DESIGN-bridge](../docs/design/DESIGN-bridge.md#assigned-root-read-order-1451).
Breaking-removal evidence: `fslc check <form>` in each own materialized tree,
base `c79016efeb630c717292e9392901cfe65ab40915` versus head
`2d05dd63f05ca68c4bd7d06c3b24ba147ce6eda4`, all 597 `.fsl` files under `specs/`,
`examples/`, `rust/fslc/tests/fixtures/` unchanged in full output/exit (400 exit 0,
191 exit 2, 6 exit 1); corpus removal set empty. Separately, 42 inline sources
direct/composed (84 forms) have 38 `ok`→`error` transitions: original 13 cases
× 2, N2 × 2, two static-subset cases × 2 and three post-loop coverage cases × 2,
with no other output changes.
