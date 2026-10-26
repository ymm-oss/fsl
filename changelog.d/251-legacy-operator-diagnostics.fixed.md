Fixed (#251): next-edition diagnostics for already-invalid expression `||` and
`->` keep the parse envelope and exit code while reporting
`unsupported_in_edition`, the operator location, canonical replacement, and
migration guidance. Structural operators and domain acceptance are unchanged.
Worker check/verify now finalize successful and error envelopes with native
edition policy, including declared-edition fields and current enum-union warnings,
while check preserves early project-gate results instead of folding model-only
governance over them.
Type-range endpoints, including parenthesized parts, retain generic parse diagnostics.
