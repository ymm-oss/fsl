Fixed (#731): `implicit_initial_value` now fires for every domain aggregate
state field the renderer already gives an implicit default -- `Int` (`0`),
`Option<T>` (`none`), `Set<T>` (`Set {}`), a top-level `Map<K, V>` (the dense
per-key `forall` init), and `value_object`-typed fields (their struct-literal
default) -- not just the original four scalar shapes (`Bool`, enum, range,
external-placeholder). `rust/fslc/src/frontend_output.rs`'s warning now
reads the selected value from `fsl_core::domain_type_default`, the same
total dispatch `domain_kernel_source` already used
(`rust/fsl-core/src/domain.rs`'s `Context::default_for_type`), instead of a
second, non-exhaustive copy of the dispatch that silently excluded any type
name containing `<`. An enum default is rendered in domain-source form (the
bare declared member, e.g. `Pending`) rather than `domain_kernel_source`'s
kernel-scoped mangled identifier (`Status_Pending`); a `DefaultForm`
threaded through `Context::default_for_type`/`default`/`normalize` selects
the right form at every enum-rendering site, including a value_object's own
explicit default field and an enum nested inside a value_object's struct
literal or a top-level Map's per-key value, so nesting depth cannot bring
the mangled form back. New regression tests
(`rust/fslc/tests/issue_250_initialization.rs`) cover container/value_object
coverage, an explicit default still suppressing the warning, bare `Int`
warning like every other scalar shape, and a nested enum inside a
value_object/Map staying bare (never `domain_kernel_source`'s mangled form)
at any depth; a `domain_expand`-vs-warning string-level check confirms the
selected value matches for every shape that renders identically in both
domain-source and kernel form -- deliberately excluding enum-bearing values,
where the warning's bare member and `domain_kernel_source`'s mangled
identifier now intentionally differ. A top-level `Map<K, V>` field (no
whole-field initializer syntax exists at all) and a `Set<T>`/`value_object`
field whose brace-literal default cannot yet round-trip through `fslc
fmt`'s reformat-and-reparse pass (issue #770, found and reproduced
independently while implementing this fix) both still warn under `check`
but omit the machine-applicable insertion -- chosen by an allowlist over
the field's type shape (`Option` only; everything else, including any
future brace-literal-rendering constructor, fails closed until reviewed),
not the rendered value's text -- and keep `edition_severity.next` at
`warning` rather than `error`, since `migrate --write` is fail-closed (it
would not write a corrupted file) but attempting that insertion would trip
#770's reformat failure and fail migration for the whole file, dropping
every other, otherwise-safe edit in it too. A "defect witness" test pins
#770's `fslc fmt` symptom directly so a future fix to #770 turns it red;
its failure message also flags issue #785, a second pre-existing defect on
the same `Context::normalize` enum-mangling guard (found independently
during review, not reachable through this PR's own withheld insertion) that
must be fixed before the `value_object` insertion can be safely re-enabled.
