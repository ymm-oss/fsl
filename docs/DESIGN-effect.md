# FSL Effect Lifecycle Design

Status: adopted as part of fsl-domain v0.

`effect` is the async/effect slice of the `domain` dialect. The authoritative
implementation design is in [`DESIGN-domain.md`](DESIGN-domain.md); this file
records the effect-specific semantics.

An async effect is modeled as a finite lifecycle keyed by a declared
`correlation_id`:

```text
NotStarted -> Pending -> Succeeded | Failed | TimedOut | Cancelled | Compensated
```

The v0 implementation lowers that lifecycle to kernel `Map<CorrelationId, EffectStatus>` and
`Map<CorrelationId, Attempt>` state. Completion actions require the request to be
pending, retry actions require a failed/timed-out status and `attempts < max`,
and successful completion is sticky.

Irreversible effects must declare `idempotency_key`; otherwise
`fslc domain check` emits `irreversible_effect_without_idempotency_key` and does
not run the formal kernel check. Missing timeout/retry/fallback and possible
late completion without a stale policy are reported as design warnings.

Irreversible effects should also declare a compensation event or be compensated
by a saga. If not, `fslc domain check` emits
`missing_compensation_for_irreversible_effect` as a warning. A `reliable` effect
must be paired with an `outbox` boundary on the effect or owning saga; otherwise
`reliable_effect_without_outbox_boundary` is reported.

Runtime evidence is handled by `fslc domain replay --logs <events.jsonl>`.
Replay checks command acceptance, effect request/completion correlation,
duplicate irreversible completion, and stale lifecycle ordering against the
finite model. It returns `conformance_checked` or `nonconformant`; this is
runtime observation evidence, not kernel proof.

The model is intentionally finite. It proves the declared lifecycle shape, not
real queue delivery, payment-gateway behavior, wall-clock timeouts, or production
exactly-once semantics.
