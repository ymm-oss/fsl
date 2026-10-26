Fixed (#251): next-edition diagnostics for already-invalid expression `||` and
`->` keep the parse envelope and exit code while reporting
`unsupported_in_edition`, the operator location, canonical replacement, and
migration guidance. Structural operators and domain acceptance are unchanged.
