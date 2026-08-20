Fixed (#846): the Rust workspace is clean again under `clippy` 1.98.0, whose
new deny-by-default `manual_is_variant_and` and `chunks_exact_to_as_chunks`
lints turned `main` red with no repository change, because every CI job uses
the unpinned `dtolnay/rust-toolchain@stable`. Both rewrites are
semantics-preserving. CI named only two sites, since cargo aborts at the first
failing crate; enumerating with `--keep-going` found four, and the fix was
also checked against the pinned 1.88.0 MSRV that `release.yml` uses so
`as_chunks` cannot break the release build after PR CI passes on stable.
