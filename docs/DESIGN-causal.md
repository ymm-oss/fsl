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

**Authority scope.** Sections 1–11 are normative for Phase 1 (model
syntax, typed IR, check/analyze/diff). Sections 12–17 (implementation
notes per phase) are normative for their respective phases: §13 for #322
evidence schemas, lifecycle, and support aggregation; §14 for #323
expectation lowering; §16 for #360 observation replay; §17 for #364
portfolio ledger. Each phase's implementation notes refine the sections
above; they never contradict them. The observation bridge (§16) generates
`design: "observational"`, `support: "inconclusive"` evidence — it does
**not** convert temporal co-occurrence into directed causal support (the
§11 non-goal). A `pass` verdict on a generated `leadsTo` expectation
(§14) carries `assurance: "bounded"` or `"replay-observed"` but the
derived claim's `formal_assurance` stays `"not_run"` and its
`causal_support` is untouched — the label is on the generated kernel
property, not on the causal claim.

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
dimensions `subsumes` ⇒ whole `subsumes`; otherwise `partial_overlap`.

**Missing-dimension handling.** When a dimension is present in the claim
scope but absent from the evidence/plan scope (or vice versa), the
per-dimension result for that dimension is `unassessable`. When either side
has an entirely empty scope (no dimensions at all), the whole comparison is
`unassessable`. A claim with no explicit scope and no model `default_scope`
has an empty scope; a missing scope is never silently treated as universal.

When declared tokens still cannot be related from `subset_of` / `overlaps` /
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
self-referentially bump the content version. A content change without a
version bump is a model error detectable by `causal diff` (the old and new
versions hash-compare as different content under the same version number,
producing a `content_changed_without_version_bump` diff finding). Evidence
artifacts pin the claim `version`, not just the ID; an artifact pinned to a
non-current version is #322's `evidence_claim_version_mismatch` warning —
retained as historical evidence for the old version, excluded from current
support aggregation.

`basis` is version-relevant content: changing a claim's basis from
`hypothesis` to `assumption` (or vice versa) changes the claim's epistemic
meaning and must bump the content version. `basis supported` cannot be
written in source (§4.3); support is derived from evidence only.

Refuted or abandoned claims are not deleted: they become `status retired`.
**`retired` is terminal**: a retired claim cannot return to `active`. To
re-propose a hypothesis after retirement, create a new claim ID (which may
reference the retired predecessor via `superseded_by` on the old claim).
Retired claims are excluded from current reachability, timeline, support
aggregation, and portfolio attention, but remain in causal diff, the
evidence graph, and history projections. Re-proposal detection: `causal
diff` reports when a new claim shares source/target/polarity with a retired
claim under a different ID, producing a `retired_hypothesis_reproposed`
finding. Retirement itself is not a formal proof of falsity.

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
  its semantics degenerate. An edge with `lag: unknown` is treated as
  `lag.min == 0` for this check because unknown lag cannot prove a positive
  delay. A cycle composed entirely of `lag: unknown` edges is therefore
  rejected as instantaneous.
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

analyze finding (every finding carries `formal_status: "not_a_violation"`).
`confidence` is a deterministic structural score (e.g. the fraction of
reachable outcomes that depend on the flagged claim, computed from the
graph alone) — it is not a probability of causality, a Bayesian posterior,
or a statistical p-value:

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

## 12. Test obligations for Phase 1

The scope three-valued comparison (§4.4), polarity sign product with unknown
absorption (§6), and `reinforcing / balancing / unknown` loop classification
(§6) are normative here and must land with deterministic, witness-carrying
tests in #321. Claim version pinning and the retired lifecycle (§4.5) must be
covered so that old-version evidence never supports a current claim and
refuted-claim history is never lost. Clock-mapping syntax, placement, and
integer-conversion fail-closed conditions (§5) are shared verbatim with #323.
