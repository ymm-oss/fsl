Documented (#724): `docs/DESIGN-domain.md`'s Findings section restated to
match the native `fslc domain check`/`domain replay` implementation instead
of the 7-kind list it previously claimed as implemented. Native
`fslc domain check` implements exactly 4 finding kinds
(`rust/fsl-tools/src/domain.rs:34-66`):
`irreversible_effect_without_idempotency_key`,
`pending_effect_without_timeout_or_fallback`,
`missing_compensation_for_irreversible_effect`, and
`reliable_effect_without_outbox_boundary`. `aggregate_boundary_violation` and
the static form of `uncorrelated_async_completion` were never findings: an
`evolve` writing outside its aggregate and an async effect with no
`correlation_id` both fail Kernel lowering itself
(`rust/fsl-core/src/domain_lowering.rs`) with a top-level `result:"error"`,
`kind:"semantics"` envelope outside the finding schema, a stronger
(fail-closed) guarantee than a warning/error finding would give; only the
`domain replay` form of `uncorrelated_async_completion` is a real finding
(`rust/fslc/src/main.rs`, `rust/fslc/tests/issue_518_domain_replay_detection.rs`).
`missing_decide_for_command`, `missing_evolve_for_event`, and the
previously-undocumented `cross_aggregate_update_without_event` exist only in
the frozen Python compatibility reference (`src/fslc/domain_expand.py`), not
in native. `late_completion_without_stale_policy` is removed rather than
restated: reviving it would recommend the `on_stale` syntax native already
rejects fail-closed (#711). `saga_dead_end` and `process_wait_cycle` are
removed with no native replacement; whether to implement them is tracked in
#769. `docs/DESIGN-effect.md`'s effect-lifecycle summary and its
timeout/retry/fallback vs. stale-policy warning claim are corrected to match.
`docs/intro/domain.en.html` and `docs/intro/domain.ja.html` carried the same
7-kind over-statement in their own findings table and prevention cards and
are corrected identically.
