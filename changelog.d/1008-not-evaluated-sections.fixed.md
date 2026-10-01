Fixed (#1008): an option that made `verify` skip a declared check removed it
from the envelope with no reason, so a broken inline `implements` seam passed
`verify --property X` with exit 0 looking exactly like a spec without one.
`--property`, `--exclude-property`, and `--from-state` now keep the key as
`implements: {"abs", "result": "not_evaluated", "reason", "reasons"}`
(`reason` is `property_selection` / `property_exclusion` / `from_state`), and
`--instances` / `--values`, under which a requirements `acceptance` /
`forbidden` scenario can fall outside the overridden scope, report the skipped
ones as `requirement_traces: {"result": "not_evaluated", "reason":
"bounds_override", "skipped": [{kind, id, reference}], ...}` (#1218). The top-level `result` and the exit code
are unchanged (a selected run still speaks only for what it checked), a spec
that declares no such check keeps the key absent, and an unfiltered `verify`
envelope is unchanged. A selected run now also reports the same compose
warnings (`fair_not_inherited`) and `no_user_invariants` suppression as a full
run; `--property` used to drop them. The Worker has no selection options and
always runs the full set. `sweep`, whose cells always carry scope overrides,
copies each skipped section into the cell's `summary` row and reports the
union over cells as `sweep.not_evaluated: {sections, reasons}`; the grid
verdict and exit code are unchanged. The frozen Python reference's per-scenario
`acceptance_skipped` downgrade under overrides is ported (#1218).
