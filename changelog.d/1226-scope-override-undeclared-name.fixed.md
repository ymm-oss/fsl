Fixed (#1226): `verify` / `sweep` `--instances NAME=N` / `--values NAME=LO..HI`
on a `requirements` or `business` document silently dropped a `NAME` the
document does not declare as `entity` / `number` — a typo, or the name of a raw
`type X = lo..hi` range — while still echoing it under `bounds_overrides`, so
`--values X=0..2` returned a verdict over the full declared domain of `X`. The
native CLI now rejects such a name as `docs/manual/LANGUAGE.md` and the frozen
Python reference already did: `result: "error"`, `kind: "semantics"`,
`verify instances references undeclared entity 'NAME' at 1:1` /
`verify values references undeclared number 'NAME' at 1:1`, exit 2 — the same
message a kernel `spec` already produced. A requirements `process` name counts
as an entity (it is one after lowering), and the business dialect, which has no
`number`, rejects every `--values`. Kernel `spec` behavior and every override
of a declared name are unchanged.
