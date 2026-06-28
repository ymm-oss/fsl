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
  `trace_type` from #23), dead `reachable`s (`unreached[]`), uncovered actions,
  vacuity warnings, failed acceptance/forbidden (as errors), and the guarantee
  bound (`checked_to_depth` + `completeness`).
- `run_scenarios(file, depth, deadlock)` — passing acceptance / forbidden /
  reachable confirmations per requirement.
- `--impl-log <trace.json>` (optional) → `run_replay` — implementation-log
  conformance (`conformant` / `nonconformant`).
- The built `spec` — the requirement registry (every `requirement` id + text) and
  `control` governance (owner / severity) when present.

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

## Structure

1. **Header** — the guarantee limit in positive form ("BMC: depth N までの全実行を
   網羅" / "k帰納法で全実行を証明") + the honesty line ("保証するのは内部整合;
   intent fidelity is borne by the per-row 判断").
2. **リスク一覧** — one row per requirement id: 業務目的 / 状態 (🔴 要確認 / 🟢
   確認済) / 検出種別 (`trace_type`) / リスク (`control.severity`) / 判断者
   (`control.owner`) / 次アクション.
3. **要件ID別詳細** — per requirement: the raw counterexample **translated into a
   business sentence** (dispatched on `trace_type`), the next action, and a
   decision line (☐承認 ☐差戻し ☐リスク受容 / 判断者 / 期限).
4. **付録** — the raw JSON findings, collapsed (`<details>`), demoted off page 1.

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
