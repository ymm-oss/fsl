Unified (#1872): the `rust/fslc` integration tests spawn the native `fslc`
binary through one helper, `rust/fslc/tests/support/fslc_cli.rs`, instead of
each file deciding its own verify-cache setup. Every child gets a per-test
`FSLC_CACHE_DIR` under `CARGO_TARGET_TMPDIR` with `FSLC_CACHE` and
`FSLC_CACHE_VERIFY` removed, so a test no longer reads or writes the
developer's or CI runner's cache; a test that shares a cache on purpose passes
its directory explicitly. `tests/cli_cache_isolation_guard.rs` fails when a
test source names the binary without the helper and without setting
`FSLC_CACHE=off` or its own `FSLC_CACHE_DIR`; it has no allowlist. Test
assertions and product behaviour are unchanged.
