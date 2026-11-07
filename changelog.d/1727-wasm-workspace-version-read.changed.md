Changed (#1727): the WASM browser parity test reads the `[workspace.package]`
version from `rust/Cargo.toml` table by table, so an array value in that table
no longer hides it. Product output is unchanged.
