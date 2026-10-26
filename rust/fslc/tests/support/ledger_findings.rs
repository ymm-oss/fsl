// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

const EMPTY_FINDINGS: &str = "検出された意図ずれ・死経路・禁止経路はありません";

pub fn section<'a>(body: &'a str, heading: &str) -> &'a str {
    body.split_once(&format!("\n## {heading}\n"))
        .unwrap_or_else(|| panic!("missing section {heading}: {body}"))
        .1
        .split("\n## ")
        .next()
        .expect("section body")
}

pub fn risk_row<'a>(body: &'a str, id: &str) -> &'a str {
    section(body, "リスク一覧（要件ID別）")
        .lines()
        .find(|line| line.starts_with(&format!("| {id} |")))
        .unwrap_or_else(|| panic!("missing risk row {id}: {body}"))
}

pub fn assert_inline_row(body: &str, abs: &str, result: &str) {
    assert_result_row(body, "inline implements", abs, result, 2);
}

pub fn assert_preservation_row(body: &str, name: &str, result: &str) {
    assert_result_row(body, "governance preservation", name, result, 4);
}

fn assert_result_row(body: &str, heading: &str, name: &str, result: &str, column: usize) {
    let table = section(body, heading);
    let row = table
        .lines()
        .find(|line| line.starts_with(&format!("| `{name}` |")))
        .unwrap_or_else(|| panic!("missing {heading} row {name}: {table}"));
    let cells = row.split('|').map(str::trim).collect::<Vec<_>>();
    let mark = if result == "refines" { "🟢" } else { "🔴" };
    assert_eq!(cells[1], format!("`{name}`"), "{row}");
    assert_eq!(cells[column], format!("{mark} `{result}`"), "{row}");
}

pub fn assert_issue<'a>(body: &'a str, id: &str, tokens: &[&str]) -> &'a str {
    let row = risk_row(body, id);
    assert!(row.contains("🔴 要確認"), "expected red {id}: {row}");
    let details = section(body, "要件ID別詳細");
    assert!(!details.contains(EMPTY_FINDINGS), "{details}");
    let detail = details
        .split_once(&format!("### {id}"))
        .unwrap_or_else(|| panic!("missing detail {id}: {details}"))
        .1
        .split("\n### ")
        .next()
        .expect("requirement detail");
    for token in tokens {
        assert!(detail.contains(token), "missing {token} in {id}: {detail}");
    }
    detail
}

pub fn raw_findings(body: &str) -> Value {
    let appendix = section(body, "付録: 生 JSON 反例（証跡）");
    let raw = appendix
        .split_once("```json\n")
        .expect("raw findings JSON fence")
        .1
        .split_once("\n```")
        .expect("raw findings fence end")
        .0;
    let value: Value = serde_json::from_str(raw).expect("raw findings JSON");
    assert!(value.is_array(), "raw findings array: {value}");
    value
}

fn contains_value(value: &Value, expected: &Value) -> bool {
    value == expected
        || match value {
            Value::Array(items) => items.iter().any(|item| contains_value(item, expected)),
            Value::Object(fields) => fields.values().any(|item| contains_value(item, expected)),
            _ => false,
        }
}

pub fn assert_raw_contains(body: &str, expected: &Value) {
    let raw = raw_findings(body);
    assert!(
        contains_value(&raw, expected),
        "missing complete evidence {expected} in raw findings: {raw}"
    );
}

pub fn assert_no_findings(body: &str) {
    let details = section(body, "要件ID別詳細");
    assert!(details.contains(EMPTY_FINDINGS), "{details}");
    assert!(!details.contains("**検出**"), "{details}");
    assert_eq!(raw_findings(body), serde_json::json!([]), "{body}");
    assert!(
        !section(body, "リスク一覧（要件ID別）").contains("| （仕様全体） |"),
        "unexpected spec-level finding: {body}"
    );
}

pub fn assert_uncertain(detail: &str) {
    assert!(detail.contains("未確立"), "expected uncertainty: {detail}");
    for forbidden in ["反例", "違反"] {
        assert!(
            !detail.contains(forbidden),
            "CTI is not an established violation: {detail}"
        );
    }
}

#[test]
fn inline_row_rejects_a_result_from_another_row() {
    let body = "\n## inline implements\n\n| abs | result |\n|---|---|\n\
                | `Wanted` | 🟢 `refines` |\n| `Decoy` | 🔴 `refinement_failed` |\n";
    let failure = std::panic::catch_unwind(|| {
        assert_inline_row(body, "Wanted", "refinement_failed");
    })
    .expect_err("another row must not satisfy the wanted row's verdict");
    let message = failure.downcast_ref::<String>().expect("assertion message");
    assert!(message.contains("left == right"), "{message}");
}
