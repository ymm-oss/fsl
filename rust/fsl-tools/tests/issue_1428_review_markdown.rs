// SPDX-License-Identifier: Apache-2.0

//! Finite reviewed corrections: literal HTML-code data, the old marker refusal
//! boundary, and free producer text in ledger cells. Byte oracles, not a claim
//! that a `CommonMark` renderer was executed. No production escaping is reused.

use fsl_syntax::{ParseOptions, UnstableFeature, UnstableFeatures};
use fsl_tools::{AppliedEvidence, Locale};
use serde_json::{Value, json};

const SOURCE: &str = "requirements Demo { state { x: Bool } init { x = false }\n@requirement(\"REQ-1\", \"plain\") action act() { x = true }\n}";

fn document(path: &str, files: &[(String, Value)]) -> Result<String, String> {
    document_source(SOURCE, path, files, ParseOptions::stable())
}

fn document_source(
    source: &str,
    path: &str,
    files: &[(String, Value)],
    options: ParseOptions,
) -> Result<String, String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let claims = fsl_tools::project_requirement_claims_from_source_with_options(
        source,
        Some(path),
        root,
        &options,
    )
    .expect("accepted source projects");
    let kernel = fsl_core::parse_kernel_source_with_options(
        source,
        &fsl_core::FsResolver::new(root),
        &options,
    )
    .expect("accepted source parses");
    let model = fsl_core::build_model(kernel.clone()).expect("accepted source checks");
    let evidence = AppliedEvidence {
        files,
        digest: "sha256:review-evidence",
    };
    fsl_tools::render_requirements_document_with_options(
        &claims,
        &kernel,
        &model,
        source,
        Locale::En,
        None,
        (!files.is_empty()).then_some(&evidence),
        None,
        &options,
    )
    .map(|doc| doc.markdown)
}

fn ledger(path: &str, files: &[(String, Value)]) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let kernel = fsl_core::parse_kernel_source(SOURCE, &fsl_core::FsResolver::new(root)).unwrap();
    let model = fsl_core::build_model(kernel).unwrap();
    fsl_tools::render_ledger(
        path,
        &model,
        &json!({"result":"verified", "checked_to_depth":2}),
        &json!({}),
        None,
        files,
    )
}

#[test]
fn r1_document_mixed_path_is_literal() {
    let mut failures = Vec::new();
    for (path, expected) in [
        ("a&*draft*.fsl", "<code>a&amp;&#42;draft&#42;.fsl:2</code>"),
        ("a|_x_.fsl", "<code>a&#124;&#95;x&#95;.fsl:2</code>"),
        (
            "a`[link](#x)![image](x)\\.fsl",
            "<code>a&#96;&#91;link&#93;(#x)&#33;&#91;image&#93;(x)&#92;.fsl:2</code>",
        ),
    ] {
        let doc = document(path, &[]).expect("mixed path still renders");
        if !doc.contains(expected) {
            failures.push(format!(
                "expected {expected:?}; produced {:?}",
                doc.lines()
                    .filter(|line| line.contains("Source:"))
                    .collect::<Vec<_>>()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn r1_ledger_mixed_path_is_literal() {
    let report = ledger("a&*draft*|_x_`[link](#x)![image](x)\\.fsl", &[]);
    let expected = "<code>a&amp;&#42;draft&#42;&#124;&#95;x&#95;&#96;&#91;link&#93;(#x)&#33;&#91;image&#93;(x)&#92;.fsl</code>";
    assert!(
        report.contains(expected),
        "expected {expected:?}; produced {report}"
    );
}

#[test]
fn r1_authored_rule_comment_is_literal() {
    // A genuine authored line comment; the actual LF ends it before >= 0.
    let source = "fsl 5\nrequirements Demo {\n  glossary { Draft = 下書き Approved = 承認済み User = 利用者 }\n  number Amount\n  actor User\n  entity Claim {\n    amount: Amount = 0\n    lifecycle Draft -> Approved\n  }\n  event approve = User approves Claim Draft -> Approved\n  rule REQ-1 \"ordinary intent\" {\n    allow User approves Claim only when amount // *must*| _x_ [link](#x) ![image](x) \\ & `tick`\n      >= 0\n  }\n}\nverify { instances Claim = 1 values Amount = 0..2 }\n";
    let options = ParseOptions::with_unstable(UnstableFeatures::NONE.with(UnstableFeature::Fsl5));
    let doc = document_source(source, "overview.fsl", &[], options).unwrap();
    let expected = "<code>allow User approves Claim only when amount // &#42;must&#42;&#124; &#95;x&#95; &#91;link&#93;(#x) &#33;&#91;image&#93;(x) &#92; &amp; &#96;tick&#96; &gt;= 0</code>";
    assert!(
        doc.contains(expected),
        "expected literal authored rule {expected:?}; produced {doc}"
    );
}

#[test]
fn r2_single_inline_marker_result_remains_accepted() {
    let files = [(
        "evidence.json".into(),
        json!({"result":"<!-- fsl:bogus -->"}),
    )];
    let doc = document("spec.fsl", &files).expect("first inline fragment is not a marker line");
    assert!(
        doc.contains("<code>&lt;!-- fsl:bogus --&gt;</code>"),
        "{doc}"
    );
}

#[test]
fn r2_first_inline_fragment_before_actual_lf_remains_accepted() {
    let files = [(
        "evidence.json".into(),
        json!({"result":"<!-- fsl:bogus -->\nordinary"}),
    )];
    let doc = document("spec.fsl", &files).expect("only subsequent standalone marker lines refuse");
    assert!(
        doc.contains("<code>&lt;!-- fsl:bogus --&gt;&#10;ordinary</code>"),
        "{doc}"
    );
}

#[test]
fn r2_subsequent_standalone_marker_still_refuses() {
    for result in [
        "verified\n<!-- fsl:bogus -->\n",
        "<!-- fsl:bogus -->\n<!-- fsl:bogus -->\n",
    ] {
        let files = [("evidence.json".into(), json!({"result":result}))];
        let error = document("spec.fsl", &files).expect_err("actual LF exposes malformed marker");
        assert_eq!(
            error,
            "malformed generated-block marker: \"<!-- fsl:bogus -->\""
        );
    }
}

#[test]
fn r2_literal_backslash_n_is_not_a_standalone_marker() {
    let files = [(
        "evidence.json".into(),
        json!({"result":r"verified\n<!-- fsl:bogus -->\n"}),
    )];
    let doc = document("spec.fsl", &files).expect("literal backslash-n is not LF");
    assert!(doc.contains("verified"), "{doc}");
    assert!(
        !doc.contains("&#10;"),
        "literal backslash-n became a line break: {doc}"
    );
}

fn raw_findings(report: &str) -> Value {
    let raw = report
        .split_once("```json\n")
        .unwrap()
        .1
        .split_once("\n```\n")
        .unwrap()
        .0;
    serde_json::from_str(raw).unwrap()
}

#[test]
fn r3_arbitrary_producer_result_is_literal() {
    let item = json!({"result":"**verified** _x_ [link](#x) ![image](x) \\ & | `tick`"});
    let files = [("evidence.json".into(), item.clone())];
    let report = ledger("spec.fsl", &files);
    // Unknown result remains unclassified and does not manufacture findings.
    assert_eq!(raw_findings(&report), json!([]));
    assert_eq!(files[0].1, item);
    let expected = r"| `evidence.json` | \*\*verified\*\* \_x\_ \[link\](#x) !\[image\](x) \\ &amp; \| \`tick\` | not_run | （仕様全体） |";
    assert!(
        report.contains(expected),
        "expected literal result {expected:?}; produced {report}"
    );
}

#[test]
fn r3_actual_external_finding_summary_is_literal() {
    let item = json!({"result":"evidence_failed", "detail":"unchanged raw **data**"});
    let files = [("**audit**_[link](#x)!.json".into(), item.clone())];
    let report = ledger("spec.fsl", &files);
    assert_eq!(raw_findings(&report), json!([item]));
    let expected = r"  - 反例要約: 外部証跡『\*\*audit\*\*\_\[link\](#x)!.json』が非適合/失敗を報告(result: evidence_failed)";
    assert!(
        report.contains(expected),
        "expected literal summary {expected:?}; produced {report}"
    );
}

#[test]
fn ordinary_backtick_branch_preserves_literal_markdown() {
    let path = "a*_[link](x)!.fsl";
    let doc = document(path, &[]).unwrap();
    assert!(doc.contains("`a*_[link](x)!.fsl:2`"), "{doc}");
    let report = ledger(path, &[]);
    assert!(report.contains("`a*_[link](x)!.fsl`"), "{report}");
}

#[test]
fn benign_ledger_whole_output_preservation() {
    // A fixed full-artifact fingerprint is filled from the reviewed baseline,
    // not used as an oracle for hostile-data corrections.
    let report = ledger(
        "spec.fsl",
        &[("evidence.json".into(), json!({"result":"verified"}))],
    );
    assert_eq!(
        fsl_tools::framed_text_digest("1428-review-benign-ledger", &report),
        "sha256:b9b53c2819397f5594714570f706333cb469a029a8d576b8fa78273ada82a8f2"
    );
}
