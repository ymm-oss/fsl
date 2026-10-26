Changed (#1441, breaking): v5.0.0 auto mappings compare declared actors on both
sides independently of `covers`, including fsl 4 process/business transitions,
cross-edition 4/5 pairs and typed/legacy compose pairs. Previously hidden
mismatches now return the existing located `maps auto actor mismatch` type
error (exit 2) in check/unscoped verify/refine and Worker; mapping-free diff
instead reports unknown/automatic_mapping_failed within semantic_diff/exit 0.
Explicit action correspondences, legacy raw metadata rules and public
Kernel/JSON schemas are unchanged. The complete four-cell `fslc check` sweep at
base `5a539bbe27dc933d7fe8df95e6189cb75cec6cc9` and head
`9ca8630c7f45f1def54c056bf53a852853c7a60c` (579 base / 591 head forms in their
own trees) measured no corpus ok→error or error→ok transitions. Separately,
matching actors with impl/business covers text `by Manager` (B3/B4) now check
ok/0 rather than error/type/2 and bounded verify returns verified/0 with
implements refines. There is no general automatic semantics-preserving
migration; see the [measured removals, expansions and migration contract](docs/design/DESIGN-refinement.md#migration-1441).
