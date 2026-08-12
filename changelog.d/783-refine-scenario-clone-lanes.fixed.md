Fixed issue #783: `fsl-runtime`'s `check_refinement` correspondence walk,
`action_cover_traces`, and `leadsto_response_traces` were the three
remaining lanes still holding a `Monitor` -- and therefore the whole
`KernelModel` by value -- per queued/frontier node, the same defect class
issue #730 fixed in `bfs`, `first_self_violation`, and
`verify_explicit_selected`; the first two additionally cloned a growing
`Vec<TraceStep>` per node. Measured directly on the shared `LabelCoreRepro`
refinement fixture (release build, depth 3): `check_refinement`'s peak
memory footprint dropped from ~1,728 MB (consistent with the issue's own
reported ~1.72 GB) to ~491 MB, with `refines`/`refinement_failed` verdicts
identical before and after. `check_refinement` and `action_cover_traces`
now carry only a `State` per frontier node (plus a scratch `Monitor`
re-pointed at each popped state) and reconstruct a trace from parent links
only when one is actually needed, using the same `trace::ParentLink`/
`reconstruct_trace` machinery issue #697 established; both are now locked
onto a shared `LeanFrontier` type so a future per-node `Monitor`/
`Vec<TraceStep>` clone in either is a consumer-side type error, not only a
review-time judgment call. `leadsto_response_traces` is a partial fix
scoped to the `Monitor` clone alone: its walk has no `visited` dedup (a
`leadsTo` pending/response history is path-dependent, so two routes
reaching the same state must stay distinct path-trees), so its per-node
`Vec<TraceStep>` clone is unchanged -- but it is now locked onto a second
shared type, `PathFrontier` (`(State, Vec<TraceStep>, usize)`, no
`Monitor`), so a future per-node `Monitor` clone there is also a
consumer-side type error, closing the one lane an independent review found
without either a ceiling or a type guard. A negative control
(`rust/fsl-runtime/tests/issue_783_refine_memory_ceiling.rs`, Linux-only,
its `LabelCoreRepro` fixture shared with the `issue_730_*` ceiling tests via
`tests/support/mod.rs`) adds a calibrated memory ceiling for
`check_refinement`'s walk and, as PR #776 deferred to this issue, one for
`first_self_violation`'s own frontier. No public contract changed:
`RefinementCheck`/`LeadstoResponse` fields, JSON envelopes, exit codes, and
verdicts/trace/state-count output are identical before and after, checked
directly across every `specs`/`examples` corpus refinement mapping (`fslc
refine`, 27 registered triples) and every `spec`-dialect corpus file (`fslc
scenarios`, 93 files, default and `--depth 6`).
