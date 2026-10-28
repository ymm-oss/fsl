Fixed (#1424): testgen preserves wide Kotlin integer literals and compares integral
values recursively through lists and maps, emits `PHP_INT_MIN` for i64 minimum,
rejects lossy Vitest integers with a located error, escapes Python string controls,
and inserts escaped source basenames only after trusted template slots are filled
in every target. Ordinary output outside the Kotlin helper and its comment is
unchanged.
