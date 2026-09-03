Changed (#798, slice 2): retired the #796 CLI post-validation on `domain
analyze` and `domain expand` now that scope-aware AST normalization makes the
renderer agree with direct lowering; rejecting controls in
`issue_796_domain_command_validation` still pass without the guard.
