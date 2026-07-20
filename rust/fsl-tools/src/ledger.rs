// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Business audit-ledger renderer over native verifier evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use fsl_core::KernelModel;
use fsl_syntax::{Annotations, RequirementLink};
use serde_json::Value;

use crate::undecided_declarations;

#[derive(Clone, Default)]
struct RequirementEntry {
    text: Option<String>,
    elements: Vec<(String, String)>,
}

#[derive(Clone)]
struct Finding {
    requirement_id: Option<String>,
    requirement_text: Option<String>,
    trace_type: String,
    name: String,
    summary: String,
    next_action: Option<String>,
    raw: Value,
}

fn add_requirement(
    order: &mut Vec<String>,
    registry: &mut BTreeMap<String, RequirementEntry>,
    group: &str,
    name: &str,
    annotations: &Annotations,
) {
    for requirement in annotations
        .requirements()
        .expect("checked model annotations are valid")
    {
        if !registry.contains_key(&requirement.id) {
            order.push(requirement.id.clone());
        }
        let entry = registry.entry(requirement.id).or_default();
        if entry.text.is_none() {
            entry.text = requirement.text;
        }
        entry.elements.push((group.to_owned(), name.to_owned()));
    }
}

fn requirement_registry(model: &KernelModel) -> (Vec<String>, BTreeMap<String, RequirementEntry>) {
    let mut order = Vec::new();
    let mut registry = BTreeMap::new();
    for property in &model.invariants {
        add_requirement(
            &mut order,
            &mut registry,
            "invariants",
            &property.name,
            &property.annotations,
        );
    }
    for action in &model.actions {
        add_requirement(
            &mut order,
            &mut registry,
            "actions",
            &action.name,
            &action.annotations,
        );
    }
    for property in &model.leadstos {
        add_requirement(
            &mut order,
            &mut registry,
            "leadstos",
            &property.name,
            &property.annotations,
        );
    }
    for property in &model.reachables {
        add_requirement(
            &mut order,
            &mut registry,
            "reachables",
            &property.name,
            &property.annotations,
        );
    }
    for property in &model.transitions {
        add_requirement(
            &mut order,
            &mut registry,
            "transitions",
            &property.name,
            &property.annotations,
        );
    }
    (order, registry)
}

fn requirement(value: &Value) -> (Option<String>, Option<String>) {
    let requirement = value.get("requirement").and_then(Value::as_object);
    if requirement
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
        .is_some_and(|id| id.eq_ignore_ascii_case("undecided"))
    {
        return (None, None);
    }
    (
        requirement
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        requirement
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    )
}

fn metadata_for(model: &KernelModel, group: &str, name: &str) -> Option<RequirementLink> {
    let annotations = match group {
        "invariants" => model
            .invariants
            .iter()
            .find(|property| property.name == name)
            .map(|property| &property.annotations),
        "transitions" => model
            .transitions
            .iter()
            .find(|property| property.name == name)
            .map(|property| &property.annotations),
        "reachables" => model
            .reachables
            .iter()
            .find(|property| property.name == name)
            .map(|property| &property.annotations),
        "leadstos" => model
            .leadstos
            .iter()
            .find(|property| property.name == name)
            .map(|property| &property.annotations),
        "actions" => model
            .actions
            .iter()
            .find(|action| action.name == name)
            .map(|action| &action.annotations),
        _ => None,
    };
    annotations.and_then(|annotations| {
        annotations
            .requirements()
            .expect("checked model annotations are valid")
            .into_iter()
            .next()
    })
}

fn finding_requirement(
    model: &KernelModel,
    value: &Value,
    group: &str,
    name: &str,
) -> (Option<String>, Option<String>) {
    let direct = requirement(value);
    if direct.0.is_some() {
        return direct;
    }
    metadata_for(model, group, name)
        .map_or((None, None), |metadata| (Some(metadata.id), metadata.text))
}

fn summarize_violation(value: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(action) = value.get("last_action") {
        let name = action
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| action.as_str())
            .unwrap_or("");
        parts.push(format!(
            "アクション `{name}` 実行後 (step {})",
            value
                .get("violated_at_step")
                .map_or_else(|| "null".to_owned(), Value::to_string)
        ));
    }
    if let Some(bindings) = value
        .get("violating_bindings")
        .filter(|bindings| !bindings.is_null())
    {
        parts.push(format!("binding {bindings}"));
    }
    if parts.is_empty() {
        "反例トレースあり（付録参照）".to_owned()
    } else {
        parts.join("; ")
    }
}

#[allow(clippy::too_many_lines)]
fn collect_findings(model: &KernelModel, verification: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    let result = verification
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or("");
    let trace_type = verification
        .get("trace_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    if result == "violated" {
        let (group, name) =
            if let Some(name) = verification.get("invariant").and_then(Value::as_str) {
                ("invariants", name)
            } else if let Some(name) = verification.get("leadsTo").and_then(Value::as_str) {
                ("leadstos", name)
            } else if let Some(name) = verification.get("trans").and_then(Value::as_str) {
                ("transitions", name)
            } else {
                (
                    "",
                    verification
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                )
            };
        let metadata = finding_requirement(model, verification, group, name);
        findings.push(Finding {
            requirement_id: metadata.0,
            requirement_text: metadata.1,
            trace_type: if trace_type.is_empty() {
                "invariant"
            } else {
                trace_type
            }
            .to_owned(),
            name: name.to_owned(),
            summary: summarize_violation(verification),
            next_action: verification
                .get("recommended_action")
                .and_then(Value::as_str)
                .map(str::to_owned),
            raw: verification.clone(),
        });
    } else if result == "reachable_failed" {
        for item in verification
            .get("unreached")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("");
            let metadata = finding_requirement(model, item, "reachables", name);
            let mut summary = match item.get("classification").and_then(Value::as_str) {
                Some("insufficient_depth") => format!(
                    "深さ {} までに到達 trace なし（より深い探索が必要かもしれない）",
                    verification
                        .get("checked_to_depth")
                        .map_or_else(|| "null".to_owned(), Value::to_string)
                ),
                Some("over_constrained") => {
                    "型境界/不変条件により到達不能（ガードが過剰）".to_owned()
                }
                _ => "到達不能".to_owned(),
            };
            if let Some(blocking) = item
                .get("blocking_requires")
                .filter(|value| value.as_array().is_some_and(|items| !items.is_empty()))
            {
                let _ = write!(summary, "／阻害: {blocking}");
            }
            findings.push(Finding {
                requirement_id: metadata.0,
                requirement_text: metadata.1,
                trace_type: "reachable".to_owned(),
                name: name.to_owned(),
                summary,
                next_action: item.get("hint").and_then(Value::as_str).map(str::to_owned),
                raw: item.clone(),
            });
        }
    } else if result == "error" && matches!(trace_type, "acceptance" | "forbidden") {
        let name = verification
            .get("id")
            .or_else(|| verification.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let metadata = requirement(verification);
        findings.push(Finding {
            requirement_id: metadata.0.or_else(|| Some(name.to_owned())),
            requirement_text: metadata.1.or_else(|| {
                verification
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }),
            trace_type: trace_type.to_owned(),
            name: name.to_owned(),
            summary: if trace_type == "forbidden" {
                "禁止フローが仕様上許容されている（accepted_trace あり）".to_owned()
            } else {
                format!(
                    "受入シナリオが不成立（step {}）",
                    verification
                        .get("failed_step")
                        .map_or_else(|| "null".to_owned(), Value::to_string)
                )
            },
            next_action: verification
                .get("hint")
                .and_then(Value::as_str)
                .map(str::to_owned),
            raw: verification.clone(),
        });
    }
    for (name, item) in verification
        .get("action_coverage")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if item.get("covered").and_then(Value::as_bool) != Some(false) {
            continue;
        }
        let metadata = finding_requirement(model, item, "actions", name);
        let mut summary = "深さ内で一度も実行可能にならない（死アクション）".to_owned();
        if let Some(blocking) = item
            .get("blocking_requires")
            .filter(|value| value.as_array().is_some_and(|items| !items.is_empty()))
        {
            let _ = write!(summary, "／阻害: {blocking}");
        }
        findings.push(Finding {
            requirement_id: metadata.0,
            requirement_text: metadata.1,
            trace_type: "coverage".to_owned(),
            name: name.clone(),
            summary,
            next_action: item.get("hint").and_then(Value::as_str).map(str::to_owned),
            raw: item.clone(),
        });
    }
    for warning in verification
        .get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = warning
            .get("name")
            .or_else(|| warning.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let metadata = requirement(warning);
        findings.push(Finding {
            requirement_id: metadata.0,
            requirement_text: metadata.1,
            trace_type: "vacuity".to_owned(),
            name: name.to_owned(),
            summary: format!(
                "空虚性の疑い（{}）: {}",
                warning.get("kind").and_then(Value::as_str).unwrap_or(""),
                warning.get("message").and_then(Value::as_str).unwrap_or("")
            ),
            next_action: warning
                .get("hint")
                .and_then(Value::as_str)
                .map(str::to_owned),
            raw: warning.clone(),
        });
    }
    findings
}

fn escape(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn translate(finding: &Finding) -> String {
    match finding.trace_type.as_str() {
        "reachable" => format!(
            "業務経路『{}』が仕様上到達できない。受入条件に到達 trace を追加し、責任者が期待経路を承認する（死経路でないことの確認）。",
            finding.name
        ),
        "forbidden" => format!(
            "禁止フロー『{}』が許容されている。ガードを追加するか、許容するなら責任者がリスク受容を判断する。",
            finding.name
        ),
        "acceptance" => format!(
            "受入シナリオ『{}』が成立しない。仕様か受入条件のどちらが正かを責任者が確定する。",
            finding.name
        ),
        "sla" => "SLA 期限を超過しうる。スケジューリング前提（urgent）か期限値を見直し、責任者が承認する。".to_owned(),
        "leadsTo" => format!(
            "応答性『{}』が保証されない経路がある。fair 指定または進行ロジックを見直す。",
            finding.name
        ),
        "leadsTo_rank" => format!(
            "応答性『{}』の停止性（ランキング）が示せない。進行が単調に進むことを確認する。",
            finding.name
        ),
        "refinement" => "詳細仕様が上位契約から逸脱している。対応付け（mapping）かガードを修正する。".to_owned(),
        "conformance" => "実装ログが仕様に非適合。実装か仕様のどちらが正かを確定する。".to_owned(),
        "coverage" => format!(
            "アクション『{}』が一度も実行可能にならない（死アクション）。ガードを緩めるか前提アクションを追加する。",
            finding.name
        ),
        "vacuity" => format!(
            "性質『{}』が空虚に成立している疑い（中身がない可能性）。`fslc mutate` で実効性を確認する。",
            finding.name
        ),
        "invariant" | "type_bound" | "trans" | "ensures" | "partial_op" | "deadlock" => format!(
            "不変条件『{}』が破れる経路がある。ガード修正かルール見直しを責任者が承認する。",
            finding.name
        ),
        other => format!("検出種別 {other}（付録の生 JSON を参照）。"),
    }
}

fn next_action(finding: &Finding) -> String {
    finding.next_action.clone().unwrap_or_else(|| {
        match finding.trace_type.as_str() {
            "reachable" => "受入条件に到達 trace を追加 / ガードを緩める",
            "forbidden" => "ガードを追加 / 責任者がリスク受容",
            "acceptance" => "仕様 or 受入条件を修正",
            "sla" => "urgent 前提 or 期限値を見直し",
            "refinement" => "mapping / ガードを修正",
            "conformance" => "実装 or 仕様を一致させる",
            "coverage" => "ガードを緩める / 前提アクションを追加",
            "vacuity" => "fslc mutate で実効性を確認",
            _ => "責任者が対応方針を決定",
        }
        .to_owned()
    })
}

/// Every requirement ID an evidence envelope declares itself attached to —
/// its own `requirements` string array, plus a singular `requirement.id`
/// field. Shared with `document_evidence.rs` (issue #332) so claim-level
/// evidence matching can never silently diverge from ledger's own.
pub(crate) fn evidence_requirement_ids(item: &Value) -> Vec<&str> {
    let mut ids = item
        .get("requirements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if let Some(id) = item
        .get("requirement")
        .and_then(Value::as_object)
        .and_then(|requirement| requirement.get("id"))
        .and_then(Value::as_str)
    {
        ids.push(id);
    }
    ids
}

/// Classify one JSON envelope (an evidence file's parsed contents, or a
/// `fslc verify` result) into the shared assurance vocabulary (issue #171,
/// `docs/DESIGN-assurance-classes.md`): `proved` / `bounded` /
/// `replay-observed` / `statistical` / `not_run`. `pub(crate)` so
/// `document_evidence.rs` (issue #332) reuses this exact classification
/// rather than re-deriving it — acceptance criterion 3 ("`bounded` never
/// displays as `proved`") reduces to this already-tested function.
pub(crate) fn assurance_token(value: &Value) -> &'static str {
    let completeness = value
        .get("completeness")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("kernel")
                .and_then(Value::as_object)
                .and_then(|kernel| kernel.get("completeness"))
                .and_then(Value::as_str)
        });
    if completeness == Some("unbounded") {
        return "proved";
    }
    if completeness == Some("bounded") {
        return "bounded";
    }
    let result = value.get("result").and_then(Value::as_str).unwrap_or("");
    let evidence_kind = value
        .get("evidence")
        .and_then(Value::as_object)
        .and_then(|evidence| evidence.get("kind"))
        .and_then(Value::as_str);
    if value.get("guarantee_kind").and_then(Value::as_str) == Some("runtime_observed")
        || matches!(evidence_kind, Some("runtime_replay" | "runtime_telemetry"))
        || matches!(
            result,
            "conformant"
                | "nonconformant"
                | "replay_conformant"
                | "replay_nonconformant"
                | "observed_conformant"
                | "observed_mismatch"
                | "conformance_checked"
                | "observed_supported"
                | "evidence_supported"
                | "evidence_failed"
        )
    {
        return "replay-observed";
    }
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or(result);
    if matches!(
        status,
        "statistically_supported" | "statistically_unsupported"
    ) {
        return "statistical";
    }
    "not_run"
}

/// Turn an `assurance_token` result into its display string
/// (`"bounded"` + `Some(8)` -> `"bounded(BMC depth 8)"`). `pub(crate)` for
/// the same reason as `assurance_token`.
pub(crate) fn assurance_label(token: &str, depth: Option<u64>) -> String {
    match token {
        "proved" => "proved(induction)".to_owned(),
        "bounded" => depth.map_or_else(
            || "bounded".to_owned(),
            |depth| format!("bounded(BMC depth {depth})"),
        ),
        "replay-observed" | "statistical" | "not_run" => token.to_owned(),
        _ => "not_run".to_owned(),
    }
}

fn formal_assurance(group: &str, name: &str, verification: &Value) -> &'static str {
    if verification.get("result").and_then(Value::as_str) == Some("proved") {
        if matches!(group, "invariants" | "transitions") {
            return "proved";
        }
        if group == "leadstos" {
            return if verification
                .get("leads_to")
                .and_then(Value::as_object)
                .and_then(|values| values.get(name))
                .and_then(Value::as_object)
                .and_then(|value| value.get("completeness"))
                .and_then(Value::as_str)
                == Some("unbounded")
            {
                "proved"
            } else {
                "bounded"
            };
        }
        return "bounded";
    }
    if verification.get("result").and_then(Value::as_str) == Some("error") {
        "not_run"
    } else if verification.get("completeness").and_then(Value::as_str) == Some("unbounded") {
        "proved"
    } else {
        "bounded"
    }
}

fn assurance_cell(
    requirement_id: &str,
    registry: &BTreeMap<String, RequirementEntry>,
    verification: &Value,
    evidence: &[(String, Value)],
) -> String {
    let mut sources = Vec::new();
    if let Some(entry) = registry.get(requirement_id) {
        let formal = entry
            .elements
            .iter()
            .map(|(group, name)| formal_assurance(group, name, verification))
            .max_by_key(|token| match *token {
                "proved" => 0,
                "bounded" => 1,
                _ => 4,
            });
        if let Some(formal) = formal {
            sources.push(formal);
        }
    }
    for (_, item) in evidence {
        if evidence_requirement_ids(item).contains(&requirement_id) {
            sources.push(assurance_token(item));
        }
    }
    let depth = verification
        .get("checked_to_depth")
        .or_else(|| verification.get("depth"))
        .and_then(Value::as_u64);
    let mut labels = Vec::new();
    for token in [
        "proved",
        "bounded",
        "replay-observed",
        "statistical",
        "not_run",
    ] {
        if sources.contains(&token) {
            let label = assurance_label(token, depth);
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
    }
    if labels.is_empty() {
        "not_run".to_owned()
    } else {
        labels.join(" + ")
    }
}

fn guarantee_line(verification: &Value) -> String {
    if verification.get("completeness").and_then(Value::as_str) == Some("unbounded") {
        return "k帰納法で **全実行を証明済み**（深さ無制限）".to_owned();
    }
    let depth = verification
        .get("checked_to_depth")
        .or_else(|| verification.get("depth"))
        .map_or_else(|| "null".to_owned(), Value::to_string);
    format!(
        "BMC（有界モデル検査）: **深さ {depth} までの全実行を網羅**。それ以遠の反例は本台帳の対象外"
    )
}

fn approval_entry<'a>(approvals: Option<&'a Value>, requirement_id: &str) -> Option<&'a Value> {
    approvals?.get("requirements")?.get(requirement_id)
}

fn approval_cell(approvals: Option<&Value>, requirement_id: &str) -> String {
    let Some(entry) = approval_entry(approvals, requirement_id) else {
        return "— unapproved".to_owned();
    };
    if entry.get("signature_status").and_then(Value::as_str) == Some("signature-invalid") {
        return "❌ signature-invalid".to_owned();
    }
    let signature = match entry.get("signature_status").and_then(Value::as_str) {
        Some("signed") => "signed",
        _ => "unsigned",
    };
    match entry.get("status").and_then(Value::as_str) {
        Some("approved") => format!("✅ approved ({signature})"),
        Some("drifted") => format!(
            "⚠ drifted ({signature}; since {})",
            entry
                .get("baseline_digest")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        Some(status) => format!("❌ {status}"),
        None => "❌ invalid".to_owned(),
    }
}

/// Render the complete requirement-oriented Markdown audit ledger.
#[must_use]
pub fn render_ledger(
    file: &str,
    model: &KernelModel,
    verification: &Value,
    scenarios: &Value,
    replay: Option<&Value>,
    evidence: &[(String, Value)],
) -> String {
    render_ledger_with_approvals(file, model, verification, scenarios, replay, evidence, None)
}

/// Render the audit ledger with optional digest-bound approval decisions.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn render_ledger_with_approvals(
    file: &str,
    model: &KernelModel,
    verification: &Value,
    scenarios: &Value,
    replay: Option<&Value>,
    evidence: &[(String, Value)],
    approvals: Option<&Value>,
) -> String {
    let (mut requirement_order, registry) = requirement_registry(model);
    let undecided = undecided_declarations(model);
    let findings = collect_findings(model, verification);
    let mut by_requirement: BTreeMap<String, Vec<&Finding>> = BTreeMap::new();
    let mut spec_level = Vec::new();
    for finding in &findings {
        if let Some(requirement_id) = &finding.requirement_id {
            by_requirement
                .entry(requirement_id.clone())
                .or_default()
                .push(finding);
            if !requirement_order.contains(requirement_id) {
                requirement_order.push(requirement_id.clone());
            }
        } else {
            spec_level.push(finding);
        }
    }
    if let Some(approved_requirements) = approvals
        .and_then(|value| value.get("requirements"))
        .and_then(Value::as_object)
    {
        for requirement_id in approved_requirements.keys() {
            if !requirement_order.contains(requirement_id) {
                requirement_order.push(requirement_id.clone());
            }
        }
    }
    let mut confirmed = BTreeSet::new();
    for scenario in scenarios
        .get("scenarios")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(id) = scenario
            .get("requirement")
            .and_then(Value::as_object)
            .and_then(|requirement| requirement.get("id"))
            .and_then(Value::as_str)
        {
            confirmed.insert(id.to_owned());
            continue;
        }
        let metadata = match scenario.get("kind").and_then(Value::as_str) {
            Some("reachable") => scenario
                .get("property")
                .and_then(Value::as_str)
                .and_then(|name| metadata_for(model, "reachables", name)),
            Some("leadsTo") => scenario
                .get("property")
                .and_then(Value::as_str)
                .and_then(|name| metadata_for(model, "leadstos", name)),
            Some("action_coverage") => scenario
                .get("action")
                .and_then(Value::as_str)
                .and_then(|name| metadata_for(model, "actions", name)),
            _ => None,
        };
        if let Some(metadata) = metadata {
            confirmed.insert(metadata.id.clone());
        }
    }

    let mut output = format!(
        "# 意図ずれ監査台帳: {}\n\n- 対象: `{file}`\n- 保証限界: {}\n- 保証クラス（要件ID別）: `proved(induction)` 全深さで証明 / `bounded(BMC depth k)` 深さkまで網羅 / `replay-observed` ログ照合のみ / `statistical` Wilson区間による統計的裏付け / `not_run` 形式的根拠なし。詳細は `docs/DESIGN-assurance-classes.md`。\n- この台帳が保証するのは **書かれた仕様の内部整合**。仕様が現実の意図に忠実かは各行の **判断** 欄で人間が担保する。\n",
        model.name,
        guarantee_line(verification)
    );
    if let Some(replay) = replay {
        match replay.get("result").and_then(Value::as_str) {
            Some("nonconformant") => {
                let _ = writeln!(
                    output,
                    "- ⚠ 実装ログ適合: **非適合**（イベント {} で乖離）",
                    replay
                        .get("failed_at_event")
                        .map_or_else(|| "null".to_owned(), Value::to_string)
                );
            }
            Some("conformant") => {
                let _ = writeln!(
                    output,
                    "- 実装ログ適合: 適合（{} ステップ）",
                    replay
                        .get("steps_checked")
                        .map_or_else(|| "null".to_owned(), Value::to_string)
                );
            }
            _ => {}
        }
    }
    if approvals.is_some() {
        output.push_str(
            "- 承認照合: versioned approval record の spec/rendering digest を照合済み。\n",
        );
    }
    if !undecided.is_empty() {
        output.push_str(
            "\n## 未決定一覧\n\n`undecided:` は意図的な未決定を記録するメタデータであり、検証条件には含まれません。\n\n| 宣言 | 未決定の理由 | 影響する要件ID |\n|---|---|---|\n",
        );
        for item in &undecided {
            let ids = item["requirement_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                output,
                "| `{}` | {} | {} |",
                escape(item["declaration"].as_str().unwrap_or_default()),
                escape(item["reason"].as_str().unwrap_or_default()),
                if ids.is_empty() {
                    "—".to_owned()
                } else {
                    escape(&ids)
                },
            );
        }
    }
    if approvals.is_some() {
        output.push_str(
            "\n## リスク一覧（要件ID別）\n\n| 要件ID | 業務目的 | 状態 | 承認状態 | 保証クラス | 検出種別 | リスク | 判断者 | 次アクション |\n|---|---|---|---|---|---|---|---|---|\n",
        );
    } else {
        output.push_str(
            "\n## リスク一覧（要件ID別）\n\n| 要件ID | 業務目的 | 状態 | 保証クラス | 検出種別 | リスク | 判断者 | 次アクション |\n|---|---|---|---|---|---|---|---|\n",
        );
    }
    for requirement_id in &requirement_order {
        let entry = registry.get(requirement_id);
        let requirement_findings = by_requirement
            .get(requirement_id)
            .map_or(&[][..], Vec::as_slice);
        let fallback_text = requirement_findings
            .first()
            .and_then(|finding| finding.requirement_text.as_deref())
            .unwrap_or("—");
        let purpose = entry
            .and_then(|entry| entry.text.as_deref())
            .unwrap_or(fallback_text);
        let (status, types, risk, owner, action) = if requirement_findings.is_empty()
            && entry.is_none()
            && approval_entry(approvals, requirement_id).is_some()
        {
            (
                "🟡 現行仕様に要件IDなし",
                "—".to_owned(),
                "要確認",
                "____",
                "要件IDの削除または変更を確認".to_owned(),
            )
        } else if requirement_findings.is_empty() {
            (
                if confirmed.contains(requirement_id) {
                    "🟢 確認済（承認可）"
                } else {
                    "🟢 反例なし"
                },
                "—".to_owned(),
                "—",
                "—",
                "—".to_owned(),
            )
        } else {
            let types = requirement_findings
                .iter()
                .map(|finding| finding.trace_type.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ");
            let action = requirement_findings
                .iter()
                .map(|finding| next_action(finding))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(" / ");
            ("🔴 要確認", types, "要確認", "____", action)
        };
        if approvals.is_some() {
            let _ = writeln!(
                output,
                "| {} | {} | {status} | {} | {} | {} | {risk} | {owner} | {} |",
                escape(requirement_id),
                escape(purpose),
                escape(&approval_cell(approvals, requirement_id)),
                escape(&assurance_cell(
                    requirement_id,
                    &registry,
                    verification,
                    evidence
                )),
                escape(&types),
                escape(&action),
            );
        } else {
            let _ = writeln!(
                output,
                "| {} | {} | {status} | {} | {} | {risk} | {owner} | {} |",
                escape(requirement_id),
                escape(purpose),
                escape(&assurance_cell(
                    requirement_id,
                    &registry,
                    verification,
                    evidence
                )),
                escape(&types),
                escape(&action),
            );
        }
    }
    if !spec_level.is_empty() {
        let types = spec_level
            .iter()
            .map(|finding| finding.trace_type.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        let depth = verification.get("checked_to_depth").and_then(Value::as_u64);
        if approvals.is_some() {
            let _ = writeln!(
                output,
                "| （仕様全体） | 要件ID未付与の検出 | 🔴 要確認 | — unapproved | {} | {} | 要確認 | ____ | 下記詳細 |",
                escape(&assurance_label(assurance_token(verification), depth)),
                escape(&types)
            );
        } else {
            let _ = writeln!(
                output,
                "| （仕様全体） | 要件ID未付与の検出 | 🔴 要確認 | {} | {} | 要確認 | ____ | 下記詳細 |",
                escape(&assurance_label(assurance_token(verification), depth)),
                escape(&types)
            );
        }
    }
    output.push_str("\n## 要件ID別詳細\n\n");
    let mut detail_ids = requirement_order
        .iter()
        .filter(|id| {
            by_requirement
                .get(*id)
                .is_some_and(|items| !items.is_empty())
        })
        .cloned()
        .collect::<Vec<_>>();
    if !spec_level.is_empty() {
        detail_ids.push("（仕様全体）".to_owned());
    }
    if detail_ids.is_empty() {
        output.push_str(
            "検出された意図ずれ・死経路・禁止経路はありません（深さ内）。受入確認の対象は上表の「確認済」行。\n\n",
        );
    }
    for requirement_id in detail_ids {
        let requirement_findings = if requirement_id == "（仕様全体）" {
            spec_level.clone()
        } else {
            by_requirement
                .get(&requirement_id)
                .cloned()
                .unwrap_or_default()
        };
        let text = registry
            .get(&requirement_id)
            .and_then(|entry| entry.text.as_deref());
        let _ = writeln!(
            output,
            "### {requirement_id}{}",
            text.map_or_else(String::new, |text| format!(" — {text}"))
        );
        if requirement_id != "（仕様全体）" {
            let _ = writeln!(
                output,
                "- 保証クラス: {}",
                assurance_cell(&requirement_id, &registry, verification, evidence)
            );
        }
        for finding in requirement_findings {
            let _ = writeln!(
                output,
                "- **検出**: `{}` — {}\n  - 反例要約: {}\n  - 業務翻訳: {}\n  - 次アクション: {}",
                finding.trace_type,
                escape(&finding.name),
                escape(&finding.summary),
                translate(finding),
                escape(&next_action(finding)),
            );
        }
        output.push_str("- 判断: ☐ 承認　☐ 差戻し　☐ リスク受容　／　判断者: ____　期限: ____\n\n");
    }
    if let Some(approvals) = approvals {
        output.push_str(
            "## 承認照合\n\n| 要件ID | 承認状態 | 承認基準 digest | 承認対象 | drift理由 | semantic diff |\n|---|---|---|---|---|---|\n",
        );
        for requirement_id in &requirement_order {
            let entry = approval_entry(Some(approvals), requirement_id);
            let baseline = entry
                .and_then(|item| item.get("baseline_digest"))
                .and_then(Value::as_str)
                .unwrap_or("—");
            let target = entry
                .and_then(|item| item.get("target_kind"))
                .and_then(Value::as_str)
                .unwrap_or("—");
            let reasons = entry
                .and_then(|item| item.get("reasons"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            let command = entry
                .filter(|item| item.get("status").and_then(Value::as_str) == Some("drifted"))
                .and_then(|item| item.get("semantic_diff_command"))
                .and_then(Value::as_str)
                .unwrap_or("—");
            let _ = writeln!(
                output,
                "| {} | {} | `{}` | {} | {} | `{}` |",
                escape(requirement_id),
                escape(&approval_cell(Some(approvals), requirement_id)),
                escape(baseline),
                escape(target),
                escape(if reasons.is_empty() { "—" } else { &reasons }),
                escape(command),
            );
        }
        output.push('\n');
    }
    if !evidence.is_empty() {
        output.push_str(
            "## 外部エビデンス\n\n| ファイル | producer結果 | 保証クラス | 対象要件ID |\n|---|---|---|---|\n",
        );
        for (source, item) in evidence {
            let ids = evidence_requirement_ids(item)
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let ids = if ids.is_empty() {
                "（仕様全体）".to_owned()
            } else {
                ids.join(", ")
            };
            let _ = writeln!(
                output,
                "| `{}` | {} | {} | {} |",
                escape(source),
                escape(item.get("result").and_then(Value::as_str).unwrap_or("")),
                assurance_label(assurance_token(item), None),
                escape(&ids),
            );
        }
        output.push('\n');
    }
    output.push_str(
        "## 付録: 生 JSON 反例（証跡）\n\n<details><summary>raw findings</summary>\n\n```json\n",
    );
    let raw = Value::Array(findings.into_iter().map(|finding| finding.raw).collect());
    output.push_str(&serde_json::to_string_pretty(&raw).unwrap_or_else(|_| "[]".to_owned()));
    output.push_str("\n```\n\n</details>\n");
    output
}
