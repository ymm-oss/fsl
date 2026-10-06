Fixed (#1226): `verify` / `sweep` `--instances NAME=N` / `--values NAME=LO..HI`
on a `requirements` or `business` document accepted a `NAME` the document does
not declare as `entity` / `number` — a typo, or the name of a raw
`type X = lo..hi` range — and echoed it under `bounds_overrides`. The name was
dropped, so `--values X=0..2` returned a verdict over the full declared domain
of `X`, except that for a raw range typing a requirements `process` field the
override's `lo` silently became the field's default initial value. The
native CLI now rejects such a name as `docs/manual/LANGUAGE.md` and the frozen
Python reference already did: `result: "error"`, `kind: "semantics"`,
`verify instances references undeclared entity 'NAME' at 1:1` /
`verify values references undeclared number 'NAME' at 1:1`, exit 2 — the same
message a kernel `spec` already produced. A requirements `process` name counts
as an entity (it is one after lowering), and the business dialect, which has no
`number`, rejects every `--values`. Kernel `spec` behavior and every override
of a declared name are unchanged. `fslc diff`, which forwards a document's own
`verify` bounds to the re-scoped old model, now forwards only the declared
names; this changes its output only for a raw range typing a requirements
`process` field (see the Changed entry).
