Distinguish proven duplicate writes from conservative `forall` index-distinctness
rejections in native `check`/`verify`, the browser Worker, and LSP diagnostics.
Unproved injectivity now reports `FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED` with
`loc` and a safe repair `hint` when one exists; acceptance is unchanged.

Known limit: an affine write inside an `if` within the `forall` is still reported with the previous duplicate-write message, because `assignment_for_target` only scans top-level assignments. That shape is unchanged from before this fix, not a regression.

The quick-fix is withheld rather than guessed: the RHS scan is built on the shared `expr_children`/`binder_exprs` walk instead of a second hand-written match, a binder whose domain does not start at zero contributes its own lower bound, a filtered binder is refused outright, and an RHS that already names `k` blocks the rewrite. The classification, code and location still arrive in every one of those cases.
