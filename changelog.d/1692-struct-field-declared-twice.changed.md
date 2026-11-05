Breaking removal (#1692): a `struct` or `value_object` that declares a field twice is refused at the repeated field, whether or not the two types agree and whether or not anything reads it, with exit 2, `kind: "name"`, `duplicate struct field 'P.x'` and `loc` at the second `x`. A component reports `component "piece.fsl" failed to check (duplicate struct field 'P.x' at piece.fsl:3:20) at root.fsl:2:3` with `kind: "semantics"`, and an inline `implements` reports `implements "abs.fsl" failed to check (duplicate struct field 'P.x' at abs.fsl:3:20)` with `kind: "type"` and `loc` on the `implements`.

These forms passed `check` before. With different types, `check` admitted a value by the first type and the bounds refused it:

- `struct P { x: K, x: Bool }` or `struct P { x: K, x: K }`, in a `spec`, in a `requirements` document, or in a composed component, read or not.
- `value_object C { value: Q = 0; value: Q = 0; }`. `domain expand` and `domain analyze` also exited 0 on a `value_object` whose copies differ, and `expand` wrote it out.

`verify` no longer returns `violated`, `verified` or `proved` on those forms, `sweep` no longer returns `sweep_failed` or `sweep_passed`, `replay` no longer returns `conformant` or `nonconformant`, and `mutate`, `html`, `ledger`, `diff`, `migrate`, `document claims`, `kernel`, `typestate`, `lint`, `fmt`, `explain`, `analyze`, `conformance`, `domain expand` and `domain analyze` exit 2 instead of their own results. `fmt` reports it as `kind: "semantics"` with no `loc`.

An input that already failed now reports the repeat, so its kind can change: `struct P { x: Set<K>, x: K }` and `struct P { x: Nope, x: K }` from `type` to `name`.

Migration: keep the one field you mean. There is no automatic migration.

The 650-form on-disk corpus has an empty transition set: base `b2cdb394fedcba015261df05587c4721583329e3`, head `66ca55474a05a8aefc9d92b29095505f1f1742af`, `fslc check` stable and with the next edition enabled, and `fslc kernel` versions 1 and 2, complete output compared.

Rust API: `SpecItem::Struct` gains `field_spans`.
