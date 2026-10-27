# FSL — `fslc ledger` business audit ledger

## Goal

`fslc ledger <file.fsl> [-o ledger.md]` turns verifier evidence into a Markdown
**audit ledger organized by requirement id**, so a PM / development-governance /
internal-audit reader can decide **approve / reject / risk-accept per requirement
from the ledger alone** — without reading raw JSON or formulas. It is the
business-audience analogue of `fslc html` (a presentation layer over the
verifier; it introduces no second parser, evaluator, or verifier).

The reframe (issue #24): the ledger does not assert "correctness guaranteed". It
surfaces **intent drift / dead paths / forbidden paths / boundary gaps** as items
a human must confirm, and states the guarantee limit in positive form so it never
reads as "guarantees nothing".

## Inputs

- `run_verify(file, depth, deadlock)` — violations (with `requirement` +
  `trace_type` from #23), dead `reachable`s (`unreached[]`), uncovered source-backed actions,
  vacuity warnings, failed acceptance/forbidden (as errors), and the guarantee
  bound (`checked_to_depth` + `completeness`).
- Edition-migration findings render as `trace_type: migration` with a 移行候補 summary containing the code and message, without mutate advice (#1449).
- Rows containing only migration findings use status 🟡 移行候補 and risk —; rows with any non-migration finding retain 🔴 要確認 and risk 要確認, including specification-wide and approval views.
- Baseline migration findings and their evidence are retained; raw-success warning stderr remains deferred under #1449. Other non-migration warning-family classification defects are tracked by #1497, not fixed by the migration-row change.
- `run_scenarios(file, depth, deadlock)` — passing acceptance / forbidden /
  reachable confirmations per requirement.
- `--impl-log <trace.json>` (optional) → `run_replay` — implementation-log
  conformance (`conformant` / `nonconformant`). A replay **error** (missing
  file, malformed JSON, wrong-spec trace, schema-invalid trace) is not
  evidence and must not be rendered as an empty/missing implementation-log
  row: it fails the whole `ledger` command through the same standard error
  envelope and exit code as any other input error (issue #499), the same way
  `--evidence` already fails closed on an unreadable evidence file.
- The built `spec` — the requirement registry (every `requirement` id + text) and
  `control` governance (owner / severity) when present.
- `--evidence <result.json>`... (repeatable) — external producer envelopes fold
  into the assurance-class column (`docs/design/DESIGN-assurance-classes.md`), but a
  **failing** source (a definitive `nonconformant`/`replay_nonconformant`/
  `observed_mismatch`/`evidence_failed`/`statistically_unsupported` verdict,
  not a verdict-less gate failure like `dataset_invalid`) also adds a 🔴 要確認
  finding: one per requirement id it attaches to, recursively — its own root
  `requirements`/`requirement.id`, or a `requirement.id` nested inside a
  `findings`/`checks` array item — or a spec-level（仕様全体）finding when it
  fails with no requirement attribution at all (issue #508). A failing source
  never lowers or removes an independently proven requirement's assurance
  class; class (method strength) and verdict (pass/fail) stay orthogonal, so
  the finding is additive only.

## CLI contract

```
fslc ledger spec.fsl --depth 8 --impl-log run.json -o ledger.md
```

- With `-o`, writes the Markdown file and prints the JSON envelope with
  `result:"generated"`, `kind:"audit_ledger"`. Without `-o`, writes the Markdown
  to stdout (mirroring `html` / `testgen`). Parse/type/semantic/io errors use the
  standard CLI error envelope and exit codes. Defaults to `--deadlock ignore`
  (an audit focuses on intent drift, not terminal-state deadlock; run
  `verify --deadlock warn` separately for that).
- A verification `result:"error"` returns its full JSON envelope and exit 2/3
  before ancillary inputs, rendering, or output-file creation; see the
  [verdict-findings amendment](#amendment-to-1316-verdict-findings-1338-decision-2026-10-26)
  for the direct-report gate and approval-only exception.

## Structure

1. **Header** — the guarantee limit in positive form ("BMC: depth N までの全実行を
   網羅" / "k帰納法で全実行を証明") + the honesty line ("保証するのは内部整合;
   intent fidelity is borne by the per-row 判断").
   - **inline implements** (issue #1336) — after the header and any approval
     comparison, before governance and the undecided list, an `implements`
     envelope adds the folded top-level verdict (`検証判定`) and a table of
     `abs` and `result`: 🟢 only for `refines`, 🔴 otherwise. Passing seams
     are shown too. Adding this section alone does not change ledgers without
     `implements`; sentinel coverage removal and the verdict findings below
     independently change affected ledgers, even without `implements`.
     Migration: an already-approved inline-implements ledger has a changed
     rendering digest; `approval check` reports `drifted` (exit 0). Review
     the new section and re-approve it; the approval schema is unchanged.
   - **governance preservation** (governance documents only, issue #1316) —
     when the verification envelope carries a `governance` section, the
     top-level verdict (`検証判定`, the verify `result` after the #1303 fold)
     and one row per `preservation` in declaration order: name, `before` →
     `after`, preserved controls, and its own `result`, 🟢 for `refines` and
     🔴 for anything else (the fold's own predicate). Every preservation is
     listed, passing ones included, so the body agrees with the exit code. A
     ledger without a `governance` section gains no such section. This does not
     promise byte stability against other finding/rendering changes.
2. **未決定一覧** — when `undecided:` metadata exists, declaration / reason /
   state-dependency-derived affected requirement IDs. The marker is explicitly
   identified as metadata, not a verification condition.
3. **リスク一覧** — one row per requirement id: 業務目的 / 状態 (🔴 要確認 / 🟢
   確認済) / 検出種別 (`trace_type`) / リスク (`control.severity`) / 判断者
   (`control.owner`) / 次アクション. Coverage findings require an authored
   primary-origin span, or otherwise a kernel action span, with positive line
   and column. The shared `KernelModel::action_source_span` owner also governs
   runtime warnings, mutate dead-action notes, and HTML coverage badges/denominators.
   The current CLI mutant-enumeration path cannot encounter generated-only dead
   actions; the shared view keeps its note eligibility aligned with reports.
   Generated-only sentinels have no such finding; their raw `action_coverage` evidence remains
   unchanged in verify/induction/Worker JSON and HTML Raw Data. Names and
   prefixes are not the classification rule. An empty eligible HTML coverage
   map renders `0/0`. Migration (#1338): removing false sentinel findings changes
   affected ledger/HTML rendering digests; existing approvals report `drifted`
   (exit 0), so review and re-approve them. Approval schemas are unchanged.
4. **要件ID別詳細** — per requirement: the raw counterexample **translated into a
   business sentence** (dispatched on `trace_type`), the next action, and a
   decision line (☐承認 ☐差戻し ☐リスク受容 / 判断者 / 期限).
5. **付録** — the raw JSON findings, collapsed (`<details>`), demoted off page 1.
   Each finding omits only its raw top-level `cost` and `cache` execution metadata,
   the same set as the scenarios digest. The projection is not recursive: full
   CTI, hint, rank-failure and other proof evidence, and same-named fields inside
   domain state, remain bound. Ledger Markdown is hashed as rendered, without
   additional approval normalization; the appendix itself excludes execution
   metadata so failed-report bytes and approval rerendering are reproducible.

### Amendment to #1316: verdict findings (#1338, decision 2026-10-26)

The dedicated governance/implements sections alone are not sufficient for
requirement-level review. Every non-`refines` preservation now adds a finding
attributed to the governance catalog's `GOV` requirement, with declaration name,
result, `before` → `after`, controls, and its raw evidence subject only to the
appendix's top-level execution-metadata omission. Passing
preservations (including an empty list) add no findings. A non-`refines` inline
`implements` adds a spec-level finding, not a fabricated requirement link; an
otherwise confirmed requirement row can remain green. Both rules include
`impl_violated` and `unknown_budget`, independently of the primary verdict.
Budget exhaustion is unfinished search, not an established counterexample.
The requirement ID and text come from the model metadata attached by governance
lowering to its catalog invariant, not a copied ID or synthesized catalog text.

`unknown_cti` adds an **unestablished** induction finding, attributed by the
same declaration-owner rule as `violated`, or to the spec as a whole. Its
summary and translation do not claim a reachable counterexample or violation;
the appendix preserves the full `cti`, `hint`, and `rank_failure` evidence.

Declaration ownership follows the envelope's obligation, not the historical
`invariant` field alone: `trans` selects transitions; `leadsTo` / `leadsTo_rank`
select leadsTo declarations; synthetic partial-operation / key-domain property
checks select the property declaration; synthetic action checks, `ensures`, and
`type_bound` select `last_action`; deadlock has no declaration owner. An explicit
envelope requirement takes precedence. Otherwise the ledger uses `NameMap`'s
`resolve_action`, `resolve_property`, and `resolve_synthetic` against the selected
declarations, preferring `generated_name` to an authored display label (including
on `last_action`). Public compose and branch spellings are resolved by that name
owner, not by ledger string replacement or copied prefix rules. Only a unique
resolved declaration supplies requirement metadata; missing or ambiguous owners
remain spec-level findings.
Reachable-failure and dead-action coverage findings likewise use `generated_name`
when present to resolve their property or action declaration through `NameMap`.

Ordinary violations, requirement confirmations, assurance classes, and human
approval status remain separate axes. Added findings, corrected coverage, and
corrected declaration attribution change affected ledger rendering digests:
existing approvals become `drifted` and require
review and re-approval.

Direct `ledger` additionally returns a verification `result:"error"` envelope
and its exit (2 for document errors, 3 for internal errors) before replay,
evidence loading, witness preparation, rendering, approval overlays, or output-file
creation. This shares its gate with direct
HTML reporting, which checks before explain/render/write. Stdout is error JSON,
not Markdown/HTML or a generated-artifact envelope. Non-error failed or
unestablished verdicts still produce reports with their existing nonzero exit.
Approval-only `generate_unapproved_ledger_report_from_source` rerendering keeps
its existing digest-comparison behavior and does not use this direct-report gate.
Its ancillary-input errors retain their existing handling. For non-error
verification results, direct ledger also retains those ancillary-input errors.

### Amendment: one native verification pipeline (#1417)

`run_verify_from_source` is a no-cache/default-policy adapter to
`verification.rs::run_verify_cli_from_source`, not a second enumeration of
verification stages. The caller supplies the captured source and depth, deadlock,
engine, explicit budget and induction k; the default vacuity probe remains enabled.
The shared owner retains governance folding outside the verification cache.
Verify dispatch metadata and edition handling remain caller-owned.

Both direct ledger and HTML run verification and the existing error gate **before**
presentation-model loading; ledger extracts its checked trace contract afterwards.
No error conversion, panic catch, duplicate fold or second verification is added.
Approval-only ledger rerendering still bypasses the direct-report gate and retains
ancillary-input handling. Non-error verdicts retain their existing reports/exits.

The only public error corrections are in direct ledger/HTML reports on invalid inputs:

| Input | Previous report output | Shared verification output |
|---|---|---|
| Broken causal syntax | `FSL-DIALECT-UNKNOWN`, generic message, location 3:1 and `supported_dialects` | `FSL-PARSE`, causal parser message, location 4:3, no `supported_dialects` (regression fixture) |
| Agent document | semantics error, missing state | parse error, Agent verification refusal; no location |
| Unknown DB column | generated-name Kernel type message and location 12:5 | specialized unknown-column message, no location (regression fixture) |
| Unknown AI authority tool | panic, empty stdout, exit 101 | JSON semantics error naming the unknown tool, exit 2 |

The first three retain exit 2. Passing artifacts, warning lists, metadata,
projection shapes and approval target digests do not change. The regression
`rust/fslc/tests/issue_1417_verify_pipeline_routes.rs` enumerates all eight native
routes with applicable dialects, rejecting report controls and passing/failing
seam, governance, trace, vacuity and specialized-projection preservation controls.
Lower-only scenarios/explain/testgen preflight and the async Worker pipeline are
outside this consolidation; it does not claim those paths share native stages.
`approval create --kind ledger|html`, `approval check`, `approval diff`, and
`counterexample export` still lower the spec before verification or rerendering,
retaining their existing invalid-input precedence. After #1464, unknown AI authority
tools return a semantics error with exit 2 rather than panicking. Approval create's
diagnostic precedence remains a separate issue.

## Column → source map

Most columns are derived from fields the JSON already carries (issue #23):
`requirement.{id,text}`, `trace_type`, `recommended_action`/`hint`,
`checked_to_depth`+`completeness`. Governance columns (risk / decider) come from
`control` metadata when the spec declares it, and are left as fill-in fields
(`____`) otherwise — the ledger never fabricates a loss figure or a deadline.

## Non-goals

- No new verification. Hollowness depth (mutate kill-rate) and full multi-layer
  refinement aggregation are out of this first cut; `fslc mutate` / `fslc chain`
  remain the deeper audits. `verify` reports the first invariant violation only,
  so a ledger row may under-count simultaneous invariant breaks (noted in the
  guarantee limit).
