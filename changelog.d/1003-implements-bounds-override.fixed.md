Propagate `fslc verify --instances` / `--values` overrides into inline `implements`
abstract specs, filtered to names the abstraction declares, so refinement keeps
running at the same world size on both sides.

Scoped runs now apply the same model-warning finalization as unscoped runs, so a
spec's `warnings` no longer depend on whether `--instances` / `--values` were
passed. Measured on the #1003 reproducer, this removes a `no_user_invariants`
warning that only scoped runs emitted.
