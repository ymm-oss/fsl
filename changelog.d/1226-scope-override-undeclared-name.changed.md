Changed (#1226): an invocation that passed `--instances` / `--values` naming
something a `requirements` / `business` document does not declare as `entity` /
`number` (including a raw `type X = lo..hi` range) used to run and report the
verdict of the unshrunk model with exit 0/1; it now fails with exit 2 before
verification. Migration: override only declared `entity` / `number` / `process`
names; to shrink a raw range, declare it as `number X` with
`verify { values X = lo..hi }` and override that, or edit the `type` bounds.
