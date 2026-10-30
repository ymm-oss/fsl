# Implementation connection, NFR and causal review

Contents: [Adapter](#9-implementation-connection-the-testgen-adapter-contract),
[NFR](#11-non-functional-requirements-nfr),
[causal](#12-the-causal-profile-review-only).

## 9. Implementation connection (the testgen Adapter contract)

### Layer selection for conformance tests

Generate from the layer whose action/state granularity the implementation Adapter
actually projects. The refinement chain ends with implementation conformance to
the design, not direct execution of every upper positive trace. A legitimate
design may add review steps or stronger guards; an upper immediate-finish positive
trace can then falsely fail a sound refinement.

From upper layers reuse **forbidden negatives only**, with faithful action/state
projection. Positive scenarios, covers and random walks belong to that layer's
anchor, not a substitute for design-level positives. Refinement is trace inclusion
of detail in abstract behavior; it does not make every abstract positive executable
in detail. Requirements calls positives `scenario`; diagnostic/generated names may
still say acceptance.

### Adapter and atomicity

- `reset()`: put the real implementation in the modeled init state.
- `step(action, params)`: execute the real operation. Public composed/branch names
  are `alias.action` / `submit.b1`.
- `observe() -> dict`: project logical state. Enum = member string, Option = null/
  payload, Seq = list, Map = string-keyed dict, struct = dict, composed keys = alias.var.

Random-walk checks use the concrete Monitor oracle, not handwritten expected
business logic. Classify divergence as spec/intent mismatch, Adapter error or
implementation defect before choosing the repair.

One model action is atomic. Generated sequences do **not** enumerate interleaving
or crash points inside its implementation. If one action spans transactions,
test interleavings/crashes at real transaction boundaries separately. Zero
conformance divergence does not establish implementation atomicity.

Emitters consume validated Public Kernel v1, scenarios and testgen-trace.v1, not
private ASTs. Schema/version/spec identity/state/action/parameter mismatches fail
closed. Compose is a producer exception because complete multi-file provenance is
not exportable yet; checked names/order feed the same adapter. Do not advertise
complete compose Kernel export on that basis.

The seed-fixed walk is capped at 100 steps, not --depth. A later discovered
violation cannot become a baked no-op expectation after Monitor rollback: testgen
returns the verification violation/exit/trace and writes no harness. Fix the spec
or verify deeply enough; do not lower depth to hide it.

| Target | Harness / logical values / wiring behavior |
|---|---|
| pytest (default) | `test_<spec>.py`; live random walk imports fslc.runtime.Monitor. |
| vitest | `<spec>.test.ts`; baked walk, no fslc/Python runtime. Reject integers outside ±(2^53−1) before generation. |
| swift | `<Spec>ConformanceTests.swift`; Swift Testing, not XCTest; `[String: Any]`, FSLNull.instance for Option none; FSLNotWired fails until wired. |
| kotlin | `<Spec>ConformanceTest.kt`; kotlin.test, `Map<String, Any?>`; numeric integer normalization through nested values, not Float/Double coercion; wider i64 literals/Long.MIN_VALUE preserved. Read params as Number. |
| dart | `<spec>_conformance_test.dart`; package:test / flutter test, recursive expected-key checks with equals for collection leaves. |
| phpunit | `<Spec>ConformanceTest.php`; PHP 8.1+/PHPUnit 10+, strict_types, associative arrays; strict leaf comparison, map-order independence, list length pinned. |

All targets fail unwired by default. --allow-unwired explicitly skips tests where
supported; Kotlin rejects it because no portable runtime skip exists. An all-skipped
green suite proves nothing. Give chain [impl] a fresh JUnit report with ≥1 executed
test and no failures/errors; zero/missing/stale evidence is indeterminate. Exit-code-
only evidence is not conformance for generated tests.

An unwitnessed reachable normally yields partial generated coverage plus a warning;
--strict restores all-or-nothing reachable_failed. Genuine verification failure is
returned verbatim, not recast as generic input error. Testplan is a selection of
accepting/requires_failed vectors with formal_result:not_run / assurance_effect:none,
not a run of those vectors. Counterexample export produces reproducer.v1 for bounded
safety-invariant bugs; it is not arbitrary liveness trace export.

### Using conformance as an implementation loop for an AI agent

1. Before code repair, confirm memo and rejection intent with the owner. Requirements
   rejection rules need actual guards and forbidden traces, not quoted text alone.
   Kernel extraction instead uses confirmed kernel properties and negative real-code
   tests; do not invent requirements declarations inside a kernel spec.
2. Read divergence: (a) spec misses/contradicts intent → ask spec owner, (b) projection
   wrong → fix Adapter, (c) real behavior wrong → fix implementation. Do not edit
   correct code to satisfy a misread contract.
3. Pin depth, domain sizes and execution coverage. Report remaining unexecuted
   tests/vectors/divergences; do not lower/filter/disable them to reach zero.
4. Count completion only after tests ran against real code, with known bad behavior
   rejected by the anchor. Model verification and conformance are separate claims.

## 11. Non-functional requirements (NFR)

| Intent | Model |
|---|---|
| Permission | role guard plus confirmed cross-action ghost property |
| Audit completeness | invariant accounting for every relevant state movement |
| Capacity | bounded types/Seq/count; test boundary and overflow attempt |
| Reliability | fault action, mode, fair recovery, response property |
| SLA/timeout | requirements time/age plus rule deadline, finite relative ticks |
| Probability/real latency | external evidence/documents, not kernel proof |

### Time/deadline rules

The [requirements syntax owner](../../fsl-requirements/references/syntax.md#time-terminal-states-measures-goals-and-policies)
has the complete example. `time { … }` is directly inside requirements, at most
one block; `deadline age <= K` is inside its `rule`, not a kernel declaration.
On generated tick, age increments while its predicate holds, resets otherwise;
bound comes from the deadline. Indexed ages are ordinary readable state.

An enabled urgent event freezes tick. Tick is generated: do not declare another
action tick. It advances clock state and has automatic stutter correspondence
where supported. A design refining a timed upper layer must still account for
that upper clock: tick→tick needs the same urgency guard and age update. A finer
design clock that runs while upper tick is disabled has no such abstract step
and can fail abs_requires_failed. In that case keep the agreed upper contract
time-less and verify SLA at the design clock; do not claim timing propagated.

### Vacuous SLA and deadline-urgency pattern

Making an always-enabled response urgent can stop time forever, verifying every
deadline vacuously. Read urgency_freeze and vacuous_deadline warnings. Make only
a handler enabled at the deadline urgent, from the source's actual policy:

```fsl-fragment
time {
  urgent respond_due
  age resp_age[c: CaseId] while stage(c) == Accepted
}
rule REQ-RESPONSE-003 "first response within the agreed tick budget" {
  deadline resp_age <= SLA_TICKS
}
```

This fragment needs the entity, events and rule-owned guard that enables the due
handler at `resp_age >= SLA_TICKS`. Do not infer an implementation or a deadline
from it. Calibrate: a K−1 deadline must violate at the expected boundary, and
removing urgency must allow a violating neglect trace. BMC witnesses it; induction
may need a confirmed auxiliary budget invariant such as age + remaining work <= K.

## 12. The causal profile (review-only)

Standalone `causal Name` sidecars describe hypotheses, not kernel causality:
variables with intervention/mediator/outcome/context roles; directed claims with
stable ID, version, polarity, lag, persists, basis and active/retired lifecycle;
declared feedback; tick/hour/day/week timebase, finite horizon; uses imports
binding to actual actions/KPIs/state/properties.

```text
fslc causal check MODEL
fslc causal analyze MODEL --projection causal_graph|causal_timeline|causal_traceability_graph [--format json|dot|mermaid]
fslc causal analyze MODEL --profile causal-review
fslc causal diff BEFORE AFTER
fslc causal analyze MODEL --evidence artifact.json [--lifecycle chain.json]
  [--as-of YYYY-MM-DD] --projection causal_evidence_graph
fslc causal verify-expectations MODEL [--depth K]
fslc causal observe-expectations MODEL --from-log events.jsonl --mapping mapping.fsl
  --scope scope.json --period-start YYYY-MM-DD --period-end YYYY-MM-DD
  [--out evidence.json] [--lifecycle-out lifecycle.json]
fslc causal ledger MODEL [--plans plan.json]... [--evidence ev.json]...
  [--lifecycle lc.json]... [--as-of YYYY-MM-DD]
```

There is no causal verify command. A successful check means well-formedness only.
Review findings have formal_status:not_a_violation; do_not_assume states the limit.
Undeclared positive-lag feedback warns, zero-lag cycles error. Cadence > arriving
claim persists.min is too coarse; unknown persistence is not_evaluable, not guessed.
Diff is structural: no evidence means support_transition:not_available. It flags
unversioned content changes, reactivation and repetition of retired relations.

Evidence artifacts fsl-causal-evidence.v0 pin claim **and content version** with
closed design/support/scope/period and canonical digest. Lifecycle records are
separate append-only digest chains. Invalid schema/digest/lifecycle fails exit 2.
Current support counts only matching version, subsuming scope, freshness, active
lifecycle and sufficient observation window; one source lineage is one vote.
Unknown lag or unconvertible window stays history with timing not evaluable, no
vote. One-sided scope dimensions are unassessable, not universal. Staleness needs
explicit as-of, never the wall clock.

Expectations are human-carved trigger/response/within-clock obligations derived
from claims; unsupported/missing/foreign clocks or fractional tick conversion
reject, and legacy supports is not a replacement for derived_from_claim. A passing
expectation does not prove its claim; a violated one does not refute it. Formal
assurance remains not_run and causal support remains its own axis.

Observation uses solver-free bounded-liveness replay. Scope/period/log/mapping are
required, never inferred. A nonconformant log aborts evidence generation. Artifacts
are observational, inconclusive, replay-observed—not directional causal support.

Ledger combines claims, immutable version-pinned validation plans, evidence and
observations with deterministic attention reasons and exclusions. Retired claims
have no attention reasons. A green ledger means required artifacts are present,
not study sufficiency, claim truth or project completion. Keep formal_assurance,
causal_support and attention_reasons distinct. Never call supported proved,
challenged refuted, or temporal co-occurrence causality.
