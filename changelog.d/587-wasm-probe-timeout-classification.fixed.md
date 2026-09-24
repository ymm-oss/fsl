Fixed (#587): the WASM browser probe now reports why it failed instead of
folding every failure into one exit code. A stalled CDP call or completion
poll exits 124 (`probe_timeout`, with `mode` `cdp` or `poll`), a refuted
native/WASM parity comparison -- including a malformed WASM envelope, a stale
corpus exclusion, or agent fail-closed drift -- exits 65 (`parity_violation`),
and anything else, including a malformed native envelope, stays a harness
failure (exit 1). All three still fail the `WASM` job; nothing is retried or
downgraded. A failure before the browser starts now exits instead of hanging
on the open HTTP server.
