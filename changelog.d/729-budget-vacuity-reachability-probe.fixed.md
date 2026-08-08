Fixed issue #729: `verification_warnings`'s `vacuous_implication`/
`vacuous_leadsto` reachability probe (`fsl-runtime`) ran one **unbudgeted**
concrete BFS per antecedent/trigger, cloning the whole `KernelModel` per
explored node. On the `LabelCoreRepro` reproducer
(`rust/fslc/tests/issue_697_all_properties_memory.rs`), isolating a single
affected property (`--property PublishedWasReviewed`) consumed ~1,095 MB
(measured control, before this fix; the issue reported ~1,096 MB), against
~47 MB for an unaffected property, and `--vacuity ignore` did not help —
`apply_vacuity_mode` only filtered the already-computed output. Every
implication antecedent and leadsTo trigger now shares one budgeted BFS
(`fsl_runtime::expression_reachability`, budgeted at the same
`CONCRETE_PROBE_BUDGET` `find_boundary_violation` uses and reusing its
established scratch-`Monitor` pattern from issue #730/#776), which drops the
same isolated-property case to ~348 MB (no vacuity option change) or ~44 MB
(`--vacuity ignore`, which now genuinely skips the probe instead of
filtering its output) and removes the per-property multiplier the shared BFS
was designed to eliminate. Reachability is now tri-state
(`Reachability::{Reachable, Unreachable, Exhausted}`): a candidate that hits
the budget before resolving reports the new `vacuity_probe_truncated` kind
(added to `fsl_core::VACUITY_KINDS`, 6 → 7) instead of silently becoming
`Unreachable` (fail-open) or being dropped (also fail-open under `--vacuity
error`) — depth-bounded non-closure is unaffected and still reports
`vacuous_implication`/`vacuous_leadsto` exactly as before. `skip_vacuity_probe`
is threaded as an explicit argument from the one CLI derivation point
(`--vacuity ignore` in `verify`/`sweep`); the `ledger`/`html`/`mutate`
baseline and the wasm Worker surface (neither has a `--vacuity` option)
always compute it. Negative controls: the `--vacuity ignore` envelope equals
the `--vacuity warn` envelope with vacuity-kind warnings filtered out
(`issue_729_vacuity_ignore_skip.rs`); `ledger`/`html` output for an ordinary
vacuous finding is unchanged, and a `vacuity_probe_truncated` finding cannot
move a requirement's assurance class since `assurance_token` never reads
`warnings` (`issue_729_vacuity_probe_truncated_ledger.rs`); a
corpus-conservation test confirms no maintained spec exhausts the shared
budget (`issue_729_vacuity_probe_corpus_budget.rs`).
