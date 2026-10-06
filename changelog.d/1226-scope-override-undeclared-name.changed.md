Changed (#1226): an invocation that passed `--instances` / `--values` naming
something a `requirements` / `business` document does not declare as `entity` /
`number` (including a raw `type X = lo..hi` range) used to run with exit 0/1
and report the verdict of a model the override did not shrink as asked — the
name was dropped, or, for a raw range `X` that types a requirements process
field, the override's `lo` replaced the field's default initial value (so
`--values X=0..1` could flip the verdict); it now fails with exit 2 before
verification. Migration: override only declared `entity` / `number` / `process`
names; to shrink a raw range, declare it as `number X` with
`verify { values X = lo..hi }` and override that, or edit the `type` bounds.
Separately, `fslc diff` on a `requirements` document whose `verify` block
carries `values X = a..b` for a raw `type X = lo..hi` that types a `process`
field, when the scope changed, no longer reports a false `behavior_removed`
(`old_to_new` `refinement_failed`). It used to forward `X` to the re-scoped old
model resolved to the type's `lo..hi`, which moved the field's default initial
value from `a` to `lo`; the re-scoped old model now keeps the document's own
bound, so only the real differences are reported. Specs without such a field
diff as before.
