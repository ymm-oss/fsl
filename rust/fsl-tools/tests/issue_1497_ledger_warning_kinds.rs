// SPDX-License-Identifier: Apache-2.0

//! #1497: warning presentation is not the CLI's eight-kind vacuity policy.
//! Expectations are the approved row contract, independent of the product table.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use serde_json::{Value, json};

const BASE: &str = "e16f65bfeae773218d23f425376ea7c23b895bff";
const SPEC: &str = "spec WarningMatrix { state { x: Bool = false } \
    action flip() { x = not x } invariant Typed { x or not x } }";
const VACUITY: &[&str] = &[
    "vacuous_implication",
    "vacuous_leadsto",
    "tautology_over_frozen",
    "urgency_freeze",
    "vacuous_deadline",
    "always_true_requires",
];
const MIGRATION: &[&str] = &[
    "deprecated_domain_enum_union",
    "legacy_string_metadata",
    "legacy_quantifier_colon",
    "inline_action_maps",
    "implicit_initial_value",
];

struct Row {
    kind: &'static str,
    trace: &'static str,
    prefix: &'static str,
    translation: &'static str,
    advice: &'static str,
}

const ROWS: &[Row] = &[
    Row {
        kind: "deadlock",
        trace: "deadlock",
        prefix: "停止状態の到達",
        translation: "実行可能なアクションがない状態に到達する。意図した停止かを確認し、必要なら操作を追加するか terminal を宣言する。",
        advice: "停止の意図を確認 / アクション追加 / terminal 宣言",
    },
    Row {
        kind: "never_enabled_action",
        trace: "coverage",
        prefix: "深さ内で一度も実行可能にならない",
        translation: "アクション『Subject』が検証した深さ内で一度も実行可能にならない。ガードと前提操作を確認し、必要なら検証の深さを見直す。",
        advice: "ガード・前提アクションを確認 / 検証の深さを見直し",
    },
    Row {
        kind: "vacuity_probe_truncated",
        trace: "verification_incomplete",
        prefix: "空虚性未確立（到達性 probe が判定に至らず）",
        translation: "性質『Subject』の空虚性は未確立。到達性 probe が判定に至っていないため、状態空間と式の評価可否を確認する。",
        advice: "状態空間を簡素化 / 深さを下げる / 式の評価可否を確認",
    },
    Row {
        kind: "no_user_invariants",
        trace: "warning",
        prefix: "安全性の確認対象なし",
        translation: "利用者が記述した不変条件がなく、暗黙の型境界だけを検証している。守るべき業務ルールを確認する。",
        advice: "必要な不変条件を追加 / 検証対象を確認",
    },
    Row {
        kind: "fair_not_inherited",
        trace: "warning",
        prefix: "公平性の前提未継承",
        translation: "合成元の fair 指定が合成後に引き継がれていない。進行に必要な公平性の前提を確認する。",
        advice: "合成後の fair 指定を確認",
    },
    Row {
        kind: "unused_control",
        trace: "warning",
        prefix: "統制の参照なし",
        translation: "宣言した統制が参照されていない。統制と業務操作の対応を確認する。",
        advice: "統制の参照を追加 / 不要な統制を見直し",
    },
    Row {
        kind: "fullwidth_space",
        trace: "warning",
        prefix: "表記上の警告",
        translation: "全角空白が通常の空白として解釈されている。原文の表記を確認する。",
        advice: "全角空白を通常の空白に修正",
    },
    Row {
        kind: "non_nfc_identifier",
        trace: "warning",
        prefix: "識別子の正規化",
        translation: "識別子が NFC 正規形に正規化されている。意図した名前かを確認する。",
        advice: "識別子を NFC 正規形に修正",
    },
    Row {
        kind: "lifecycle_edge_without_event",
        trace: "warning",
        prefix: "イベントのない状態遷移",
        translation: "ライフサイクルの遷移に対応するイベントがない。業務上の遷移操作を確認する。",
        advice: "遷移に対応するイベントを確認",
    },
    Row {
        kind: "actor_shadows_member",
        trace: "warning",
        prefix: "名前の重なり",
        translation: "actor 名がメンバー名と重なっている。参照先が意図どおりかを確認する。",
        advice: "actor とメンバーの名前を見直し",
    },
    Row {
        kind: "implied_by_lifecycle",
        trace: "warning",
        prefix: "ライフサイクルによる含意",
        translation: "記述したルールはライフサイクルから既に導かれる。重複した記述が意図したものかを確認する。",
        advice: "ルールとライフサイクルの対応を確認",
    },
    Row {
        kind: "never_rule_without_forbidden",
        trace: "warning",
        prefix: "禁止フローの確認不足",
        translation: "禁止ルールに対応する forbidden がない。拒否すべき業務経路を具体例で確認する。",
        advice: "対応する forbidden を追加 / 禁止ルールを確認",
    },
    Row {
        kind: "decide_row_emptied",
        trace: "warning",
        prefix: "判断表の行が空",
        translation: "判断表の行に該当する場合が残っていない。条件の優先順位と重なりを確認する。",
        advice: "判断表の条件と優先順位を見直し",
    },
    Row {
        kind: "acceptance_skipped",
        trace: "warning",
        prefix: "受入シナリオ未評価",
        translation: "検証範囲の上書きにより受入シナリオが評価されていない。対象の値を含む範囲で再検証する。",
        advice: "instances / values の範囲を見直して再検証",
    },
    Row {
        kind: "forbidden_skipped",
        trace: "warning",
        prefix: "禁止フロー未評価",
        translation: "検証範囲の上書きにより禁止フローが評価されていない。対象の値を含む範囲で再検証する。",
        advice: "instances / values の範囲を見直して再検証",
    },
    Row {
        kind: "untagged",
        trace: "warning",
        prefix: "要件の紐付けなし",
        translation: "宣言に要件の注釈がない。要求との対応を確認する。",
        advice: "要件の注釈を追加 / モデル上の意図を記録",
    },
    Row {
        kind: "unreferenced_requirement",
        trace: "warning",
        prefix: "要件の参照なし",
        translation: "要件 ID を参照する宣言やシナリオがない。仕様への反映を確認する。",
        advice: "要件 ID を宣言・受入条件・禁止フローに紐付け",
    },
];

fn warning(kind: Option<Value>, hinted: bool, attributed: bool) -> Value {
    let mut value =
        json!({"name":"Subject", "message":"fixture message", "loc":{"line":3,"column":5}});
    if let Some(kind) = kind {
        value["kind"] = kind;
    }
    if hinted {
        value["hint"] = json!("fixture hint");
    }
    if attributed {
        value["requirement"] = json!({"id":"REQ-WARNING", "text":"warning is attributable"});
        value["requirements"] = json!([value["requirement"].clone()]);
    }
    value
}

fn render(warnings: &[Value], approvals: bool) -> String {
    let model = build_model(parse_kernel_source(SPEC, &FsResolver::new(".")).unwrap()).unwrap();
    fsl_tools::render_ledger_with_approvals(
        "warning-matrix.fsl",
        &model,
        &json!({"result":"verified", "completeness":"bounded", "checked_to_depth":2, "warnings":warnings}),
        &json!({}),
        None,
        &[],
        approvals.then(|| json!({"requirements":{}})).as_ref(),
    )
}

fn details(body: &str) -> String {
    body.lines()
        .filter(|line| line.starts_with("- **検出**:") || line.starts_with("  - "))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[allow(clippy::too_many_arguments)]
fn assert_row(
    kind: Option<Value>,
    trace: &str,
    summary: &str,
    translation: &str,
    advice: &str,
    hinted: bool,
    attributed: bool,
) {
    let warning = warning(kind, hinted, attributed);
    let body = render(std::slice::from_ref(&warning), false);
    let expected = format!(
        "- **検出**: `{trace}` — Subject\n  - 反例要約: {summary}\n  - 業務翻訳: {translation}\n  - 次アクション: {advice}\n"
    );
    assert_eq!(details(&body), expected);
    let raw = body
        .split("```json\n")
        .nth(1)
        .unwrap()
        .split("\n```")
        .next()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(raw).unwrap(),
        json!([warning])
    );
    let risk = body
        .lines()
        .find(|line| {
            line.starts_with(if attributed {
                "| REQ-WARNING |"
            } else {
                "| （仕様全体） |"
            })
        })
        .unwrap();
    let (status, risk_text) = if trace == "migration" {
        ("🟡 移行候補", "—")
    } else {
        ("🔴 要確認", "要確認")
    };
    let expected_risk = if attributed {
        // This synthetic ID has no formal element in the model; warnings do not confer assurance.
        format!(
            "| REQ-WARNING | warning is attributable | {status} | not_run | {trace} | {risk_text} | ____ | {advice} |"
        )
    } else {
        format!(
            "| （仕様全体） | 要件ID未付与の検出 | {status} | bounded(BMC depth 2) | {trace} | {risk_text} | ____ | 下記詳細 |"
        )
    };
    assert_eq!(risk, expected_risk);
}

fn check_kind(kind: &str) {
    for hinted in [false, true] {
        for attributed in [false, true] {
            let (trace, summary, translation, default) = if VACUITY.contains(&kind) {
                (
                    "vacuity",
                    format!("空虚性の疑い（{kind}）: fixture message"),
                    "性質『Subject』が空虚に成立している疑い（中身がない可能性）。`fslc mutate` で実効性を確認する。",
                    "fslc mutate で実効性を確認",
                )
            } else if MIGRATION.contains(&kind) {
                (
                    "migration",
                    format!("移行候補（{kind}）: fixture message"),
                    "検出種別 migration（付録の生 JSON を参照）。",
                    "責任者が対応方針を決定",
                )
            } else {
                let row = ROWS.iter().find(|row| row.kind == kind).unwrap();
                let summary = if kind == "vacuity_probe_truncated" {
                    format!("{}: fixture message", row.prefix)
                } else {
                    format!("{}（{kind}）: fixture message", row.prefix)
                };
                (row.trace, summary, row.translation, row.advice)
            };
            let advice = if hinted && trace != "migration" {
                "fixture hint"
            } else {
                default
            };
            assert_row(
                Some(json!(kind)),
                trace,
                &summary,
                translation,
                advice,
                hinted,
                attributed,
            );
        }
    }
}

macro_rules! kind_tests {
    ($($kind:ident),+ $(,)?) => { $(#[test] fn $kind() { check_kind(stringify!($kind)); })+ };
}
kind_tests!(
    vacuous_implication,
    vacuous_leadsto,
    tautology_over_frozen,
    urgency_freeze,
    vacuous_deadline,
    always_true_requires,
    deprecated_domain_enum_union,
    legacy_string_metadata,
    legacy_quantifier_colon,
    inline_action_maps,
    implicit_initial_value,
    deadlock,
    never_enabled_action,
    vacuity_probe_truncated,
    no_user_invariants,
    fair_not_inherited,
    unused_control,
    fullwidth_space,
    non_nfc_identifier,
    lifecycle_edge_without_event,
    actor_shadows_member,
    implied_by_lifecycle,
    never_rule_without_forbidden,
    decide_row_emptied,
    acceptance_skipped,
    forbidden_skipped,
    untagged,
    unreferenced_requirement
);

fn fallback(kind: Option<&Value>, label: &str) {
    for hinted in [false, true] {
        for attributed in [false, true] {
            assert_row(
                kind.cloned(),
                "warning",
                &format!("未分類の警告（{label}）: fixture message"),
                "警告の分類が未登録。付録の生 JSON と警告内容を確認し、責任者が対応方針を決定する。",
                if hinted {
                    "fixture hint"
                } else {
                    "警告内容と分類を確認"
                },
                hinted,
                attributed,
            );
        }
    }
}
#[test]
fn unknown_kind_is_generic() {
    fallback(Some(&json!("future_warning")), "future_warning");
}
#[test]
fn missing_kind_is_generic() {
    fallback(None, "不明");
}
#[test]
fn non_string_kind_is_generic() {
    fallback(Some(&json!(17)), "不明");
}
#[test]
fn null_kind_is_generic() {
    fallback(Some(&Value::Null), "不明");
}

fn captured_outputs() -> BTreeMap<String, String> {
    let mut outputs = BTreeMap::new();
    for kind in VACUITY.iter().chain(MIGRATION) {
        for hinted in [false, true] {
            for attributed in [false, true] {
                let warning = warning(Some(json!(kind)), hinted, attributed);
                outputs.insert(
                    format!("{kind}/hint={hinted}/requirement={attributed}"),
                    render(&[warning], false),
                );
            }
        }
    }
    // Mixed migration/vacuity and approval overlays must retain their current red status.
    for approvals in [false, true] {
        for attributed in [false, true] {
            outputs.insert(
                format!("mixed/approvals={approvals}/requirement={attributed}"),
                render(
                    &[
                        warning(Some(json!("legacy_string_metadata")), true, attributed),
                        warning(Some(json!("vacuous_implication")), true, attributed),
                    ],
                    approvals,
                ),
            );
        }
    }
    outputs
}

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/issue_1497_preserved_ledger.json")
}

#[test]
fn preserved_families_match_captured_main_bytes() {
    let captured: Value =
        serde_json::from_str(&std::fs::read_to_string(baseline_path()).unwrap()).unwrap();
    assert_eq!(captured["base_sha"], BASE);
    let expected: BTreeMap<String, String> =
        serde_json::from_value(captured["outputs"].clone()).unwrap();
    assert_eq!(expected.len(), 48);
    assert_eq!(
        captured_outputs(),
        expected,
        "complete artifacts, no exclusions"
    );
}

#[test]
#[ignore = "one-time capture on the named base; never update to bless changed output"]
fn capture_main_preservation_bytes() {
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(head.status.success());
    assert_eq!(String::from_utf8(head.stdout).unwrap().trim(), BASE);
    assert!(
        !baseline_path().exists(),
        "do not overwrite baseline evidence"
    );
    let captured = json!({"base_sha":BASE, "outputs":captured_outputs()});
    std::fs::write(
        baseline_path(),
        serde_json::to_string_pretty(&captured).unwrap() + "\n",
    )
    .unwrap();
}
