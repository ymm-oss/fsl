Fixed (#731): `implicit_initial_value` now fires for every domain aggregate
state field the renderer already gives an implicit default -- `Option<T>`
(`none`), `Set<T>` (`Set {}`), a top-level `Map<K, V>` (the dense per-key
`forall` init), and `value_object`-typed fields (their struct-literal
default) -- not just the four scalar shapes (`Bool`, enum, range,
external-placeholder). `rust/fslc/src/frontend_output.rs`'s warning now
reads the selected value from `fsl_core::domain_type_default`, the same
total dispatch `domain_kernel_source` already used
(`rust/fsl-core/src/domain.rs`'s `Context::default_for_type`), instead of a
second, non-exhaustive copy of the dispatch that silently excluded any type
name containing `<`; a new `domain_expand`-vs-warning string-level parity
check and container/`value_object` regression tests
(`rust/fslc/tests/issue_250_initialization.rs`) cover the fix, alongside a
negative control confirming an explicit default still suppresses the
warning. A top-level `Map<K, V>` field (no whole-field initializer syntax
exists) and a `Set<T>`/`value_object` field whose brace-literal default
cannot yet round-trip through `fslc fmt` (issue #770, found and reproduced
independently while implementing this fix) both still warn under `check`
but omit the machine-applicable insertion and keep
`edition_severity.next` at `warning` rather than `error`, since the next
edition cannot yet demand an initializer it has no safe way to insert.
