# FSL — `causal` profile: typed causal hypothesis graphs (review-only)

Issue: #320 (design). Spike: #319. Phases: #321 (check/analyze), #322 (evidence),
#323 (expectation lowering), #360 (observation bridge), #364 (portfolio ledger).

## 1. Goal and epistemic status

The `causal` profile structures long-horizon causal hypotheses — interventions,
mediators, outcomes, contexts, time lags, persistence, feedback, measurement,
and applicability scope — and analyzes their consistency with each other and
with existing FSL actions, KPIs, requirements, and properties.

FSL does **not** prove real-world causality. A causal claim is always a
hypothesis. There is, by design, no output path that attaches `proved` or
`verified` to a causal claim, and no reviewer, agent, or CI policy may present
`causal` output as formal assurance of a causal relationship.

The architecture separates three planes:

1. **Causal Specification Plane** — describes causal hypotheses, time delays,
   persistence, feedback, measurement, and applicability scope (this document
   and #321).
2. **Formal Expectation Plane** — verifies, with the existing kernel, only the
   observable temporal/safety contracts a human explicitly carves out of a
   causal hypothesis (#323). Causal claims themselves are never lowered.
3. **Evidence Plane** — ingests external experiments, observational studies,
   and statistical results, and displays deterministic support/challenge state
   per claim (#322, #360).

The causal graph itself never enters `KernelModel`, `fsl-runtime`, or
`fsl-solver`. A graph edge is a hypothesis about the real world, not a possible
transition: auto-converting it to a kernel action or `leadsTo` would conflate
"Q holds after P in time" with "intervening on P causes Q", and expanding
multi-year causality into kernel ticks would inflate state space and
verification cost for no assurance gain.

Every JSON output in the profile separates `formal_assurance` from
`causal_support` as two orthogonal axes and carries a `do_not_assume` array.

## 2. Adopted and rejected decisions

Adopted:

- `causal <Name> { ... }` is an independent top-level dialect/profile (a
  sidecar close to the governance catalog: causal models span multiple files
  and have a different lifetime and owner than a business spec).
- Causal source is type-checked into an independent typed `CausalModel`.
- Causal claims are always hypotheses; they never receive FSL `proved` /
  `verified` verdicts.
- External-evidence-derived `causal_support` and existing FSL
  `formal_assurance` are kept in separate fields, always.
- Model-wide discrete `timebase`; per-claim `lag` and `persists`; finite
  `horizon`; delayed feedback.

Rejected:

- Auto-conversion of causal claims to `leadsTo`. Z3 encoding of the causal
  graph.
- do-calculus, identifiability analysis, causal discovery, Bayesian inference,
  differential equations, system dynamics.
- Estimating effect sizes, decay curves, or probability distributions inside
  FSL. Inferring causal structure from natural-language rationale.

Considered alternatives and why they were rejected: a `cause` construct nested
in `business` (mixes kernel expansion with non-kernel semantics; may be
revisited later only as pure sugar expanding to a standalone model); expressing
claims as `leadsTo` (conflates temporal response with causality); reusing
`relation` + annotations (too weak for typed role/time/evidence contracts;
acceptable only as a Phase 0 prototype's internal notation); delegating fully
to an external tool (severs traceability to actions/KPIs/requirements); adding
causal constructs to the kernel (breaks runtime/solver/oracle agreement).

## 3. Vocabulary and guarantee boundary

| Term | Meaning | What FSL can guarantee | What FSL does not guarantee |
|---|---|---|---|
| causal variable | a variable playing an intervention/mediator/outcome/context role | ID, role, reference, and measurement-definition consistency | that all real-world relevant variables are enumerated |
| causal claim | a directed causal hypothesis `A -> B` | structural, temporal, and evidence-reference consistency | that A actually causes B |
| expectation | an observable formal contract | bounded/proved decidable by the existing kernel (#323) | that expectation success proves the causal claim |
| evidence | an external experiment/observation/statistical result | schema, digest, scope, and claim-reference consistency (#322) | that the study design or data quality is truly sufficient |
| causal support | the state of evidence supporting/challenging a claim | deterministic aggregation of supplied evidence | that support is universal causal proof |
| formal assurance | `proved` / `bounded` / `replay-observed` / `statistical` / `not_run` | the existing assurance-class contract | that different evidence kinds are comparable in meaning |

## 4. Surface syntax

The canonical example (normative for shape; the corpus under `examples/causal/`
is the executable reference):

```fsl
causal SubscriptionRetention {
  uses biz from "subscription_business.fsl"
  uses req from "subscription_requirements.fsl"

  timebase day        // all time values are non-negative integers in days
  horizon 730

  scope population {
    token all_users
    token new_users subset_of all_users
  }
  scope environment { token production }
  scope segment {
    token self_serve
    token enterprise
    self_serve disjoint_with enterprise
  }
  default_scope {
    population all_users
    environment production
    segment self_serve
  }

  clock req_clock {
    kernel req
    1 tick = 1 day
  }

  variable onboarding_support {
    role intervention
    binds action req.enable_onboarding_support
    covers REQ-ONBOARDING
  }

  variable first_success {
    role mediator
    observes kpi biz.first_success_7d
    cadence 1
  }

  variable habit_formation {
    role mediator
    latent
    proxy kpi biz.weekly_active_days
    cadence 7
  }

  variable retention_90d {
    role outcome
    observes kpi biz.retention_90d
    cadence 7
  }

  variable seasonality {
    role context
    observes kpi biz.seasonality_index
    cadence 30
  }

  claim C_Onboarding_FirstSuccess onboarding_support -> first_success {
    version 1
    status active
    polarity positive
    lag 0..7
    persists 7..30
    basis hypothesis
    evidence EXP_ONBOARDING_2026
  }

  claim C_FirstSuccess_Habit first_success -> habit_formation {
    version 1
    status active
    polarity positive
    lag 14..90
    persists 30..180
    basis hypothesis
  }

  claim C_Habit_Retention habit_formation -> retention_90d {
    version 1
    status active
    polarity positive
    lag 60..180
    persists 90..365
    basis hypothesis
  }

  claim C_Retention_Onboarding retention_90d -> onboarding_support {
    version 1
    status active
    polarity positive
    lag 0..30
    persists unknown
    basis assumption
  }

  feedback F_RetentionLoop {
    claims C_Retention_Onboarding,
           C_Onboarding_FirstSuccess,
           C_FirstSuccess_Habit,
           C_Habit_Retention
  }

  evidence EXP_ONBOARDING_2026
    from "evidence/onboarding-2026.causal.json"
}
```

### 4.1 `uses` imports and reference paths

`uses <alias> from "<relative-path>"` binds an identifier alias to one FSL
source file, resolved relative to the causal file's directory (the same
resolution rule as `use`/compose). Aliases are unique within a model;
re-binding an alias is a check error. Every cross-model reference is an
alias-qualified two-segment path `alias.name` (`binds action
req.enable_onboarding_support`, `observes kpi biz.first_success_7d`,
`clock ... kernel req`). Unqualified or unknown-alias references are
`causal_unknown_reference` errors. The imported file is parsed and validated
with the normal frontend for its dialect; a file that fails its own check
fails the causal check (fail-closed, no partial import).

### 4.2 `variable` field contract

| Field | Required | Meaning |
|---|---:|---|
| `role` | yes | `intervention` / `mediator` / `outcome` / `context` |
| `binds action` | in principle yes for intervention | reference to an intervenable FSL action |
| `observes` | in principle yes for outcome | reference to a KPI, state, property, or typed predicate |
| `latent` | optional | a concept not directly measured |
| `proxy` | optional | proxy measurement for a latent variable |
| `cadence` | recommended for observed/proxy | measurement interval in timebase units |
| `covers` | optional | requirement-ID traceability |
| `scope` | optional | strict token sets for population / environment / segment |

`role` and observability are separate concepts (a mediator may be observable
or latent). Violations of the "in principle" rows are warnings
(`causal_intervention_unbound`, `causal_outcome_unmeasured`), not errors.

### 4.3 `claim` field contract

Anonymous edges are not allowed: evidence, diff, traceability, and
change-impact analysis need stable anchors.

| Field | Required | Meaning |
|---|---:|---|
| source / target | yes | causal variable IDs |
| `polarity` | yes | `positive` / `negative` / `unknown` |
| `lag` | yes | `min..max` or `unknown` |
| `persists` | yes | `min..max` / `unknown` / `unbounded` |
| `basis` | yes | `hypothesis` / `assumption` |
| `evidence` | optional | list of external evidence IDs |
| `scope` | optional | claim applicability scope |
| `covers` | optional | requirement/control IDs |
| `version` | yes | monotonically increasing positive-integer content version |
| `status` | yes | `active` / `retired` |
| `superseded_by` | optional when retired | successor claim ID |

`basis supported` cannot be written in source. Support is derived from
evidence only (#322); letting humans declare it invites divergence from the
actual evidence.

### 4.4 Scope declaration and containment semantics

Scope tokens are not free strings: they are declared per dimension
(`population` / `environment` / `segment`) inside the model. Containment is
explicit `subset_of`, symmetric non-containment overlap is `overlaps`, and
exclusivity is symmetric `disjoint_with`. Declarations whose transitive plus
symmetric closure contradicts itself, cyclic strict inclusion, undeclared
tokens, and cross-dimension references are check errors. Overlap or
disjointness is never inferred from names or shared parents. Custom
namespaced tokens are allowed only when declared.

A claim without a scope inherits the explicit model `default_scope`; with no
default either, its scope is `unspecified` and evidence comparison returns a
fail-closed diagnostic instead of assuming an implicit universal scope.

Evidence scope relates to claim scope by comparing per dimension over the
transitive closure of declared containment, yielding exactly three values for
comparable scopes:

- `subsumes`: the evidence scope contains every token of the claim scope
  (exact match included). Example: `all_users` subsumes `new_users`.
- `partial_overlap`: at least some overlap is established (via `overlaps` or
  partial token containment) but the whole claim scope is not contained.
- `disjoint`: the closure of `disjoint_with` establishes an empty intersection
  in at least one required dimension.

Multi-dimension composition: any dimension `disjoint` ⇒ whole `disjoint`; all
dimensions `subsumes` ⇒ whole `subsumes`; otherwise `partial_overlap`. When
declared tokens still cannot be related from `subset_of` / `overlaps` /
`disjoint_with`, that is not a model check error; at evidence-comparison time
it becomes an `evidence_scope_unassessable` warning and the evidence is
excluded from current support. A missing scope with no model default gets the
same warning. Undeclared tokens, contradictory relation declarations, and
dimension-kind mismatches are well-formedness errors that stop evidence
analysis entirely. #322's support aggregation applies `subsumes` directly;
`partial_overlap` is transportability review; `disjoint` is a mismatch.

### 4.5 Claim content version and lifecycle

A claim ID is the stable anchor for diff, evidence, and traceability; it is
separated from content. The content version covers causal meaning — source /
target, polarity, lag, persists, basis, scope, future moderation — and
excludes evidence references, derived support, and lifecycle metadata
(`status`, `superseded_by`). Adding evidence or retiring a claim does not
self-referentially bump the content version. Evidence artifacts pin the claim
`version`, not just the ID; an artifact pinned to a non-current version is
#322's `evidence_claim_version_mismatch` warning — retained as historical
evidence for the old version, excluded from current support aggregation.

Refuted or abandoned claims are not deleted: they become `status retired`.
Retired claims are excluded from current reachability, timeline, and support
aggregation, but remain in causal diff, the evidence graph, and history
projections, so re-proposing an already-refuted hypothesis is detectable.
Retirement itself is not a formal proof of falsity.

## 5. Time semantics

- `timebase` is one discrete unit for the whole model; all time values are
  non-negative integers in that unit. Variable-length months/years are
  avoided; normalize to `day` etc.
- `lag a..b` is the interval from the source change/intervention to the first
  detectable response of the target.
- `persists a..b` is the interval the effect is assumed to last after response
  onset. It feeds observation-window checks; it is never used for effect-size
  convolution or decay computation.
- The first-response interval of an acyclic path is the **Minkowski sum** of
  its claims' lag intervals (e.g. `0..7` + `14..90` + `60..180` = `74..277`).
- A path containing `lag unknown` has time reach `unknown`. No convenient
  default is substituted.
- Clock mappings connecting to the kernel's generated `tick` are fixed as
  named `clock` declarations directly under the `causal` block. A declaration
  binds an imported kernel spec, a positive-integer number of kernel ticks,
  and a positive-integer number of causal timebase units; expectations (#323)
  reference the clock by name. One mapping may be shared by several
  expectations but is never implicitly applied to a different kernel spec.
- Missing mapping, mapping to a different spec, or a `within` that does not
  convert to an exact integer number of kernel ticks is fail-closed. "1 kernel
  step = 1 day" is never assumed; nothing is rounded. These conditions are
  identical to #323's expectation-compiler gates.

## 6. Feedback semantics

A directed cycle is not automatically an error (network effects, referral,
capability accumulation, and negative control loops are ordinary).

- **Instantaneous loop** (minimum lag sum over the cycle = 0): check error —
  its semantics degenerate.
- **Delayed feedback** (minimum lag sum > 0): allowed but requires an explicit
  `feedback` declaration (an undeclared delayed cycle is the
  `causal_unacknowledged_feedback` warning, §11.5).
- Path polarity composes as the sign product of edge polarities:
  `positive × positive = positive`, `negative × negative = positive`,
  `positive × negative = negative`, `negative × positive = negative`;
  `unknown` is absorbing (any `unknown` edge makes the path `unknown`). The
  empty path has no polarity.
- Each `feedback` declaration names one simple directed cycle as an ordered
  claim list. An SCC with several cycles gets one declaration and
  classification per cycle; no single sign is fabricated for a whole SCC.
- A feedback loop is classified from the product of its cycle-edge polarities
  as `reinforcing` (positive) / `balancing` (negative) / `unknown`. The
  classification is a first-class derived attribute `loop_class` in
  `causal-graph.v0`, and each representative cycle carries the claim-ID list
  used for the composition as its witness.
- A feedback declaration only acknowledges the loop's existence and sign
  class. Reinforcing does not mean divergence; balancing does not mean
  stability; nothing about stability/convergence/divergence is proved.
  `feedback_without_damping_story` fires only when `loop_class: reinforcing`
  and no declared `loop_class: balancing` feedback shares at least one
  variable with it. Damping is never inferred from constraints, bounds, or
  natural-language rationale; the finding does not mean divergence, and a
  balancing loop sharing no variable is not treated as damping.
- No unbounded path enumeration: SCCs are condensed, and the output reports
  earliest/latest first-pass windows, each cycle's min/max single-lap lag, an
  upper bound on repetitions within the horizon, and the fact that the effect
  is recurrent.

## 7. Typed Causal IR

`CausalModel` is an authority surface separate from `KernelModel`, owned by
the analysis layer (`fsl-tools`), never by runtime/solver/verifier.

```rust
struct CausalModel {
    name: SymbolPath,
    timebase: Timebase,
    horizon: u64,
    scope_vocabulary: ScopeVocabulary,
    clocks: BTreeMap<ClockId, ClockMapping>,
    sources: Vec<SourceRef>,
    variables: BTreeMap<VariableId, CausalVariable>,
    claims: BTreeMap<ClaimId, CausalClaim>,
    feedbacks: BTreeMap<FeedbackId, FeedbackLoop>,
    evidence_refs: BTreeMap<EvidenceId, EvidenceRef>,
    expectations: BTreeMap<ExpectationId, FormalExpectation>,
    annotations: Annotations,
}

struct CausalVariable {
    role: VariableRole,
    measurement: MeasurementBinding,
    scope: ScopeTokens,
    requirements: Vec<RequirementRef>,
    origin: Origin,
}

struct CausalClaim {
    source: VariableId,
    target: VariableId,
    content_version: u64,
    lifecycle: ClaimLifecycle,
    polarity: Polarity,
    lag: KnownOrUnknown<Interval<u64>>,
    persistence: Persistence,
    basis: ClaimBasis,
    evidence: Vec<EvidenceId>,
    scope: ScopeTokens,
    requirements: Vec<RequirementRef>,
    origin: Origin,
}
```

IR invariants:

- Claim, variable, and evidence IDs are unique; source/target are resolved;
  claim versions are positive integers; evidence references resolve down to
  the pinned version.
- Scope-vocabulary containment/overlap/disjoint relations are closed within a
  dimension and contradictions after closure are rejected. Clock mappings
  reference only imported kernel specs and positive-integer ratios.
- Retired claims are excluded from current analysis but never lose stable ID,
  version, or evidence provenance.
- Intervals are non-negative with `min <= max`.
- Source-file bindings are typed and resolved; causal origin and requirement
  traceability are preserved.
- Causal claims are never implicitly converted to kernel properties.

## 8. JSON contracts

Versioned schemas live under `schemas/fslc/causal/`.

check (`causal-check.v0`):

```json
{
  "result": "causal_model_checked",
  "schema_version": "causal-check.v0",
  "formal_result": "not_run",
  "model": "SubscriptionRetention",
  "variables_checked": 7,
  "claims_checked": 8,
  "feedbacks_checked": 1,
  "warnings": [],
  "do_not_assume": [
    "The causal claims are true",
    "The graph is causally complete"
  ]
}
```

analyze finding (every finding carries `formal_status: "not_a_violation"`):

```json
{
  "finding_id": "causal-finding:high-leverage:C_Habit_Retention",
  "analysis": "causal_structure",
  "finding_type": "high_leverage_untested_claim",
  "severity": "review_required",
  "confidence": 0.92,
  "formal_status": "not_a_violation",
  "involved_nodes": ["claim:C_Habit_Retention", "variable:habit_formation", "variable:retention_90d"],
  "witness": {
    "dependent_outcomes": ["variable:retention_90d"],
    "dependent_paths": 4,
    "evidence_count": 0
  },
  "why_it_matters": "Multiple long-horizon outcomes depend on one unvalidated causal claim.",
  "candidate_repairs": [
    {"kind": "add_evidence_plan"},
    {"kind": "add_alternative_path"},
    {"kind": "downgrade_strategy_confidence"}
  ],
  "do_not_assume": ["The claim is false", "The downstream outcomes will not occur"]
}
```

`causal-graph.v0` carries composed path polarity and per-cycle `loop_class`
with claim-ID witnesses as first-class attributes (§6). The evidence artifact
schema (`fsl-causal-evidence.v0`), the lifecycle record chain, and the
deterministic support-aggregation table are defined by #322. `causal-diff.v0`
(claim identity by ID, content by version and typed fields) is defined by
#321; its support transitions stay `not_available` until #322's evidence
inputs exist.

## 9. Authority surface and algorithm constraints

Phase 1 (#321) changes: `rust/fsl-syntax`, `rust/fsl-core`, `rust/fsl-tools`,
`rust/fslc`, `schemas/fslc/causal/`, `rust/fsl-lsp` (minimal), docs / skills /
examples / tests.

Never changed by this profile: `rust/fsl-runtime`, `rust/fsl-solver*`,
`rust/fsl-verifier`, Public Kernel v1, the frozen Python reference.

- Graph storage is deterministic ID-ordered `BTreeMap` / `BTreeSet`.
- Reachability, dominators, and timelines are computed on the SCC condensation
  DAG; acyclic earliest/latest first-pass windows via interval DP.
- Inside a feedback SCC, no full walk enumeration: representative cycle, cycle
  lag range, and the within-horizon repetition upper bound are returned.
- Representative path output has an explicit cap and truncation metadata;
  reachability, earliest time, and dominators come from graph algorithms, not
  enumeration, so the cap can never change a judgment.

## 10. Resolved open decisions

1. **Canonical syntax**: standalone top-level `causal` is final. A
   business-nested `cause` sugar is not adopted; it may be revisited later
   only as pure surface sugar that expands to a standalone causal model.
2. **Timebase vocabulary**: closed enum `tick | hour | day | week` in v0
   (fixed-length units only; `week` = 7 days). Named custom units are
   rejected at check time. Any future custom-unit extension must keep the
   fail-closed `not_evaluable` path for unconvertible comparisons.
3. **`persists`**: adopted in the MVP as a required claim field (§4.3). It
   powers observation-window and cadence findings (#321); quantitative
   convolution stays out of scope.
4. **Source import**: `uses <alias> from "<relative-path>"` with
   alias-qualified two-segment reference paths (§4.1).
5. **Undeclared positive-lag cycles**: warning
   (`causal_unacknowledged_feedback`), not an error. Escalation to a hard
   gate is CI policy, kept separate from model correctness; instantaneous
   loops remain errors.
6. **Per-property evidence dependency**: confined to Phase 2 (#322). Phase 1
   only records evidence *references* (ID uniqueness, resolvability of claim
   → evidence links); it does not open, parse, or validate evidence artifact
   files, and emits no support values.
7. **Schema independence**: causal schemas stay under `schemas/fslc/causal/`
   indefinitely and are not merged into a future Public Kernel v2 without a
   separate accepted design.
8. **Moderation**: `moderated_by` is *not* in v0. It is documented as
   reserved future work; until a follow-up design accepts it, context
   dependence of opposing polarities stays uncaptured and the token has no
   grammar. If adopted later it stays review-only (a warning that opposing
   polarity may be context-dependent), with no effect-size estimation,
   per-context polarity resolution, or identifiability analysis.

## 11. Phase map and non-goals

| Phase | Issue | Deliverable |
|---|---|---|
| 0 | #319 | spike / dogfood models, go decision |
| 1 | #321 | `fslc causal check/analyze/diff`, typed IR, `causal-graph.v0` |
| 2 | #322 | evidence artifacts, lifecycle chains, per-claim support overlay |
| 3 | #323 | explicit `expectation` lowering to existing kernel properties |
| 4 | #360 | production replay → observational evidence artifact bridge |
| 5 | #364 | validation-plan artifacts and portfolio ledger |

Non-goals across all phases: proving real-world causality; causal discovery;
do-calculus/identifiability; effect-size estimation; converting temporal
co-occurrence into causal support; letting external tools overwrite
`causal_support`.

## 12. Phase 1 implementation notes (fixed by #321)

Deterministic details settled during the Phase 1 implementation; they refine,
never contradict, the sections above.

- **Variable fields `deadline N` and `window a..b`** (both optional, timebase
  units, measured from the intervention change): `deadline` on an outcome
  feeds `deadline_before_earliest_effect`; `window` on an observed variable
  feeds `observation_window_misses_effect` (a window that ends before an
  arriving claim's `lag.min` or starts after `lag.max + persists.max` cannot
  overlap the response).
- **Measurement kinds in v0** are `action` (via `binds`), `kpi`, `state`, and
  `property`. Typed-predicate bindings are deferred to a follow-up design.
- **Diagnostic kinds added for well-formedness**: `causal_invalid_model`
  (missing/duplicate timebase, horizon, or required claim field; invalid
  enum values; clock unit ≠ timebase) and `causal_scope_invalid` (undeclared
  dimension/token, contradictory or cyclic relations, duplicate dimension in a
  selection). Both are errors; analysis never starts on them.
- **Unacknowledged-cycle rule** (warning `causal_unacknowledged_feedback` and
  finding `unacknowledged_feedback_loop`): computed at edge granularity on the
  SCC condensation — any active claim inside a nontrivial SCC that is not part
  of a declared feedback's claim list triggers one warning per SCC. No cycle
  enumeration.
- **Timeline bounds**: `first_pass.min` is exact (Dijkstra over `lag.min`,
  known-lag edges only). `first_pass.max` is exact on acyclic routes; when the
  pair is connected through a nontrivial SCC (`via_feedback: true`), it is an
  upper bound that charges the SCC with the sum of its internal known
  `lag.max` values. A reachable pair with no fully-known-lag path reports
  `first_pass: "unknown"` plus the `unknown_lag_blocks_timeline` finding.
- **Deterministic thresholds** (integer arithmetic, documented in the finding
  witness): `long_horizon_without_leading_indicator` fires when
  `earliest * 2 >= horizon` and no observable path variable has
  `earliest * 4 <= horizon`; indicator classes compare against the largest
  outcome earliest `E` — `leading` when `3 * earliest <= E`, `lagging` when
  `3 * earliest >= 2 * E`, else `intermediate`.
- **`high_leverage_untested_claim`** requires the evidence-free claim to be a
  cut (removal disconnects) for at least two distinct outcomes;
  `single_hypothesis_bottleneck` requires one claim or mediator to be a cut
  for every outcome reachable from an intervention.
- **CLI routing**: `fslc check` on a causal document routes to the causal
  checker via the pre-dispatch `is_causal_source` sniff (the same mechanism as
  legacy AI project files); `causal` is deliberately not in the dialect
  registry, so the frozen Python `DIALECT_KEYWORDS` parity gate does not move.
  LSP diagnostics and the document index apply the same sniff.
- **Worker waiver**: the browser Worker exposes only `check`/`verify` by
  standing policy; causal commands are CLI-only and fall through to the
  Worker's deny-by-default error. This is an explicit waiver, not an omission.
- **Phase 1 evidence handling**: `evidence <Id> from "<path>"` declarations
  are recorded and referenced by ID; the artifact file is never opened, parsed,
  or validated (that is #322), and no support values are emitted anywhere.

## 13. Phase 2 implementation notes (fixed by #322)

- **Digest conventions.** `artifact_digest` = `sha256:` over the canonical
  JSON (recursively sorted keys, compact separators) of the artifact with the
  `artifact_digest` field removed. A lifecycle `record_digest` = the same
  canonicalization of the record with `record_digest` removed plus the
  chain's `evidence_id` and `artifact_digest` injected. Tampering with either
  is a fatal `causal_evidence_digest_mismatch` /
  `causal_evidence_lifecycle_mismatch`.
- **Fail-closed boundary.** Artifact schema violations (unknown design/
  support/observation fields, non-`not_run` `formal_result`), digest
  mismatches, and lifecycle-chain violations (sequence gaps/forks, broken
  digest links, records after a terminal status, unresolvable
  `superseded_by`) stop the analysis with exit 2. Applicability conditions
  (version pin, scope, freshness, lifecycle status, window-vs-lag, lineage)
  are warnings: the artifact stays in history and in the graph but is
  excluded from current support.
- **Staleness needs `--as-of`.** `stale_evidence` compares `valid_until`
  against an explicit `--as-of YYYY-MM-DD` only; without `--as-of`, a
  declared `valid_until` is accepted as-declared (never the wall clock, so
  identical inputs always produce identical output). Missing `valid_until`
  is `unknown_freshness` and excludes the artifact regardless.
- **Window conversion.** `period.end - period.start` converts to timebase
  units from ISO-8601 date differences: exact for `day` and `hour`
  (days × 24); `week` only when the difference is a whole number of weeks;
  `tick` and fractional conversions are `not_evaluable` (never rounded).
  `w < lag_min` excludes with `evidence_window_shorter_than_lag`;
  `w == lag_min` passes.
- **Lineage.** The lineage root is the transitive `derived_from` root when
  present, else `source_study_id`, else the artifact itself. Roots sharing
  members emit `duplicate_evidence_source`; one root is one vote; support
  contradictions inside a root collapse to `inconclusive` with
  `conflicting_evidence`.
- **Support vocabulary.** Aggregation follows the #322 table verbatim;
  `unsupported_by_current_evidence` applies when a claim is referenced by
  artifacts (or declares evidence references) but no artifact is currently
  applicable. `causal_support` never appears without an adjacent
  `formal_assurance: "not_run"` in graph output.
- **CLI surface.** `--evidence` and `--lifecycle` are repeatable;
  `--projection causal_evidence_graph` requires at least one `--evidence`;
  `--profile causal-review` with `--evidence` appends evidence findings and
  a `causal_support` map to the review envelope. Measurement findings
  (`unobserved_mediator`, `proxy_only_critical_variable`,
  `unsupported_assumption_chain`) are model-structural and fire without
  evidence inputs.

## 14. Phase 3 implementation notes (fixed by #323)

- **Syntax.** `expectation <Id> { trigger action alias.name | trigger
  predicate alias { <expr> }  response predicate alias { <expr> }  within N
  clock <name>  derived_from_claim <Id> }`. Inline predicate blocks capture
  raw kernel-expression source; they parse with the ordinary expression
  grammar and type-check against the target spec's state space when the
  augmented model builds — anything else (KPI deltas, averages, effect
  sizes) fails closed there. The legacy field names `supports` /
  `supports_claim` are parse errors.
- **Action triggers** lower to a one-step pulse ghost
  (`_expectation_fired_<id>: Bool`, init false, set true by the trigger
  action and false by every other action), so the generated
  `leadsTo pulse ~> within ticks response` reads "within N ticks of the
  action firing". Guards are untouched — enabledness and deadlock behavior
  do not change; the ghost only widens the state space by one Bool.
- **Fail-closed gates** (all `causal_expectation_invalid`): missing
  trigger/response/within/clock, unknown clock, trigger or response alias ≠
  the clock's kernel spec, unresolved `derived_from_claim`, a target that is
  not a plain kernel spec (dialect-lowered state spaces are not stable
  expectation surfaces in v0), a trigger action absent from the spec, and a
  `within` that does not convert to an exact integer tick count
  (`within × ticks ÷ units` must divide evenly; nothing is rounded).
- **Verdict boundary.** `fslc causal verify-expectations` runs the ordinary
  bounded verifier per expectation. A violation of the *generated* property
  is `verdict: "violated"`; a violation of any pre-existing property is an
  error ("fix the spec first") so expectation verdicts never absorb baseline
  bugs. Both verdicts leave every claim at `formal_assurance: "not_run"`
  with untouched `causal_support` (`causal-expectations.v0`, held by pass
  and violated goldens). There is still no `causal verify` alias.

## 15. Test obligations for Phase 1

The scope three-valued comparison (§4.4), polarity sign product with unknown
absorption (§6), and `reinforcing / balancing / unknown` loop classification
(§6) are normative here and must land with deterministic, witness-carrying
tests in #321. Claim version pinning and the retired lifecycle (§4.5) must be
covered so that old-version evidence never supports a current claim and
refuted-claim history is never lost. Clock-mapping syntax, placement, and
integer-conversion fail-closed conditions (§5) are shared verbatim with #323.

## 16. Phase 4 implementation notes (fixed by #360)

- **Observation bridge architecture.** `fslc causal observe-expectations`
  replays compiled expectations against a production JSONL log using the
  solver-free `BoundedLivenessMonitor` from `fsl-runtime`. For each compiled
  expectation, one `BoundedLivenessMonitor` is built from the augmented
  `KernelModel` (carrying the generated `leadsTo`). The monitor is fed the
  log's mapped observed state — extended with the pulse ghost for action
  triggers — at each step. Pass or violated verdicts carry
  `assurance: "replay-observed"` and never change `formal_assurance` or
  `causal_support` on any claim.
- **Ghost state extension.** For action-trigger expectations, the pulse ghost
  `_expectation_fired_<id>` is computed per log record as
  `mapped_action == trigger_action`; it is not part of the production state.
  For predicate-trigger expectations, no ghost is needed — the trigger
  expression evaluates directly against the mapped observed state.
- **`--trace` / `--from-log` equivalence (AC 7).** The existing `fslc replay
  --trace` (schema 1.2) feeds the `Monitor`'s spec-computed state to
  `BoundedLivenessMonitor`. The observation bridge feeds the log's mapped
  observed state instead. For conformant logs (observed state ≡ spec-computed
  state) the two paths produce identical verdicts. Conformance is checked as
  a precondition — a nonconformant log aborts evidence generation.
- **Fail-closed conditions.** Missing `--scope`, `--period-start`,
  `--period-end`, `--from-log`, or `--mapping` is a CLI error (exit 2).
  A nonconformant log record (action not enabled, state mismatch) aborts
  with `observation_replay_nonconformant`. A mapping failure (unknown
  action, parameter mismatch, missing state variable) aborts with
  `observation_replay_failed`. All failure outputs carry `do_not_assume`.
- **Evidence artifacts.** Per-expectation `fsl-causal-evidence.v0` artifacts
  with the `observation` object filled in: `design: "observational"`,
  `support: "inconclusive"`, `observation.kind: "expectation_replay"`,
  `observation.assurance: "replay-observed"`. The schema's `if/then` gate
  rejects any artifact that sets `observation` but declares `support` other
  than `"inconclusive"` or `design` other than `"observational"` (AC 3/4).
  `observation.digests` carries sha256 digests of the causal source, JSONL
  log, and mapping file. Lifecycle records are sequence-1 active chains.
- **File output.** `--out` and `--lifecycle-out` write one file per artifact
  (individually consumable by `fslc causal analyze --evidence`). Multiple
  expectations produce files with the evidence ID embedded in the filename.
- **`do_not_assume`.** Every envelope and per-expectation result includes the
  five-item array specified in #360: "The causal claim is proved", "Temporal
  co-occurrence establishes causality", "No unmodeled common cause exists",
  "Expectation violation refutes the causal claim", "Unobserved behavior did
  not occur".
- **CLI envelope.** `causal-observation.v0` schema (inventory now 36);
  `result: "causal_expectations_observed"`, `formal_result: "not_run"`.
- **Worker waiver.** Same policy as other causal commands: CLI-only.

## 17. Phase 5 implementation notes (fixed by #364)

- **Validation plan artifacts.** `fsl-causal-validation-plan.v0` is an
  immutable JSON artifact pinning claim IDs + content versions, a study
  `design` (same closed vocabulary as evidence), scope tokens, an
  observation window (timebase + minimum), measurement variable references,
  opaque `external_refs`, and a canonical `artifact_digest`. Plan lifecycle
  reuses the existing `fsl-causal-evidence-lifecycle.v0` chain schema —
  the `evidence_id` field carries the `plan_id`.
- **Plan validator.** `parse_plan()` in `fsl-tools/src/causal_plan.rs`
  validates schema version, claim pins, design vocabulary, scope tokens,
  observation window, measurements, and artifact digest (fail-closed on
  mismatch). Lifecycle validation uses the existing
  `validate_lifecycle_chain()`.
- **Ledger projection.** `fslc causal ledger model.fsl [--plans ...
  --evidence ... --lifecycle ... --as-of YYYY-MM-DD]` integrates the
  model's claims with plans, evidence (via the existing
  `aggregate_support()`), and per-claim attention reasons into a
  `causal-ledger.v0` JSON envelope. Every active claim appears regardless
  of plan/evidence availability (AC 1). Retired claims appear but have no
  attention reasons. Output is claim-ID-ordered and byte-stable (AC 5).
- **Attention reasons (closed vocabulary).** 12 deterministic attention
  reasons derived from facts: `validation_plan_missing`,
  `validation_plan_version_mismatch`, `validation_plan_scope_unassessable`,
  `validation_plan_scope_inapplicable`,
  `validation_window_shorter_than_lag`,
  `required_measurement_unavailable`, `current_evidence_missing`,
  `current_evidence_inconclusive`,
  `conflicting_evidence_requires_decision`,
  `challenging_evidence_requires_decision`,
  `evidence_freshness_requires_refresh`,
  `observation_not_directional_support`. Each carries a typed witness.
  Attention reasons are review projections, not formal violations (AC 7).
- **Plan scope checking.** The existing 3-valued scope comparison
  (`compare_scope`, extracted from `scope_application`) is reused for
  plan–claim scope assessment with the same `subsumes / partial_overlap /
  disjoint / unassessable` semantics.
- **`do_not_assume`.** Every ledger output includes: "The causal claim is
  proved or verified", "A completed validation plan establishes causality",
  "No unmodeled common cause exists", "Absence of challenging evidence
  means the claim is true", "Portfolio readiness is a formal assurance
  class".
- **Schemas.** `fsl-causal-validation-plan.v0` and `causal-ledger.v0`
  (inventory now 38).
- **Worker waiver.** Same policy as other causal commands: CLI-only.
