Changed (#1680, breaking removal): `check` refuses a formal or input name an earlier formal of the same action took, on every route that loads the document: action, requirements action, process transition, event, domain command input, event field an effect or saga action takes as a formal, synchronized root action, inline `implements` abstraction action, and every compose component action whether lifted, synchronized or internal and unreferenced. A component reports `component "piece.fsl" failed to check (part.put: duplicate action parameter 'a' at piece.fsl:1:81) at root.fsl:2:3`, and an inline `implements` reports `implements "abs.fsl" failed to check (step: duplicate action parameter 'x' at abs.fsl:1:77)` with `kind: "type"` and `loc` on the `implements`.

These forms passed `check` before and are now semantic errors (exit 2):

- `internal part.put` with `part.put(0, 1)`, in either order, for `put(a: K, a: K)`, `put(a in 0..1, a in 0..1)` or `put(a: K, b: K, a: K)`. The body read the last actual and dropped the first, so swapping the arguments flipped the verdict.
- `internal part.put` with no reference to it.

On those component forms the other routes change too: `verify` no longer returns a verdict, `refine` no longer returns `refines`, `replay` no longer returns `conformant` or `nonconformant`, and `lint`, `fmt`, `conformance`, `explain` and `analyze` exit 2 instead of their own results. `domain analyze` and `domain expand` build no model and still exit 0 on a repeated command input.

Migration: give each formal its own name and pass each actual to the formal it belongs to. There is no automatic migration.

Elsewhere the message is `duplicate action parameter 'a'` and `loc` is the repeated name, not the first token spelled like it; a domain command or an event no longer adds its declaration's position to the message. A log mapping's `action ev(a, a) -> put(a)` and a domain event field no action takes stay accepted, because no actual is lost. A lifted component action, which reported no location, now uses the component wrapper. `part.put(true, 0)` for such formals now reports the repeated formal instead of the `Bool` actual; `part.put(0)` keeps its arity error. `def` parameters were already refused on every route and are unchanged.

The 617-form on-disk corpus has an empty transition set: base `e3c19a39c9c3bf5c315c6002fd18114aa22cd654`, head `a7e54a21b390dec8df9cfbfdf6a5f113186b454f`, `fslc check` stable and FSL5 opt-in, and `fslc kernel` versions 1 and 2, complete output compared. The one inline change is the `duplicate_formal_with_erased_bool` row of `issue_1517_breaking_removals`, error to error. The new inputs of `issue_1680_duplicate_formals` change only where they repeat a formal.

Rust API: `SpecItem::Action`, `SyncAction` and `RequirementAction` require `param_spans`, and `ProcessTransition` requires `input_spans`.
