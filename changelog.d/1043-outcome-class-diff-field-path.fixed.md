Fixed (#1043): `outcome_class`'s `semantic_diff` arm now reads the
`gate.passed` field `run_diff` actually publishes, instead of a top-level
`violations` array `run_diff` never writes (it only appears nested under
`gate`). This is a latent-defect fix in the shared classifier: no in-tree
caller currently folds a `semantic_diff` envelope through `outcome_class`,
and `fslc diff`'s own process exit already computed its status independently
and correctly. There is no observable CLI behavior change.
