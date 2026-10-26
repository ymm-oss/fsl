Changed (#1441, breaking): v5.0.0 auto mappings compare source actors on both
sides (the `by X` label for an edition 4 process transition) independently of
`covers`, including fsl 4 process/business transitions,
cross-edition 4/5 pairs and typed/legacy compose pairs. Previously hidden
mismatches now return the existing located `maps auto actor mismatch` type
error (exit 2) in check/unscoped verify/refine and Worker; mapping-free diff
instead reports unknown/automatic_mapping_failed within semantic_diff/exit 0.
Explicit action correspondences, metadata rules for unregistered actions
(raw spec/compose actions and requirements action items), and public
Kernel/JSON schemas are unchanged. The complete four-cell `fslc check` sweep at
base `5a539bbe27dc933d7fe8df95e6189cb75cec6cc9` and head
`9ca8630c7f45f1def54c056bf53a852853c7a60c` (579 base / 591 head forms in their
own trees) measured no corpus ok→error or error→ok transitions. Separately,
the expansion class is registered transition metadata whose requirement text
begins with `by ` (covers text and next-edition rule/decide text). Matching actors with
impl/business covers text `by Manager` (B3/B4) or abstract event rule text
`by Manager only` without covers (F1/p5) now check ok/0 rather than error/type/2;
bounded verify returns verified/0 with implements refines and F1 standalone
refine returns refines/0 instead of error/type/2. There is no general automatic
semantics-preserving migration; see the measured removals, expansions and migration contract in `docs/design/DESIGN-refinement.md`, section "#1441: source-actor auto mapping and breaking-source migration".
