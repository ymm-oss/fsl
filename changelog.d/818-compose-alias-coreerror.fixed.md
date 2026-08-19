Fixed (#818): a compose document with an undeclared component alias inside a
top-level `init` block's `forall` binder no longer panics past the public
API boundary. `rewrite_compose_statements` now returns `Result` and its
caller in `lower_compose` propagates with `?`, so `fsl_core::parse_kernel_source`
returns `Err(CoreError)` with the "unknown alias" message instead of the
process aborting. A rejecting control exercises the reproduction through
`parse_kernel_source`; an accepting control confirms a correctly declared
alias used in a `forall` binder still lowers successfully.
