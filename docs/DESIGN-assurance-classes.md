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
`statistical` with a failing verdict. The classifier matches the status token
itself; it does not inspect whether an interval was produced.

## Classification table (what each class does / does not guarantee)

| Class | Guarantees | Explicitly does NOT guarantee |
|---|---|---|
| `proved` | the property holds on **every** execution of the written spec, any depth (k-induction; ranked leadsTo via ranking proof) | fidelity of the spec to real intent; anything about the implementation; dialect assumptions (`verified_under_assumptions` lists them — finite windows, complete capability declarations) still apply |
| `bounded` | **every** execution up to `checked_to_depth` was examined; no counterexample within it (or the reported one is real) | anything beyond depth k; termination/liveness past the bound; implementation behavior |
| `replay-observed` | the **specific traces observed** conform (or definitively deviate) step-by-step; deviations are real findings | unobserved behavior — absence from logs is not absence of behavior (`*-ASSUME-OBSERVABILITY-COVERAGE`); no universal or statistical claim |
| `statistical` | a Wilson bound at the stated confidence supports (or fails) the threshold over the **fixed evaluated dataset** | per-case correctness; out-of-distribution inputs; sample independence and evaluator calibration (assumed via `AI-ASSUME-*`, not proved); never displayable as `proved`/`verified` (DESIGN-stochastic) |
| `not_run` | nothing — structural analysis, profiles, comparisons, or errored/gated runs carry no requirement-level guarantee | everything; `agent_analyzed`/`ai_project_analyzed`/`compat_profile_generated`/`compared` are useful artifacts but not requirement evidence |

## Shared classifier — native Rust (`rust/fsl-tools`)

Per `AGENTS.md`, the **authoritative** assurance vocabulary lives in the native
Rust workspace. `fslc ledger`, `fslc html`, and `fslc document` all consume the
same rules from `rust/fsl-tools/src/ledger.rs` (and, for document evidence
overlay, `rust/fsl-tools/src/document_evidence.rs`); neither command surface
may re-derive classes locally.

The frozen Python reference (`src/fslc/assurance.py`) is listed in
[Frozen Python reference](#frozen-python-reference-srcfslcassurancepy) below for
parity inspection only — it is **not** the contract surface.

### Envelope classifier — `ledger::assurance_token`

`pub(crate) fn assurance_token(value: &Value) -> &'static str`
(`rust/fsl-tools/src/ledger.rs`) — one command's result envelope (or bare JSON
object) to a token. Ordered rules, first match wins:

1. formal completeness (`completeness` or nested `kernel.completeness`):
   `"unbounded"` → `proved`; `"bounded"` → `bounded`. Keys such as
   `formal_result` and the top-level `result` token are **not** read for
   this step — e.g. `{"result":"proved",...}` without `completeness` does
   not upgrade assurance class.
2. observation markers (`guarantee_kind:"runtime_observed"`,
   `evidence.kind` in `runtime_replay`/`runtime_telemetry`, or `result` in
   `conformant`/`nonconformant`/`replay_conformant`/`replay_nonconformant`/
   `observed_conformant`/`observed_mismatch`/`observed_supported`/
   `conformance_checked`/`evidence_supported`/`evidence_failed`) →
   `replay-observed`.
3. statistical: `status` (when absent, fall back to `result`) in
   `statistically_supported`/`statistically_unsupported` → `statistical`;
   gate statuses such as `dataset_invalid`/`evaluator_untrusted`/
   `insufficient_samples`/`slice_missing`/`inconclusive` fall through to
   step 4. Schema names such as `fsl-ai-statistical-result.v0` are not
   consulted.
4. else → `not_run`.

### Verdict mapping — `ledger::evidence_verdict` (issue #508)

`fn evidence_verdict(value: &Value) -> Option<bool>`
(`rust/fsl-tools/src/ledger.rs`, module-private) — whether an evidence
envelope's own `result`/`status` token is a definitive pass, a definitive
fail, or carries no verdict at all. Deliberately independent of
[`assurance_token`](#envelope-classifier--ledgerassurance_token): class
(method strength) and verdict (outcome) are orthogonal, so a failing source
must never change the assurance label, only add a finding.

- `Some(true)` on `result` `conformant`/`replay_conformant`/
  `observed_conformant`/`conformance_checked`/`observed_supported`/
  `evidence_supported`, or on `status` `statistically_supported`.
- `Some(false)` on `result` `nonconformant`/`replay_nonconformant`/
  `observed_mismatch`/`evidence_failed`, or on `status`
  `statistically_unsupported`.
- `None` when the envelope carries no verdict (gate failures like
  `dataset_invalid`, structural output like `compared`). It does **not**
  return a dict.

### Per-element classifier — `ledger::formal_assurance`

`pub(crate) fn formal_assurance(group: &str, name: &str, verification: &Value) -> &'static str`
(`rust/fsl-tools/src/ledger.rs`) — per spec element against a `verify`/`prove`
result. Under `result:"proved"`: `invariants`/`transitions` → `proved`; a
`leadstos` entry → `proved` iff
`verification["leads_to"][name]["completeness"] == "unbounded"` (ranked),
else `bounded`; `reachables` and action coverage → `bounded` (base BMC only).
When `result` is not `"proved"`: if `result == "error"` → `not_run`; else if
`completeness == "unbounded"` → `proved`; else → `bounded`.

**Rust vs frozen Python — evaluation order (#995, unadjudicated).** The frozen
Python `classify_element` checks `completeness` **before** `result == "error"`.
Rust `formal_assurance` returns `not_run` on `result == "error"` **before**
reading `completeness`. Neither side is declared authoritative here; issue
#995 records the defect. Until it is adjudicated, do not treat either
implementation alone as the contract for this corner:

| Input | Rust `formal_assurance` | Python `classify_element` / prior doc |
|---|---|---|
| `result:"error"` + `completeness:"bounded"` | `not_run` | `bounded` |
| `result:"error"` + `completeness:"unbounded"` | `not_run` | `proved` |

### Display label — `ledger::assurance_label`

`pub(crate) fn assurance_label(token: &str, depth: Option<u64>) -> String`
(`rust/fsl-tools/src/ledger.rs`) — `"bounded"` + `Some(k)` →
`"bounded(BMC depth k)"`; `"proved"` → `"proved(induction)"`; other tokens
pass through unchanged.

### Requirement-id matching — `ledger::evidence_requirement_ids`

`pub(crate) fn evidence_requirement_ids(item: &Value) -> Vec<&str>`
(`rust/fsl-tools/src/ledger.rs`) — every requirement ID an evidence envelope
declares at its root (`requirements` string array plus singular
`requirement.id`). Shared with `document_evidence.rs` so claim-level evidence
matching cannot diverge from ledger. Ledger additionally walks nested
`findings`/`checks` via private `evidence_attached_requirement_ids`.

### Ledger row aggregation — `ledger::assurance_cell` (private)

`fn assurance_cell(...)` (`rust/fsl-tools/src/ledger.rs`) — combines
`formal_assurance` over a requirement's registry elements with
`assurance_token` on attached `--evidence` envelopes, then renders joined
`assurance_label` strings. Precedence order is fixed:
`proved > bounded > replay-observed > statistical > not_run` (display only,
not subsumption).

### Document evidence overlay — `document_evidence::requirement_assurance`

`pub fn requirement_assurance(requirement_id: &str, evidence: &[(String, Value)]) -> RequirementAssurance`
(`rust/fsl-tools/src/document_evidence.rs`) — classifies **external evidence
only** for one requirement ID, routing matched envelopes into three fixed
dimensions (`formal`, `conformance`, `statistical`) by `assurance_token`.
Performs no live verify pass and no formal-verification aggregation; that
remains `fslc ledger`'s job. Companion: `unmatched_evidence_paths` flags
evidence files that name requirement IDs not present in the spec.

### Frozen Python reference (`src/fslc/assurance.py`)

Observable parity reference only (AGENTS.md evidence order, rank 3). Approximate
name map:

| Rust (authoritative) | Frozen Python |
|---|---|
| `assurance_token` | `classify_result` |
| `formal_assurance` | `classify_element` (see #995 table above) |
| `evidence_verdict` | *(no equivalent `def`)* |
| `assurance_label(token, depth)` | `assurance_label(token, *, depth, confidence, under_assumptions)` |
| `document_evidence::requirement_assurance` | `requirement_assurance(registry, verification, evidence_results)` — different signature and responsibility |
| `assurance_cell` precedence | `strongest` / `weakest` / `ASSURANCE_ORDER` |
| — | `confidence_of` (Wilson confidence extraction; no Rust counterpart in this module) |

### External evidence — classification without verification (issue #990)

The shared classifier is a **display mapper**: it reads fields already present
in a JSON dict and renders assurance labels. It is **not** a trust boundary
for external evidence files.

Both evidence-loading paths — `fslc ledger`'s inline read and the one
`fslc document` uses — reject unreadable paths, non-UTF-8 bytes, invalid
JSON, and non-object envelopes, and neither applies a schema, version, or
producer check beyond that.

- **Completeness-only upgrade.** `completeness` / `kernel.completeness` alone
  can classify as `proved` or `bounded` (e.g.
  `{"completeness":"unbounded",...}` without a matching `result` token). The
  loader and classifier do not confirm that the value came from a completed
  verification run.
- **No binding.** Classification of external evidence does not tie it to
  producer identity, target spec digest/revision, or proof artifacts. The same
  `fslc ledger` `--approval` path does compare versioned approval record
  spec/rendering digests; external evidence has no equivalent.
- **Confirmation is out of band.** Responsibility for confirming that external
  evidence is authentic and applicable rests with a **versioned public
  Adapter** (rules under design in issue #994, go/no-go pending). FSL core —
  evidence loading and this classifier — does not perform that confirmation.
- **Caller bears trust.** Treating ledger/html assurance labels as audit
  conclusions requires upstream trust decisions; the labels alone assert no
  integrity or applicability guarantee.

This section records that FSL core does not verify external evidence and does
not adopt a future trust model or Adapter contract (issue #994).

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
  envelope of any producer above). Native renderer:
  `ledger::render_ledger` / `render_ledger_with_approvals` (Rust); frozen
  Python `run_ledger` / `render_ledger` remain compatibility surfaces only.
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

- Status section adds an "Assurance" row:
  `ledger::assurance_label(ledger::assurance_token(verification), depth)` —
  same labels as the ledger.
- Property rows gain an Assurance column via `ledger::formal_assurance` per
  element (kind→group: invariant→invariants, leadsTo→leadstos,
  reachable→reachables, trans→transitions).
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
