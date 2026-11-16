Unified (#1875): native scoped, replay, and resolver-backed loaders share their
parse/lower, build, and requirements-gate composition with explicit scope and
replay policies. `check --strict-tags` reuses its checked model while preserving
the existing non-Kernel dispatch, diagnostics, and output contracts.
