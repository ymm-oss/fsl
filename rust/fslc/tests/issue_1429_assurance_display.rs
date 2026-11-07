// SPDX-License-Identifier: Apache-2.0

//! #1429 S1: ledger and HTML assurance displays require reported completeness
//! (`docs/design/DESIGN-assurance-classes.md`, "amendment #1429 S1"). The spec
//! model and `explain` skeleton come from the real CLI; each verification
//! envelope is a synthetic public-renderer input, not a native-verifier capture.
//! Expectations are literals from the accepted contract, never the production
//! classifier.

#[path = "support/ledger_findings.rs"]
#[allow(dead_code)]
mod findings;

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const SOURCE: &str = r#"spec DisplayContract {
  type N = 0..2
  state { x: N }
  init { x = 0 }
  action step() { requires x < 2 x = x + 1 }
  @requirement("REQ-INV") invariant Safe { x >= 0 }
  @requirement("REQ-TRANS") trans Stable { x >= old(x) }
  @requirement("REQ-LIVE") leadsTo Done { x == 0 ~> x == 1 }
  @requirement("REQ-TOP") reachable Top { x == 1 }
}"#;

const NO_REPORT: &str = "形式的根拠なし（not_run）。検査完了の根拠は報告されていない";
const ERROR: &str = "形式的根拠なし（not_run）。検証はエラーで終了した";
const UNREPORTED_DEPTH: &str = "有界検査の報告（bounded）。検査深さは報告されていない";
const REPLAY: &str = "観測済みログの照合のみ（replay-observed）。未観測の実行は保証しない";
const STATISTICAL: &str = "固定データセットに対する統計的裏付け（statistical）。全実行は保証しない";
const PROVED: &str = "k帰納法で **全実行を証明済み**（深さ無制限）";
const BOUNDED_3: &str =
    "BMC（有界モデル検査）: **深さ 3 までの全実行を網羅**。それ以遠の反例は本台帳の対象外";
const BOUNDED_0: &str =
    "BMC（有界モデル検査）: **深さ 0 までの全実行を網羅**。それ以遠の反例は本台帳の対象外";

/// Property order: invariant `Safe`, trans `Stable`, leadsTo `Done`, reachable `Top`.
const PROPERTIES: [(&str, &str); 4] = [
    ("REQ-INV", "Safe"),
    ("REQ-TRANS", "Stable"),
    ("REQ-LIVE", "Done"),
    ("REQ-TOP", "Top"),
];

struct Case {
    name: &'static str,
    input: Value,
    aggregate: &'static str,
    properties: [&'static str; 4],
    guarantee: &'static str,
}

#[rustfmt::skip]
fn cases() -> Vec<Case> {
    const PROVED_SAFETY: [&str; 4] = ["proved(induction)", "proved(induction)", "bounded", "bounded"];
    vec![
        // (a)/(b) negative controls: nothing reports a completed bounded check.
        Case { name: "missing", input: json!({}), aggregate: "not_run", properties: ["not_run"; 4], guarantee: NO_REPORT },
        Case { name: "method-and-depth-only", input: json!({"method":"induction","depth":3}), aggregate: "not_run", properties: ["not_run"; 4], guarantee: NO_REPORT },
        Case { name: "unknown-result", input: json!({"result":"unknown_future","checked_to_depth":3}), aggregate: "not_run", properties: ["not_run"; 4], guarantee: NO_REPORT },
        Case { name: "unknown-completeness", input: json!({"result":"unknown_budget","completeness":"unknown"}), aggregate: "not_run", properties: ["not_run"; 4], guarantee: NO_REPORT },
        Case { name: "top-unknown-masks-nested-unbounded", input: json!({"completeness":"unknown","kernel":{"completeness":"unbounded"}}), aggregate: "not_run", properties: ["not_run"; 4], guarantee: NO_REPORT },
        Case { name: "error", input: json!({"result":"error","kind":"semantics"}), aggregate: "not_run", properties: ["not_run"; 4], guarantee: ERROR },
        // #995 aggregate difference is out of scope and kept; the sentence still reports the error.
        Case { name: "error-with-bounded-completeness", input: json!({"result":"error","completeness":"bounded","checked_to_depth":3}), aggregate: "bounded(BMC depth 3)", properties: ["not_run"; 4], guarantee: ERROR },
        Case { name: "replay", input: json!({"result":"nonconformant"}), aggregate: "replay-observed", properties: ["not_run"; 4], guarantee: REPLAY },
        Case { name: "statistical", input: json!({"status":"statistically_unsupported"}), aggregate: "statistical", properties: ["not_run"; 4], guarantee: STATISTICAL },
        // Positive controls: reported bounded completeness keeps the BMC sentence and class.
        Case { name: "bounded-depth", input: json!({"completeness":"bounded","checked_to_depth":3}), aggregate: "bounded(BMC depth 3)", properties: ["bounded(BMC depth 3)"; 4], guarantee: BOUNDED_3 },
        Case { name: "bounded-secondary-depth-zero", input: json!({"completeness":"bounded","depth":0}), aggregate: "bounded(BMC depth 0)", properties: ["bounded(BMC depth 0)"; 4], guarantee: BOUNDED_0 },
        Case { name: "bounded-no-depth", input: json!({"completeness":"bounded"}), aggregate: "bounded", properties: ["bounded"; 4], guarantee: UNREPORTED_DEPTH },
        Case { name: "bounded-invalid-primary-masks-secondary", input: json!({"completeness":"bounded","checked_to_depth":null,"depth":3}), aggregate: "bounded", properties: ["bounded"; 4], guarantee: UNREPORTED_DEPTH },
        // The aggregate token reads nested kernel completeness; per-element classes read only the top level.
        Case { name: "nested-only-bounded", input: json!({"kernel":{"completeness":"bounded"},"checked_to_depth":3}), aggregate: "bounded(BMC depth 3)", properties: ["not_run"; 4], guarantee: BOUNDED_3 },
        // Existing proved/unbounded branches are unchanged.
        Case { name: "unbounded", input: json!({"completeness":"unbounded"}), aggregate: "proved(induction)", properties: PROVED_SAFETY, guarantee: PROVED },
        Case { name: "proved-without-completeness", input: json!({"result":"proved"}), aggregate: "not_run", properties: PROVED_SAFETY, guarantee: NO_REPORT },
        Case { name: "ranked-leads-to", input: json!({"completeness":"unbounded","leads_to":{"Done":{"completeness":"unbounded"}}}), aggregate: "proved(induction)", properties: ["proved(induction)", "proved(induction)", "proved(induction)", "bounded"], guarantee: PROVED },
    ]
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    text.split_once(start)
        .unwrap_or_else(|| panic!("missing {start:?}"))
        .1
        .split_once(end)
        .unwrap_or_else(|| panic!("missing {end:?} after {start:?}"))
        .0
}

fn html_property<'a>(html: &'a str, name: &str) -> &'a str {
    let section = between(html, "id=\"properties\"", "</section>");
    let row = section
        .split("<tr>")
        .find(|row| row.contains(&format!("<code>{name}</code>")))
        .unwrap_or_else(|| panic!("missing property row {name}"));
    row.split("<td>")
        .nth(3)
        .and_then(|cell| cell.split_once("</td>"))
        .expect("property assurance cell")
        .0
}

#[test]
fn ledger_and_html_assurance_require_reported_completeness() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target")
        .join(format!(
            "issue-1429-assurance-display-{}",
            std::process::id()
        ));
    fs::create_dir_all(&folder).unwrap();
    let spec = folder.join("spec.fsl");
    fs::write(&spec, SOURCE).unwrap();
    let model = fsl_core::build_model(
        fsl_core::parse_kernel_source(SOURCE, &fsl_core::FsResolver::new(&folder)).unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("explain")
        .arg(&spec)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let explained: Value = serde_json::from_slice(&output.stdout).unwrap();
    fs::remove_dir_all(&folder).unwrap();

    let mut failures = Vec::new();
    let mut check = |case: &str, field: &str, actual: &str, expected: &str| {
        if actual != expected {
            failures.push(format!(
                "{case}/{field}: actual={actual:?}, expected={expected:?}"
            ));
        }
    };
    for case in cases() {
        let ledger =
            fsl_tools::render_ledger("spec.fsl", &model, &case.input, &json!({}), None, &[]);
        let html =
            fsl_tools::render_html_report("spec.fsl", SOURCE, &model, &explained, &case.input, &[]);
        let guarantee = ledger
            .lines()
            .find_map(|line| line.strip_prefix("- 保証限界: "))
            .expect("ledger guarantee line");
        check(case.name, "guarantee", guarantee, case.guarantee);
        if case.guarantee != BOUNDED_3 && case.guarantee != BOUNDED_0 {
            check(
                case.name,
                "no BMC coverage claim",
                &ledger.contains("全実行を網羅").to_string(),
                "false",
            );
        }
        let aggregate = between(&html, "<th>Assurance</th><td>", "</td>");
        check(case.name, "html aggregate", aggregate, case.aggregate);
        for ((id, name), expected) in PROPERTIES.into_iter().zip(case.properties) {
            let ledger_class = findings::risk_row(&ledger, id)
                .split('|')
                .nth(4)
                .expect("ledger assurance column")
                .trim();
            check(case.name, &format!("ledger/{name}"), ledger_class, expected);
            check(
                case.name,
                &format!("html/{name}"),
                html_property(&html, name),
                expected,
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
