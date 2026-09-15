Fixed (#1065)!: `analyze --projection code_audit` no longer misreads a prose
mention of the `@fsl.trace ` sentinel (for example, a comment describing the
annotation convention itself) as a broken annotation. A line is now treated
as an annotation attempt only when the text following the sentinel, after
stripping leading whitespace, starts with `{`; any other line is skipped
without a finding or an error. A line where the sentinel is followed by `{`
but the JSON is malformed, or where the JSON parses but fails
schema/kernel-target/`origin_assurance` validation, still fails with the
same `CodeAuditError::Semantics` exit-2 error as before -- only the
discriminator for "is this an annotation attempt at all" changed. Recorded
as breaking because it changes the CLI's exit code for an existing
observable input: a source tree containing a prose mention of the sentinel
with no following JSON object, which previously exited 2 (`error`), now
exits 0 (`analyzed`) with the mention simply absent from `findings` and
`coverage`. This is a known cost of the `{`-prefix discriminator, not an
unverified gap: a malformed annotation whose payload does not start with `{`
(for example a typo like `@fsl.trace requirement=REQ-1`) previously failed
with the same `malformed code trace` error and is now skipped as well --
the rule cannot distinguish it from a prose mention, so the fix trades
detecting that narrower class of broken annotation for not misreading prose.
