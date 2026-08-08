Fixed (#723): effect `retry { backoff ... }` clauses (e.g. `retry { max_attempts
3; backoff exponential }`) are now rejected fail-closed by both lowering
paths through the same `validate_lowerable_constructs` gate
(`rust/fsl-core/src/domain_lowering.rs`) that #710/#711/#712 established:
no accepted design pins a backoff strategy's execution meaning in the
finite model, and the parsed `backoff` value was never read by either
lowering path or by the frozen Python reference
(`src/fslc/domain_parser.py`, `src/fslc/domain_ir.py`), which only stores
it in the IR. **Migration**: remove the `backoff ...` line from any `retry`
block; it never had executable meaning under `check`/`verify`, so removing
it changes no verification outcome. `retry { max_attempts N }` without a
`backoff` clause is unaffected and still lowers the `attempts < N` guard.
`examples/domain/order_async_effect.fsl` and
`rust/fslc/tests/fixtures/issue_518_domain_replay.fsl` had their now-rejected
`backoff exponential` lines removed as part of this fix, and
`docs/intro/domain.{en,ja}.html`'s matching code sample was updated to
match. See `docs/DESIGN-domain.md`. Tracked mechanism for future migration
diagnostics: #702, #703. Remaining #723 items (`projection`, effect
`compensation`, saga `outboxes`/`inboxes`) are out of this fix's scope.
