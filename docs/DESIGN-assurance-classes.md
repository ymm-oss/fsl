# FSL — Assurance classes (issue #171)

## Goal

fslc already separates the formally verified slice from external evidence
(`formal_result: "not_run"` in fsl-ai eval/regress/compare/drift, fsl-ai replay,
fsl-domain runtime replay, fsl-db observe). This design fixes **one shared
vocabulary** for that separation and surfaces it **per requirement id** in
`fslc ledger` and with the same labels in `fslc html`, so a PM/auditor can read
"how far is this requirement actually proven" without decoding per-dialect
result fields. Presentation/aggregation only: no new verification, no change to
the JSON envelope or exit codes.

## Vocabulary

Five classes, strongest first. The token is the canonical machine string; the
label is what ledger/html render.

| Token | Label | Evidence form |
|---|---|---|
| `proved` | `proved(induction)` | k-induction / ranking proof, `completeness:"unbounded"` — all depths |
| `bounded` | `bounded(BMC depth k)` | BMC exploration, `completeness:"bounded"`, `checked_to_depth: k` |
| `replay-observed` | `replay-observed` | concrete log/trace/telemetry checked against the spec (Monitor replay, ai replay, db observe, domain replay, ai drift) |
| `statistical` | `statistical(Wilson c%)` | Wilson interval over precomputed Bernoulli eval records (ai eval / regress) |
| `not_run` | `not_run` | no completed evidence of any stronger form |

**Class is method coverage, verdict is outcome — they are orthogonal.** A
`violated` BMC run is still class `bounded` (all executions to depth k were
examined; one definitively fails); the failure surfaces in the ledger 状態
column and findings, never by silently downgrading the class. A statistical
gate failure that produced **no interval** (`dataset_invalid`,
`evaluator_untrusted`, `insufficient_samples`, `slice_missing`, `inconclusive`)
is `not_run` — there is no bound to point at — while
`statistically_unsupported` (interval computed, threshold missed) stays
`statistical` with a failing verdict.

## Classification table (what each class does / does not guarantee)

| Class | Guarantees | Explicitly does NOT guarantee |
|---|---|---|
| `proved` | the property holds on **every** execution of the written spec, any depth (k-induction; ranked leadsTo via ranking proof) | fidelity of the spec to real intent; anything about the implementation; dialect assumptions (`verified_under_assumptions` lists them — finite windows, complete capability declarations) still apply |
| `bounded` | **every** execution up to `checked_to_depth` was examined; no counterexample within it (or the reported one is real) | anything beyond depth k; termination/liveness past the bound; implementation behavior |
| `replay-observed` | the **specific traces observed** conform (or definitively deviate) step-by-step; deviations are real findings | unobserved behavior — absence from logs is not absence of behavior (`*-ASSUME-OBSERVABILITY-COVERAGE`); no universal or statistical claim |
| `statistical` | a Wilson bound at the stated confidence supports (or fails) the threshold over the **fixed evaluated dataset** | per-case correctness; out-of-distribution inputs; sample independence and evaluator calibration (assumed via `AI-ASSUME-*`, not proved); never displayable as `proved`/`verified` (DESIGN-stochastic) |
| `not_run` | nothing — structural analysis, profiles, comparisons, or errored/gated runs carry no requirement-level guarantee | everything; `agent_analyzed`/`ai_project_analyzed`/`compat_profile_generated`/`compared` are useful artifacts but not requirement evidence |

## Shared classifier — `src/fslc/assurance.py` (new module)

One classifier, two consumers (`ledger.py`, `html_report.py`); neither may
re-derive classes locally.

- `classify_result(result: dict) -> str` — one command's result dict (envelope
  or bare) to a token. Ordered rules, first match wins:
  1. formal evidence present (`completeness` / `kernel.completeness` /
     `formal_result` in kernel vocabulary): `"unbounded"` or `proved` →
     `proved`; `"bounded"` or `verified/violated/reachable_failed/unknown_cti`
     → `bounded`.
  2. observation markers (`guarantee_kind:"runtime_observed"`,
     `evidence.kind` in `runtime_replay`/`runtime_telemetry`, or result in
     `conformant`/`nonconformant`/`replay_*`/`observed_*`/
     `conformance_checked`/`evidence_supported`/`evidence_failed`) →
     `replay-observed`.
  3. statistical markers (schema `fsl-ai-statistical-result.v0` /
     `fsl-ai-migration-result.v0` with status
     `statistically_supported|statistically_unsupported`) → `statistical`;
     gate statuses (see above) → `not_run`.
  4. else → `not_run`.
- `classify_source(result: dict) -> dict` —
  `{"assurance", "verdict": "pass"|"fail"|"none", "under_assumptions": bool,
  "label", "detail"}`; `under_assumptions` is true for dialect wrappers
  (`verified_under_assumptions`).
- `classify_element(group, name, verification) -> str` — per spec element.
  Under BMC everything is `bounded`. Under `result:"proved"`: `invariants`,
  `transitions` → `proved`; a `leadstos` entry → `proved` iff
  `verification["leads_to"][name]["completeness"] == "unbounded"` (ranked),
  else `bounded`; `reachables` and action coverage → `bounded` (base BMC only —
  `prove()` already notes this). `result:"error"` without `completeness` →
  `not_run`.
- `strongest(classes) -> str`, `ASSURANCE_ORDER`, `assurance_label(token, *,
  depth=None, confidence=None, steps=None, under_assumptions=False) -> str`.
- `requirement_assurance(registry, verification, replay_result=None,
  evidence_results=()) -> dict` — `{req_id: {"assurance", "sources": [...],
  "under_assumptions"}}`.

### Producer → class map (the acceptance-criteria table)

| Producer | Result fields | Class |
|---|---|---|
| `fslc verify` (BMC) | `completeness:"bounded"` | `bounded` |
| `fslc verify --engine induction` | `proved`, `completeness:"unbounded"` | `proved` (per-element nuance above) |
| induction `unknown_cti` | `completeness:"bounded"` | `bounded` |
| `fslc ai check` (component) / `fslc db check` / `fslc domain check` | `verified_under_assumptions` + `kernel.completeness` | `proved`/`bounded` + under-assumptions marker |
| `fslc replay` (kernel Monitor) | `conformant`/`nonconformant` | `replay-observed` |
| `fslc ai replay` | `replay_conformant`/`replay_nonconformant`, `formal_result:"not_run"` | `replay-observed` |
| `fslc db observe` | `observed_conformant`/`observed_mismatch`, `formal_result:"not_run"` | `replay-observed` |
| `fslc domain replay` | `conformance_checked`/`nonconformant`, `guarantee_kind:"runtime_observed"` (no `formal_result` field) | `replay-observed` |
| `fslc ai drift` | `observed_supported`/`observed_mismatch`, `formal_result:"not_run"` | `replay-observed` (telemetry observation, not a Wilson claim) |
| `fslc ai eval` / `fslc ai regress` | `statistically_supported`/`statistically_unsupported`, `formal_result:"not_run"` | `statistical` |
| `fslc ai eval` gate failures | `dataset_invalid` 等 | `not_run` |
| `fslc ai compare` | `compared` (no threshold claim) | `not_run` |
| `fslc ai check` (agent / project) | `agent_analyzed` / `ai_project_analyzed` | `not_run` |
| `fslc ai compat` | `compat_profile_generated` | `not_run` |
| db preservation/engine evidence artifacts | `evidence_supported`/`evidence_failed`, `formal_result:"not_run"` | `replay-observed` |

## Per-requirement aggregation

- Kernel verification attributes to every registry requirement via the
  element groups it tags (`_requirement_registry` gains
  `elements: {group: [names]}`); the requirement's formal class is the
  **weakest** of its elements' classes (a requirement is only as proven as its
  least-proven element).
- External evidence (`--evidence`, below) attributes to a requirement only via
  an explicit `requirements: [ids]` top-level list or `requirement.id` inside
  findings/checks; otherwise it is spec-level (external-evidence section, not a
  row upgrade). fsl-ai/db results do not carry kernel requirement ids today —
  no name-matching heuristics.
- **Multiple sources: primary = strongest completed source in issue order
  (`proved > bounded > replay-observed > statistical > not_run`); all sources
  are always listed.** The order is a display precedence, not subsumption —
  replay and statistical evidence are incomparable in kind (exact-but-anecdotal
  vs quantified-but-aggregate), so neither is hidden by the other; the primary
  answers the audit question "how much is formally proven", and weaker sources
  still document model fidelity and quality coverage.
- Weaker evidence never upgrades a class; a failing source never lowers the
  class of an independently proven requirement — it adds a 要確認 finding.

## `fslc ledger` changes

- CLI: `--engine {bmc,induction}` (default `bmc`; without it a ledger can never
  show `proved`) and repeatable `--evidence <result.json>` (a saved stdout
  envelope of any producer above). `run_ledger(file, depth=8, output=None,
  deadlock_mode="ignore", impl_log=None, write_file=True, engine="bmc",
  evidence=None)`; `render_ledger(..., evidence_results=None)`.
- Header: a legend line under 保証限界 naming the five labels and pointing at
  this document.
- リスク一覧 gains a 保証クラス column after 状態:
  `| 要件ID | 業務目的 | 状態 | 保証クラス | 検出種別 | リスク | 判断者 | 次アクション |`
  e.g. `bounded(BMC depth 8)`, `proved(induction)※前提付き`,
  `bounded(BMC depth 8) + statistical(Wilson 95%)`.
- 要件ID別詳細 gains one `- 保証クラス:` line listing every source with verdict.
- New `## 外部エビデンス` section (only when `--evidence` given): file, producer
  schema, class, verdict, target component — spec-level evidence lives here.
- Envelope unchanged (`result:"generated"`, `kind:"audit_ledger"`).

## `fslc html` changes

- `_status_section` adds an "Assurance" row: `assurance_label(classify_result
  (verification), ...)` — same labels as the ledger.
- `_properties_section(properties, auto_checks, verification)` gains an
  Assurance column per property row via `classify_element` (kind→group:
  invariant→invariants, leadsTo→leadstos, reachable→reachables,
  trans→transitions).
- `fslc html --engine {bmc,induction}` for parity, so a proved report can
  render `proved(induction)`.

## Non-goals

- No automatic requirement-id inference for external evidence (explicit
  annotation only).
- No family-wise statistical aggregation across evidence files
  (DESIGN-stochastic already disclaims it).
- No change to any producer's result schema, the JSON envelope, or exit codes;
  `formal_result:"not_run"` boundaries in DESIGN-ai-hard / DESIGN-stochastic /
  DESIGN-db stay authoritative — this layer only names them.
- Hollowness (mutate kill-rate) is still not an assurance dimension
  (DESIGN-ledger non-goal stands); a `proved` hollow invariant is proved and
  hollow.
