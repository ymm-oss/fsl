Changed (#1450, breaking removal): `check` rejects calls whose caller action
parameters, lets, expression quantifier/aggregate binders or statement `forall`
would capture free names in the expanded def body, excluding the def's own
parameters. The error is exit 2, `kind: "semantics"`, and `would capture variable`
at the call. Rename the caller binding; no automatic rename or bypass is added.
Direct specs and source-local compose/component defs share the check. Existing
pattern-shadowing refusals and locations are preserved, including imported enum
members. Compose-source expansion now receives Common declarations for its
global-name inventory and runs after component loading, still before rewriting.

Exception / remaining soundness risk (Refs #1528): binders declared directly by
a `leadsTo` property are not passed to this capture check and can still produce
a false green by capturing a def-body free name. Rename that binder before
relying on verification; expression quantifiers/aggregates inside the property
are covered, but the declaration-level path is not fixed here.

The `check` sweep of all 597 repository forms in materialized trees for base
`9af1fa5f36f91e415b7ba249888f4ada4cf7a0db` and implementation
`d806e6bc6677016dac3803ac5559d3f0b1e3da67` has zero ok-to-error transitions.
All 20 corpus compose files have byte-identical check and Kernel v1/v2 outputs
and exits with per-tree repository roots; no output fields are excluded. The
new rejection is demonstrated separately by inline capture controls.
