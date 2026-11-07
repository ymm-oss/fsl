Changed (#1440): the `issue_1573_pattern_reuse` and `issue_1480` envelope
tests and the WASM browser parity test take the expected `fslc-rust`,
`fsl-wasm` and `fsl-core` versions from the build's package version instead
of a literal `4.12.0`, so a release bump no longer fails them. Product output
is unchanged.
