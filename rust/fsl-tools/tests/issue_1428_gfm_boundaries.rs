// SPDX-License-Identifier: Apache-2.0
//! Independent literal encodings calibrated by parent-owned actual GFM evidence.
//! These public-API tests execute no renderer/network and reuse no escaping oracle.

use fsl_syntax::{ParseOptions, UnstableFeature, UnstableFeatures};
use fsl_tools::{AppliedEvidence, Locale, RequirementClaimSet, Segment};
use serde_json::{Value, json};

const SOURCE: &str = "requirements Demo { state { x: Bool } init { x = false }\n@requirement(\"REQ-1\", \"plain\") action act() { x = true }\n}";
#[rustfmt::skip]
const DOMAINS: &[(&str, &str, &str)] = &[
    ("user@foo_bar.example.com", "user<span>@</span>foo&#95;bar.example.com", r"user<span>@</span>foo\_bar.example.com"),
    ("user@_foo.example.com", "user<span>@</span>&#95;foo.example.com", r"user<span>@</span>\_foo.example.com"),
    ("user@foo_.example.com", "user<span>@</span>foo&#95;.example.com", r"user<span>@</span>foo\_.example.com"),
    ("user@_.example.com", "user<span>@</span>&#95;.example.com", r"user<span>@</span>\_.example.com"),
];
fn document(
    source: &str,
    path: &str,
    files: &[(String, Value)],
    options: ParseOptions,
) -> (RequirementClaimSet, String) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let claims = fsl_tools::project_requirement_claims_from_source_with_options(
        source,
        Some(path),
        root,
        &options,
    )
    .expect("fixture projects, not semantic RED");
    let before = claims.clone();
    let kernel = fsl_core::parse_kernel_source_with_options(
        source,
        &fsl_core::FsResolver::new(root),
        &options,
    )
    .expect("fixture parses");
    let model = fsl_core::build_model(kernel.clone()).expect("fixture checks");
    let evidence = AppliedEvidence {
        files,
        digest: "sha256:boundary-evidence",
    };
    let doc = fsl_tools::render_requirements_document_with_options(
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
    .expect("fixture renders")
    .markdown;
    assert_eq!(claims, before, "complete input RCIR is immutable");
    (claims, doc)
}

fn stable(source: &str, path: &str, files: &[(String, Value)]) -> (RequirementClaimSet, String) {
    document(source, path, files, ParseOptions::stable())
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

fn blocks(md: &str) -> Vec<Segment> {
    fsl_tools::parse_generated_document(md)
        .expect("WHOLE document parses")
        .segments
        .into_iter()
        .filter(|s| matches!(s, Segment::Claim { .. }))
        .collect()
}

fn contains(failures: &mut Vec<String>, actual: &str, expected: &str) {
    if !actual.contains(expected) {
        failures.push(format!("expected {expected:?}; produced {actual:?}"));
    }
}

fn finish(failures: &[String]) {
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
fn email_slots(cases: &[(&str, &str, &str)]) {
    let mut failures = Vec::new();
    for &(raw, code, prose) in cases {
        let path = format!("{raw}|");
        let files = [(
            path.clone(),
            json!({"result":path, "detail":"raw **data**"}),
        )];
        let saved = files.clone();
        let (plain, plain_doc) = stable(SOURCE, "spec.fsl", &[]);
        let (claims, doc) = stable(SOURCE, "spec.fsl", &files);
        let expected = format!(
            "- <code>{code}&#124;</code> (result: <code>{code}&#124;</code>, matched requirement IDs: (whole specification; not attributed to an individual requirement))"
        );
        contains(&mut failures, &doc, &expected);
        assert_eq!(claims, plain);
        assert_eq!(blocks(&doc), blocks(&plain_doc)); // no claim-ID exclusions
        let report = ledger(&path, &files);
        contains(
            &mut failures,
            &report,
            &format!("<code>{code}&#124;</code>"),
        );
        contains(
            &mut failures,
            &report,
            &format!("| <code>{code}&#124;</code> | {prose}\\| | not_run | （仕様全体） |"),
        );
        let failed = json!({"result":"evidence_failed", "detail":raw});
        let summary = ledger("spec.fsl", &[(raw.into(), failed.clone())]);
        contains(
            &mut failures,
            &summary,
            &format!("反例要約: 外部証跡『{prose}』が非適合/失敗を報告(result: evidence_failed)"),
        );
        for (artifact, expected) in [(&report, json!([])), (&summary, json!([failed]))] {
            let (_, appendix) = artifact.split_once("```json\n").unwrap();
            let (raw_json, _) = appendix.split_once("\n```\n").unwrap();
            assert_eq!(serde_json::from_str::<Value>(raw_json).unwrap(), expected);
        }
        assert_eq!(files, saved, "all producer data preserved");
    }
    finish(&failures);
}

#[test]
fn underscore_domain_labels_in_document_and_ledger_slots() {
    email_slots(DOMAINS);
}
#[test]
fn underscore_domain_labels_in_authored_comments() {
    let mut failures = Vec::new();
    let options = ParseOptions::with_unstable(UnstableFeatures::NONE.with(UnstableFeature::Fsl5));
    for &(raw, code, _) in DOMAINS {
        let source = |comment: &str| {
            format!(
                "fsl 5\nrequirements Demo {{\n glossary {{ Draft = 下書き Approved = 承認済み User = 利用者 }}\n number Amount\n actor User\n entity Claim {{\n amount: Amount = 0\n lifecycle Draft -> Approved\n }}\n event approve = User approves Claim Draft -> Approved\n rule REQ-1 \"plain\" {{\n allow User approves Claim only when amount // {comment}|\n >= 0\n }}\n}}\nverify {{ instances Claim = 1 values Amount = 0..2 }}\n"
            )
        };
        let (claims, doc) = document(&source(raw), "spec.fsl", &[], options);
        let neutral = source(&"p".repeat(raw.len()));
        let (plain, control) = document(&neutral, "spec.fsl", &[], options);
        contains(
            &mut failures,
            &doc,
            &format!(
                "<code>allow User approves Claim only when amount // {code}&#124; &gt;= 0</code>"
            ),
        );
        assert_eq!(claims.claims, plain.claims);
        assert_eq!(blocks(&doc), blocks(&control));
    }
    finish(&failures);
}

fn intent_cases(cases: &[(&str, &str)]) {
    let mut failures = Vec::new();
    for &(intent, expected) in cases {
        let source = SOURCE.replace("plain", intent);
        let neutral = SOURCE.replace("plain", &"p".repeat(intent.len()));
        let (claims, doc) = stable(&source, "spec.fsl", &[]);
        let (control, plain) = stable(&neutral, "spec.fsl", &[]);
        contains(&mut failures, &doc, &format!("\n{expected}\n"));
        assert_eq!(claims.claims, control.claims);
        assert_eq!(blocks(&doc), blocks(&plain));
    }
    finish(&failures);
}

#[test]
fn deep_quoted_indent_uses_whole_raw_line_code_context() {
    intent_cases(&[
        ("    1. literal intent", "> `    1. literal intent`"),
        ("    1) literal intent", "> `    1) literal intent`"),
        ("    # heading", "> `    # heading`"),
        ("    - item", "> `    - item`"),
        ("    ~~tilde~~", "> `    ~~tilde~~`"),
        (
            "    <literal>&|`",
            "> <code>    &lt;literal&gt;&amp;&#124;&#96;</code>",
        ),
        ("    `tick`", "> <code>    &#96;tick&#96;</code>"),
        ("    a|b", "> <code>    a&#124;b</code>"),
        ("\t\t1. double tab intent", "> `\t\t1. double tab intent`"),
        ("  \t# literal heading", "> `  \t# literal heading`"),
        ("\t  + item", "> `\t  + item`"),
    ]);
}

#[test]
fn multiline_artifact_keeps_paragraph_blank_and_deep_quotes_separate() {
    // Genuine separate source statements: no LF-string admission or RCIR forgery.
    let source = SOURCE.replace("plain", "paragraph").replace("\n}",
        "\n@requirement(\"REQ-1\", \"\") action blank() { x = false }\n@requirement(\"REQ-1\", \"    1. literal intent\") action deep() { x = false }\n@requirement(\"REQ-1\", \"    <literal>&|`\") action html() { x = false }\n}");
    let neutral = source
        .replace(
            "    1. literal intent",
            &"p".repeat("    1. literal intent".len()),
        )
        .replace("    <literal>&|`", &"p".repeat("    <literal>&|`".len()));
    let (claims, doc) = stable(&source, "spec.fsl", &[]);
    let (plain, control) = stable(&neutral, "spec.fsl", &[]);
    assert_eq!(claims.claims, plain.claims);
    assert_eq!(blocks(&doc), blocks(&control));
    let expected = "> \n\n(Source: `spec.fsl:3`)\n\n> `    1. literal intent`\n\n(Source: `spec.fsl:4`)\n\n> <code>    &lt;literal&gt;&amp;&#124;&#96;</code>\n\n(Source: `spec.fsl:5`)\n\n> paragraph\n\n(Source: `spec.fsl:2`)";
    let mut failures = Vec::new();
    contains(&mut failures, &doc, expected);
    finish(&failures);
}

#[test]
fn benign_shallow_single_tab_and_inert_prefixes_unchanged() {
    intent_cases(&[
        ("1. intent", "> 1\\. intent"),
        (" 1) intent", ">  1\\) intent"),
        ("  1. intent", ">   1\\. intent"),
        ("   1. intent", ">    1\\. intent"),
        ("\t1. intent", "> \t1\\. intent"),
        (" \t# heading", ">  \t\\# heading"),
        ("1.item", "> 1.item"),
        ("1)item", "> 1)item"),
        ("1234567890. intent", "> 1234567890. intent"),
        ("1234567890) intent", "> 1234567890) intent"),
    ]);
}

#[test]
fn benign_normal_email_no_dot_and_input_html_unchanged() {
    #[rustfmt::skip]
    let cases = [
        ("user@example.com", "user<span>@</span>example.com", "user<span>@</span>example.com"),
        ("user@foo_bar", "user@foo&#95;bar", r"user@foo\_bar"),
        ("<span>@</span>", "&lt;span&gt;@&lt;/span&gt;", "&lt;span>@&lt;/span>"),
    ];
    email_slots(&cases);
}
