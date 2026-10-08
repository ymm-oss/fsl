Unified (#1201): the verifier's per-action partial-operation candidate check
(`action_has_partial_operation_candidate`) moved unchanged into `fsl-core`, next
to the partial-operation inventory, so the verifier and the obligation catalog
decide whether an action can reach a partial operation with one function.
Verification output is unchanged.
