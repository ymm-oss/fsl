Changed (#1440): the `issue_1573_pattern_reuse` and `issue_1480` envelope
tests take the expected `fslc-rust` and `fsl-core` versions from the build's
package version, and the WASM browser parity test reads `fsl-wasm` and
`fsl-core` from the `[workspace.package]` version in `rust/Cargo.toml`, instead
of a literal `4.12.0`, so a release bump no longer fails them. Product output
is unchanged.
