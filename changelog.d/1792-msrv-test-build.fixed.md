Fixed (#1792): the workspace's own tests compile again on Rust 1.88, the
declared minimum `rust-version`. A test helper used an `if let` match guard,
which 1.88 rejects, so the release workflow's MSRV test step failed before it
built any native binary. Test-only; the shipped binaries are unchanged.
