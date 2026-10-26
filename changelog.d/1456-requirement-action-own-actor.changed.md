Changed (#1456, breaking): v5.0.0 automatic mappings use requirement actions'
own actors, including standalone actions and generated branches on either side,
not enclosing requirement prose. Hidden own-actor mismatches now fail check,
unscoped verify, refine and Worker with the existing located type error (exit 2);
mapping-free diff keeps its semantic_diff/exit-0 envelope and reports affected
directions as unknown/automatic_mapping_failed. Matching or absent own actors
previously rejected by prose are intentionally accepted. Typed requirement text
does not declare an actor. Explicit action entries, inline branch maps and stutter
stay exempt; public metadata priority, Kernel v1/v2, RCIR, digests and report
attribution are unchanged. The repeated four-cell corpus check at base
`e8cf78f0162530e264128f588981d49790f2767f` / head
`b8daaa8245f374f74448fd983755689f832484cc` (591 base / 592 head forms per binary,
each own tree) found no corpus ok→error or error→ok transitions. Acceptance
removals/expansions are separate controls. Confirm policy before correcting an
action's actor; there is no general automatic migration, compatibility bypass or
grace period. If problems occur, return to a 4.x binary. See
`docs/design/DESIGN-refinement.md`, section
`#1456: requirement-action own actors and breaking-source migration`.
