Fixed (#993): `fslc document claims` no longer rejects a `verify { values N =
lo..hi }` bound written with compile-time consts. The projector now reads the
evaluated `KernelModel::types` domain bounds the model already built, so RCIR
v1's `analysis_scope.values[].lo/hi` carry plain integers and the renderer
shows `-1` to `2`. This aligns the projector with `docs/LANGUAGE.md`, which has
always written the bound as `values <Number> = <lo>..<hi>` without restricting
it to integer literals — it does not widen a documented contract. RCIR v1's
schema is unchanged, and existing literal bounds project byte-identically. A
`values` bound naming a number the spec never declares is not rejected either,
so no `values` bound makes `document claims` refuse a `requirements` spec that
`fslc check` accepts; a dialect RCIR v1 has no adapter for is still refused, as
a scope boundary. Rejection of a
non-constant bound is unchanged and now carries the model's own message
(`unknown constant 'LO'` rather than `must be an integer literal`); a
preservation control pins that, and separate detectors cover the evaluated,
literal-fallback, and omitted cases. `fslc diff` still drops such a bound
silently — that surface is tracked in #997, not fixed here.
