Documented (#723): `docs/DESIGN-effect.md`'s effect lifecycle diagram no
longer lists `Compensated` as a reachable status. `effect_outcome_member`
(`rust/fsl-core/src/domain_lowering.rs`) is a total function over exactly
`{Succeeded, Failed, TimedOut, Cancelled}`, and the renderer
(`rust/fsl-core/src/domain.rs`) mirrors the same set; an effect's
`compensation { emits ... }` block only feeds the presence-only
`missing_compensation_for_irreversible_effect` finding
(`rust/fsl-tools/src/domain.rs`) and never writes effect status, so
`Compensated` was unreachable in both v0 lowering paths. The generated
`EffectStatus` enum keeps the `Compensated` member (reserved) because
`rust/fslc/tests/issue_450_sibling_enum_conversion.rs` and the domain
characterization baseline already depend on it for refinement enum
conversion. A new negative control,
`effect_compensated_status_is_never_assigned_by_either_lowering_path`
(`rust/fsl-core/tests/domain_render_agreement.rs`), pins the current
unreachability across both lowering paths so a future writer for
`Compensated` is forced to update this test and `docs/DESIGN-effect.md`
together; real compensation semantics are tracked against #679's
correlation-indexed `SagaPhase` history, a precondition for defining which
compensation cancels which specific effect instance.
`docs/intro/domain.en.html` and `docs/intro/domain.ja.html` carried the same
`Compensated` transition in their effect-lifecycle callout and are corrected
identically.
