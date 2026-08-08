Fixed issue #730: `fsl-runtime`'s `bfs` (the solver-free agreement oracle),
`first_self_violation` (`check_refinement`'s self-consistency precondition),
and `verify_explicit_selected` each held a `Monitor` -- and therefore the
whole `KernelModel` by value -- per queued/frontier node, so exploring `n`
states cloned the model `n` times; `first_self_violation` additionally
cloned its whole accumulated `Vec<TraceStep>` per node, worse than
`find_boundary_violation` before issue #697's fix. Measured directly on the
`LabelCoreRepro` reproducer (release build, depth 3, 16,290 states): `bfs`'s
peak memory footprint dropped from ~946 MB to ~236 MB with the state count
unchanged. All three now carry only a `State` (plus a scratch `Monitor`
re-pointed at each popped state, `find_boundary_violation`'s established
pattern) and reconstruct a trace from parent links only when one is
actually needed. `first_self_violation`'s multi-root case (issue #493:
nondeterministic init) is handled by `trace::reconstruct_trace` discovering
each state's actual root by walking parent links to whichever parentless
state the chain terminates at, rather than assuming a single passed-in
initial state. No public contract changed: verdicts, JSON envelopes, and
state counts are identical before and after on the reproducer and on the
`specs`/`examples` corpus.
