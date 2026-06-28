# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Audit-ledger renderer (issue #24).

A business-facing presentation layer over the verifier evidence, the audit
analogue of ``html_report``: it re-organizes ``verify`` / ``scenarios`` /
``replay`` findings **by requirement id** so a PM / governance / internal-audit
reader can decide approve / reject / risk-accept per requirement from the ledger
alone, without reading raw JSON or formulas. It introduces no second evaluator.

Most columns are derived from fields the JSON already carries: ``requirement``
and ``trace_type`` (issue #23), ``recommended_action``, ``checked_to_depth`` +
``completeness``. Governance columns (risk / decider) come from ``control``
metadata when present and are left as fill-in fields otherwise.
"""
from __future__ import annotations

import json
from pathlib import Path


def default_output_name(file: str) -> str:
    return f"{Path(file).stem}_ledger.md"


# --------------------------------------------------------------------------
# requirement registry (id -> business context) from the built spec
# --------------------------------------------------------------------------
_META_GROUPS = ("invariants", "actions", "leadstos", "reachables", "transitions")


def _requirement_registry(spec: dict) -> dict:
    """{req_id: {text, controls, owner, severity}} for every tagged element."""
    control_by_id = {c["id"]: c for c in (spec.get("controls") or [])}
    reg: dict = {}
    for group in _META_GROUPS:
        for item in spec.get(group) or []:
            meta = item.get("meta") if isinstance(item, dict) else None
            if not meta or not meta.get("id"):
                continue
            rid = meta["id"]
            entry = reg.setdefault(
                rid, {"text": meta.get("text"), "controls": [], "owner": None, "severity": None}
            )
            if entry["text"] is None:
                entry["text"] = meta.get("text")
            for ctrl in meta.get("controls") or []:
                cid = ctrl.get("id")
                if cid and cid not in [c["id"] for c in entry["controls"]]:
                    entry["controls"].append(ctrl)
                    info = control_by_id.get(cid)
                    if info:
                        entry["owner"] = entry["owner"] or info.get("owner")
                        entry["severity"] = entry["severity"] or info.get("severity")
    return reg


# --------------------------------------------------------------------------
# finding collection — the "needs human review" signals, grouped by requirement
# --------------------------------------------------------------------------
def _req_of(obj: dict):
    r = obj.get("requirement") if isinstance(obj, dict) else None
    if isinstance(r, dict):
        return r.get("id"), r.get("text")
    return None, None


def _finding(req_id, req_text, trace_type, name, summary, raw, next_action=None):
    return {
        "req_id": req_id,
        "req_text": req_text,
        "trace_type": trace_type,
        "name": name,
        "summary": summary,
        "next_action": next_action,
        "raw": raw,
    }


def _summarize_violation(v: dict) -> str:
    bindings = v.get("violating_bindings")
    last = v.get("last_action")
    parts = []
    if last:
        ln = last.get("name") if isinstance(last, dict) else last
        parts.append(f"アクション `{ln}` 実行後 (step {v.get('violated_at_step')})")
    if bindings:
        parts.append(f"binding {json.dumps(bindings, ensure_ascii=False)}")
    return "; ".join(parts) or "反例トレースあり（付録参照）"


def _collect_findings(verification: dict) -> list:
    findings = []
    result = verification.get("result")
    tt = verification.get("trace_type")

    if result == "violated":
        name = verification.get("invariant") or verification.get("leadsTo") or verification.get("trans") or verification.get("name")
        rid, rtext = _req_of(verification)
        findings.append(_finding(
            rid, rtext, tt or "invariant", name,
            _summarize_violation(verification), verification,
            verification.get("recommended_action"),
        ))
    elif result == "reachable_failed":
        for u in verification.get("unreached") or []:
            rid, rtext = _req_of(u)
            cls = u.get("classification")
            blocking = u.get("blocking_requires")
            summary = {
                "insufficient_depth": f"深さ {verification.get('checked_to_depth')} までに到達 trace なし（より深い探索が必要かもしれない）",
                "over_constrained": "型境界/不変条件により到達不能（ガードが過剰）",
            }.get(cls, "到達不能")
            if blocking:
                summary += f"／阻害: {json.dumps(blocking, ensure_ascii=False)}"
            findings.append(_finding(
                rid, rtext, "reachable", u.get("name"), summary, u, u.get("hint"),
            ))
    elif result == "error" and tt in ("acceptance", "forbidden"):
        rid, rtext = _req_of(verification)
        name = verification.get("id") or verification.get("name")
        if tt == "forbidden":
            summary = "禁止フローが仕様上許容されている（accepted_trace あり）"
        else:
            summary = f"受入シナリオが不成立（step {verification.get('failed_step')}）"
        findings.append(_finding(
            rid or name, rtext or verification.get("text"), tt, name, summary,
            verification, verification.get("hint"),
        ))

    # uncovered actions surface on any verify result (incl. verified)
    for action, info in (verification.get("action_coverage") or {}).items():
        if isinstance(info, dict) and info.get("covered") is False:
            rid, rtext = _req_of(info)
            blocking = info.get("blocking_requires")
            summary = "深さ内で一度も実行可能にならない（死アクション）"
            if blocking:
                summary += f"／阻害: {json.dumps(blocking, ensure_ascii=False)}"
            findings.append(_finding(
                rid, rtext, "coverage", action, summary, info, info.get("hint"),
            ))

    for w in verification.get("warnings") or []:
        if not isinstance(w, dict):
            continue
        rid, rtext = _req_of(w)
        findings.append(_finding(
            rid, rtext, "vacuity", w.get("name") or w.get("kind"),
            f"空虚性の疑い（{w.get('kind')}）: {w.get('message', '')}", w, w.get("hint"),
        ))
    return findings


# --------------------------------------------------------------------------
# business translation — dispatch on trace_type (issue #23 is what makes this work)
# --------------------------------------------------------------------------
def _translate(f: dict) -> str:
    t = f["trace_type"]
    name = f.get("name") or ""
    table = {
        "reachable": f"業務経路『{name}』が仕様上到達できない。受入条件に到達 trace を追加し、責任者が期待経路を承認する（死経路でないことの確認）。",
        "forbidden": f"禁止フロー『{name}』が許容されている。ガードを追加するか、許容するなら責任者がリスク受容を判断する。",
        "acceptance": f"受入シナリオ『{name}』が成立しない。仕様か受入条件のどちらが正かを責任者が確定する。",
        "sla": "SLA 期限を超過しうる。スケジューリング前提（urgent）か期限値を見直し、責任者が承認する。",
        "leadsTo": f"応答性『{name}』が保証されない経路がある。fair 指定または進行ロジックを見直す。",
        "leadsTo_rank": f"応答性『{name}』の停止性（ランキング）が示せない。進行が単調に進むことを確認する。",
        "refinement": "詳細仕様が上位契約から逸脱している。対応付け（mapping）かガードを修正する。",
        "conformance": "実装ログが仕様に非適合。実装か仕様のどちらが正かを確定する。",
        "coverage": f"アクション『{name}』が一度も実行可能にならない（死アクション）。ガードを緩めるか前提アクションを追加する。",
        "vacuity": f"性質『{name}』が空虚に成立している疑い（中身がない可能性）。`fslc mutate` で実効性を確認する。",
    }
    if t == "sla":
        return table["sla"]
    if t in ("invariant", "type_bound", "trans", "ensures", "partial_op", "deadlock"):
        return f"不変条件『{name}』が破れる経路がある。ガード修正かルール見直しを責任者が承認する。"
    return table.get(t, f"検出種別 {t}（付録の生 JSON を参照）。")


_DEFAULT_ACTION = {
    "reachable": "受入条件に到達 trace を追加 / ガードを緩める",
    "forbidden": "ガードを追加 / 責任者がリスク受容",
    "acceptance": "仕様 or 受入条件を修正",
    "sla": "urgent 前提 or 期限値を見直し",
    "refinement": "mapping / ガードを修正",
    "conformance": "実装 or 仕様を一致させる",
    "coverage": "ガードを緩める / 前提アクションを追加",
    "vacuity": "fslc mutate で実効性を確認",
}


def _next_action(f: dict) -> str:
    return f.get("next_action") or _DEFAULT_ACTION.get(f["trace_type"]) or "責任者が対応方針を決定"


# --------------------------------------------------------------------------
# rendering
# --------------------------------------------------------------------------
def _guarantee_line(verification: dict) -> str:
    completeness = verification.get("completeness")
    depth = verification.get("checked_to_depth", verification.get("depth"))
    if completeness == "unbounded":
        return "k帰納法で **全実行を証明済み**（深さ無制限）"
    return f"BMC（有界モデル検査）: **深さ {depth} までの全実行を網羅**。それ以遠の反例は本台帳の対象外"


def _esc(text) -> str:
    return str(text or "").replace("|", "\\|").replace("\n", " ")


def _confirmed_reqs(scenarios_result: dict) -> dict:
    """req_id -> list of passing scenario names (acceptance/forbidden/reachable)."""
    out: dict = {}
    for sc in scenarios_result.get("scenarios") or []:
        rid, _ = _req_of(sc)
        if rid:
            out.setdefault(rid, []).append(f"{sc.get('kind')}:{sc.get('name')}")
    return out


def render_ledger(file, spec, verification, scenarios_result, replay_result=None) -> str:
    registry = _requirement_registry(spec)
    findings = _collect_findings(verification)
    confirmed = _confirmed_reqs(scenarios_result or {})

    by_req: dict = {}
    spec_level = []
    for f in findings:
        (by_req.setdefault(f["req_id"], []).append(f) if f["req_id"] else spec_level.append(f))

    # every requirement id that exists or has a finding
    all_ids = list(registry.keys())
    for rid in by_req:
        if rid not in all_ids:
            all_ids.append(rid)

    L = []
    L.append(f"# 意図ずれ監査台帳: {spec.get('name', Path(file).stem)}")
    L.append("")
    L.append(f"- 対象: `{file}`")
    L.append(f"- 保証限界: {_guarantee_line(verification)}")
    L.append("- この台帳が保証するのは **書かれた仕様の内部整合**。仕様が現実の意図に忠実かは各行の **判断** 欄で人間が担保する。")
    if replay_result is not None:
        rr = replay_result.get("result")
        if rr == "nonconformant":
            L.append(f"- ⚠ 実装ログ適合: **非適合**（イベント {replay_result.get('failed_at_event')} で乖離）")
        elif rr == "conformant":
            L.append(f"- 実装ログ適合: 適合（{replay_result.get('steps_checked')} ステップ）")
    L.append("")

    # page 1 — risk list
    L.append("## リスク一覧（要件ID別）")
    L.append("")
    L.append("| 要件ID | 業務目的 | 状態 | 検出種別 | リスク | 判断者 | 次アクション |")
    L.append("|---|---|---|---|---|---|---|")
    for rid in all_ids:
        reg = registry.get(rid, {})
        fs = by_req.get(rid, [])
        purpose = _esc(reg.get("text") or (fs[0]["req_text"] if fs else "") or "—")
        if fs:
            status = "🔴 要確認"
            types = ", ".join(sorted({f["trace_type"] for f in fs}))
            action = " / ".join(sorted({_next_action(f) for f in fs}))
        else:
            status = "🟢 確認済（承認可）" if rid in confirmed else "🟢 反例なし"
            types = "—"
            action = "—"
        risk = _esc(reg.get("severity") or ("要確認" if fs else "—"))
        owner = _esc(reg.get("owner") or ("____" if fs else "—"))
        L.append(f"| {_esc(rid)} | {purpose} | {status} | {_esc(types)} | {risk} | {owner} | {_esc(action)} |")
    if spec_level:
        types = ", ".join(sorted({f["trace_type"] for f in spec_level}))
        L.append(f"| （仕様全体） | 要件ID未付与の検出 | 🔴 要確認 | {_esc(types)} | 要確認 | ____ | 下記詳細 |")
    L.append("")

    # page 2+ — per-requirement detail
    L.append("## 要件ID別詳細")
    L.append("")
    detail_ids = [r for r in all_ids if by_req.get(r)] + (["（仕様全体）"] if spec_level else [])
    if not detail_ids:
        L.append("検出された意図ずれ・死経路・禁止経路はありません（深さ内）。受入確認の対象は上表の「確認済」行。")
        L.append("")
    for rid in detail_ids:
        fs = spec_level if rid == "（仕様全体）" else by_req.get(rid, [])
        reg = registry.get(rid, {})
        L.append(f"### {rid}" + (f" — {reg.get('text')}" if reg.get("text") else ""))
        if reg.get("controls"):
            cids = ", ".join(c.get("id") for c in reg["controls"])
            gov = f"統制: {cids}"
            if reg.get("owner"):
                gov += f"（owner: {reg['owner']}"
                gov += f", severity: {reg['severity']}）" if reg.get("severity") else "）"
            L.append(f"- {gov}")
        for f in fs:
            L.append(f"- **検出**: `{f['trace_type']}` — {_esc(f.get('name'))}")
            L.append(f"  - 反例要約: {_esc(f['summary'])}")
            L.append(f"  - 業務翻訳: {_translate(f)}")
            L.append(f"  - 次アクション: {_esc(_next_action(f))}")
        L.append(f"- 判断: ☐ 承認　☐ 差戻し　☐ リスク受容　／　判断者: {reg.get('owner') or '____'}　期限: ____")
        L.append("")

    # appendix — raw JSON demoted
    L.append("## 付録: 生 JSON 反例（証跡）")
    L.append("")
    L.append("<details><summary>raw findings</summary>")
    L.append("")
    L.append("```json")
    L.append(json.dumps([f["raw"] for f in findings], indent=2, ensure_ascii=False))
    L.append("```")
    L.append("")
    L.append("</details>")
    L.append("")
    return "\n".join(L)
