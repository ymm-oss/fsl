Breaking removal (#1680): `check` refuses a formal or input name an earlier formal of the same action took, at the repeated name, on every route: action, requirements action, process transition, event, synchronized root action, and every compose component action whether lifted, synchronized or internal and unreferenced. A component reports `component "piece.fsl" failed to check (part.put: duplicate action parameter 'a' at piece.fsl:1:81) at root.fsl:2:3`.

These forms passed `check` before and are now semantic errors (exit 2):

- `internal part.put` with `part.put(0, 1)`, in either order, for `put(a: K, a: K)`, `put(a in 0..1, a in 0..1)` or `put(a: K, b: K, a: K)`. The body read the last actual and dropped the first, so swapping the arguments flipped the verdict.
- `internal part.put` with no reference to it.

Migration: give each formal its own name and pass each actual to the formal it belongs to. There is no automatic migration.

Outside a component the message is unchanged and `loc` is now the repeated name, not the first token spelled like it. A lifted component action, which reported no location, now uses the component wrapper. `part.put(true, 0)` for such formals now reports the repeated formal instead of the `Bool` actual; `part.put(0)` keeps its arity error. `def` parameters were already refused on every route and are unchanged.

The 617-form on-disk corpus has an empty transition set: base `e3c19a39c9c3bf5c315c6002fd18114aa22cd654`, head `e973194e888d7a9f34686c96ae8da579b3de933b`, `fslc check` stable and FSL5 opt-in, and `fslc kernel` versions 1 and 2, complete output compared. The one inline change is the `duplicate_formal_with_erased_bool` row of `issue_1517_breaking_removals`, error to error.

Rust API: `SpecItem::Action`, `SyncAction` and `RequirementAction` require `param_spans`, and `ProcessTransition` requires `input_spans`.
