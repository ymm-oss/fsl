(#726): `fslc domain analyze` now routes the parsed `DomainSpec` through the
same `validate_lowerable_constructs` guard `check`, `verify`, `compose`, the
wasm Worker, `domain check`, and `domain expand` already call, instead of
projecting the raw AST directly. A top-level `await`, an `on_stale` policy,
or a `value_object` invariant (#710/#711/#712) is now rejected by `analyze`
with the same diagnostic kind, location, and exit code as `check`, instead
of silently succeeding with an incomplete structural projection. A
well-formed domain spec still analyzes successfully.
