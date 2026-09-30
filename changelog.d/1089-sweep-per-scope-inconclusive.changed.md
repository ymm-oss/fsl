Changed (#1089): `fslc sweep` settles depth-limited (`insufficient_depth`)
reachability cells per `--instances`/`--values` scope instead of over the
whole grid. A determinate success at any depth of the same scope still
settles it, but a success in another scope no longer does: with no true failure,
the grid is `sweep_passed`/exit 0 only when every scope has a determinate
success at some depth, and otherwise `sweep_inconclusive`/exit 1 with a null
`minimal_counterexample`. The new `sweep.inconclusive_scopes` array lists each
all-inconclusive `{instances, values}` scope (`[]` when there is none) under
every sweep result. This is a breaking CLI contract change for `--instances`/
`--values` sweeps only; `--depth`-only sweeps are one scope and keep their 4.7.0
verdicts. Migration: a grid such as `--values Amount=1..5 --depth 3..3`, where
`Amount` 1..3 never witnesses a `reachable` that 1..4 does, now returns
`sweep_inconclusive`/exit 1 where 4.7.0 and 4.8.0 returned `sweep_passed`/exit
0. Raise `--depth` for the listed scopes or narrow the sweep bounds; consumers
already gating on the exit code need no change beyond expecting exit 1 for such
grids.
