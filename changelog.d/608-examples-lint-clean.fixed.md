Fixed (#608): 25 more shipped examples now pass `fslc lint`, including the
teaching examples `examples/pm/cancel_system.fsl`, `examples/pm/cancel_flow.fsl`,
`examples/layers/return_policy.fsl` and `examples/layers/return_system.fsl`.
Their requirement, acceptance, forbidden, policy, goal, control and assumption
IDs use the canonical `PREFIX-SCOPE-NNN` form: `REQ-1` becomes
`REQ-CANCEL-001`, `goal CanRetain` becomes `goal GOAL-CANCEL-001`, and
`FB-EARLY-REFUSE` becomes `FB-EARLY-REFUSE-001`. Legacy `"ID: text"` tags
become `@requirement(...)`, and domain invariants use `=>` instead of `->`.
Each changed file keeps its `check` result, its `verify --depth 8` verdict and
property results, and its `mutate` kill rate. A new corpus test,
`every_corpus_spec_lints_clean_or_is_allowlisted`, fails when a `specs/` or
`examples/` file adds a lint finding. Each file it still allows names its
reason, for example a file the frozen Python reference loads, or business
policy IDs that `preserve progress` must reference as identifiers.
