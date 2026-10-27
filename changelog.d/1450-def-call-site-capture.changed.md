Changed (#1450, breaking removal): `check` rejects calls whose caller action
parameters, lets, expression quantifier/aggregate binders or statement `forall`
would capture free names in the expanded def body, excluding the def's own
parameters. The error is exit 2, `kind: "semantics"`, and `would capture variable`
at the call. Rename the caller binding; no automatic rename or bypass is added.
Non-global `is some(v)` bindings exported by `requires` and by a binary left
operand also count as caller bindings and reject these previously accepted calls.
Direct specs and source-local compose/component defs share the check. Pattern
collision rules remain in force, including imported enum members; the explicit
diagnostic-order exceptions below still reject with the same exit and kind.
Compose-source expansion now receives Common declarations for its
global-name inventory and runs after component loading, still before rewriting.

Exception / remaining soundness risk (Refs #1528): binders declared directly by
a `leadsTo` property are not passed to this capture check in before/after,
`helpful` arguments, `decreases`, or `within`, and can still produce
a false green by capturing a def-body free name. Rename that binder before
relying on verification; expression quantifiers/aggregates inside the property
are covered, but the declaration-level path is not fixed here.
The declaration binder's own `where` expression is checked for capture; this can
precede the existing rejection of unsupported wrapper `where` filters. First
error ordering can also change when both a component load and a compose def are
invalid, or when both an action body and its parameter range contain invalid calls.
In particular, **v01** (invalid parameter range and action body) remains
exit 2/kind `name`, but can report the body error before the parameter error.
**a01** (a pattern-bound argument colliding with a def-body binder) remains
exit 2/kind `semantics`, but can report pattern shadowing instead of the earlier
`would capture` message at a different first location. Restoring those two
legacy diagnostics is deferred; neither change introduces a successful verdict.

The final `check` sweep of all 597 repository forms, in the respective
materialized trees for base `11f01225e244dabde0f8dfd9aaa665bc47dccfd4` and built
implementation `dd7b47885680076412ab2f0b46ebe8854ff6758b`, has zero ok-to-error
transitions. Check/analyze outputs agree for all 20 compose corpus files and two
refactor controls; BMC at depth 4 agrees outside observed elapsed-duration values,
calibrated with two same-base repeats. No semantic field is excluded. Inline
capture controls separately demonstrate the breaking removal.
