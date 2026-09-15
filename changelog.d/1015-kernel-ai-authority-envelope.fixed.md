Fixed (#1015): `fslc kernel` no longer panics on an `ai_component` whose
`authority` block names an undeclared tool; it now returns the same
`{"result":"error","kind":"semantics"}` envelope and exit 2 that `fslc check`
already returned for this input, instead of exiting 101 with no JSON. A
negative control in `error_envelope_parity.rs`'s `kernel` coverage pins this
envelope so the lookup panic cannot be reintroduced silently; `fslc check`'s
envelope for the same input is unchanged.
