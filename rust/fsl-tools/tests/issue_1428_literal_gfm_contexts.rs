// SPDX-License-Identifier: Apache-2.0

//! Literal byte oracles for parent-reproduced GitHub GFM cases, not an embedded
//! Markdown renderer. Numeric/backslash @ still autolinks there; the constant
//! trusted <span>@</span> boundary preserves email text without trusting input HTML.
//! Legacy accept/refuse oracles come from the base82 CLI, not today's formatter.

use fsl_syntax::ParseOptions;
use fsl_tools::{AppliedEvidence, Locale, RequirementClaimSet, Segment};
use serde_json::{Value, json};

const SOURCE: &str = "requirements Demo { state { x: Bool } init { x = false }\n@requirement(\"REQ-1\", \"plain\") action act() { x = true }\n}";
// Explicit raw / HTML-code / prose triples; never call production escaping as an oracle.
#[rustfmt::skip]
const LITERALS: &[(&str, &str, &str)] = &[
    ("~~verified~~", "&#126;&#126;verified&#126;&#126;", r"\~\~verified\~\~"),
    ("http://example.com", "http&#58;//example.com", "http&#58;//example.com"),
    ("https://example.com/?x=1&y=2", "https&#58;//example.com/?x=1&amp;y=2", "https&#58;//example.com/?x=1&amp;y=2"),
    ("ftp://example.com", "ftp&#58;//example.com", "ftp&#58;//example.com"),
    ("HTTP://example.com", "HTTP&#58;//example.com", "HTTP&#58;//example.com"),
    ("HTTPS://example.com", "HTTPS&#58;//example.com", "HTTPS&#58;//example.com"),
    ("FTP://example.com", "FTP&#58;//example.com", "FTP&#58;//example.com"),
    ("www.example.com", "www&#46;example.com", "www&#46;example.com"),
    ("WWW.example.com", "WWW&#46;example.com", "WWW&#46;example.com"),
    ("WwW.example.com", "WwW&#46;example.com", "WwW&#46;example.com"),
    ("user@example.com", "user<span>@</span>example.com", "user<span>@</span>example.com"),
    ("user+tag@example.com", "user+tag<span>@</span>example.com", "user+tag<span>@</span>example.com"),
];

fn document(
    source: &str,
    path: &str,
    files: &[(String, Value)],
    options: ParseOptions,
) -> (RequirementClaimSet, Result<String, String>) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let claims = fsl_tools::project_requirement_claims_from_source_with_options(
        source,
        Some(path),
        root,
        &options,
    )
    .expect("fixture source must project (not a semantic RED)");
    let kernel = fsl_core::parse_kernel_source_with_options(
        source,
        &fsl_core::FsResolver::new(root),
        &options,
    )
    .expect("fixture source must parse (not a semantic RED)");
    let model = fsl_core::build_model(kernel.clone()).expect("fixture source must check");
    let evidence = AppliedEvidence {
        files,
        digest: "sha256:literal-gfm-evidence",
    };
    let rendered = fsl_tools::render_requirements_document_with_options(
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
    .map(|doc| doc.markdown);
    (claims, rendered)
}

fn stable(path: &str, files: &[(String, Value)]) -> (RequirementClaimSet, Result<String, String>) {
    document(SOURCE, path, files, ParseOptions::stable())
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
        .expect("whole artifact must parse")
        .segments
        .into_iter()
        .filter(|s| matches!(s, Segment::Claim { .. }))
        .collect()
}

fn contains(failures: &mut Vec<String>, label: &str, actual: &str, expected: &str) {
    if !actual.contains(expected) {
        // Diagnostics only; comparisons above still use the complete artifact.
        let slots: Vec<_> = actual
            .lines()
            .filter(|line| {
                line.contains("code>")
                    || line.contains("| `")
                    || line.contains("反例要約")
                    || line.starts_with("> ")
            })
            .collect();
        failures.push(format!(
            "{label}: expected {expected:?}; produced slots {slots:?}"
        ));
    }
}

fn finish(failures: &[String]) {
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn gfm_code_paths_and_document_results_are_literal() {
    let mut failures = Vec::new();
    for &(raw, code, _) in LITERALS {
        let path = format!("prefix {raw}|"); // trailing entity must not be swallowed by www autolinking
        let (claims, rendered) = stable(&path, &[]);
        let doc = rendered.expect("ordinary path must render");
        let expected = format!("<code>prefix {code}&#124;:2</code>");
        contains(&mut failures, &path, &doc, &expected);
        let report = ledger(&path, &[]);
        let expected = format!("<code>prefix {code}&#124;</code>");
        contains(&mut failures, &path, &report, &expected);
        let files = [(
            "evidence.json".into(),
            json!({"result":format!("{raw}|<>&")}),
        )];
        let (with_evidence, result) = stable(&path, &files);
        let result = result.expect("nonmarker result must render");
        let expected = format!("<code>{code}&#124;&lt;&gt;&amp;</code>");
        contains(&mut failures, raw, &result, &expected);
        assert_eq!(claims, with_evidence); // EVERY RCIR field
        assert_eq!(blocks(&doc), blocks(&result)); // complete ID/body/digest records
    }
    finish(&failures);
}

#[test]
fn gfm_authored_rule_comments_are_literal() {
    let mut failures = Vec::new();
    let options = ParseOptions::stable();
    for &(raw, code, _) in LITERALS {
        let comment = format!("{raw}| ~~must~~ <&> `tick`");
        let source = |comment: &str| {
            format!(
                "fsl 5\nrequirements Demo {{\n  glossary {{ Draft = 下書き Approved = 承認済み User = 利用者 }}\n  number Amount\n  actor User\n  entity Claim {{\n    amount: Amount = 0\n    lifecycle Draft -> Approved\n  }}\n  event approve = User approves Claim Draft -> Approved\n  rule REQ-1 \"ordinary intent\" {{\n    allow User approves Claim only when amount // {comment}\n      >= 0\n  }}\n}}\nverify {{ instances Claim = 1 values Amount = 0..2 }}\n"
            )
        };
        let (claims, doc) = document(&source(&comment), "overview.fsl", &[], options);
        let doc = doc.expect("genuine line comment must render");
        let expected = format!(
            "<code>allow User approves Claim only when amount // {code}&#124; &#126;&#126;must&#126;&#126; &lt;&amp;&gt; &#96;tick&#96; &gt;= 0</code>"
        );
        contains(&mut failures, raw, &doc, &expected);
        let neutral = "p".repeat(comment.len()); // identical UTF-8 width, lines and following offsets
        let (plain, plain_doc) = document(&source(&neutral), "overview.fsl", &[], options);
        assert_eq!(claims.claims, plain.claims); // all normative claims, not selected fields
        assert_eq!(blocks(&doc), blocks(&plain_doc.unwrap()));
    }
    finish(&failures);
}

#[test]
fn gfm_producer_results_and_external_summaries_are_literal() {
    let mut failures = Vec::new();
    for &(raw, _, prose) in LITERALS {
        let item = json!({"result":format!("{raw} <>&"), "detail":"untouched raw **data**"});
        let files = [("evidence.json".into(), item.clone())];
        let report = ledger("spec.fsl", &files);
        let expected = format!("| `evidence.json` | {prose} &lt;>&amp; | not_run | （仕様全体） |");
        contains(&mut failures, raw, &report, &expected);
        let failed = json!({"result":"evidence_failed", "detail":raw});
        let summary = ledger(
            "spec.fsl",
            &[(format!("{raw} ~~audit~~ <>&.json"), failed.clone())],
        );
        let expected = format!(
            "反例要約: 外部証跡『{prose} \\~\\~audit\\~\\~ &lt;>&amp;.json』が非適合/失敗を報告(result: evidence_failed)"
        );
        contains(&mut failures, raw, &summary, &expected);
        for (artifact, expected) in [(&report, json!([])), (&summary, json!([failed]))] {
            let raw_json = artifact
                .split_once("```json\n")
                .unwrap()
                .1
                .split_once("\n```\n")
                .unwrap()
                .0;
            assert_eq!(
                serde_json::from_str::<Value>(raw_json).unwrap(),
                expected,
                "no classification/raw finding drift"
            );
        }
        assert_eq!(files[0].1, item, "producer bytes are immutable");
    }
    finish(&failures);
}

#[test]
fn all_html_fallback_selectors_contain_gfm() {
    let mut failures = Vec::new();
    for (trigger, encoded) in [
        ("|", "&#124;"),
        ("&", "&amp;"),
        ("<", "&lt;"),
        (">", "&gt;"),
        ("\r", "&#13;"),
        ("`", "&#96;"),
    ] {
        let path = format!("a{trigger}~~must~~.fsl");
        let doc = stable(&path, &[])
            .1
            .expect("fallback selector is valid path data");
        let expected = format!("<code>a{encoded}&#126;&#126;must&#126;&#126;.fsl:2</code>");
        contains(&mut failures, trigger, &doc, &expected);
    }
    finish(&failures);
}

#[test]
fn ordinary_backticks_and_nontrigger_punctuation_are_preserved() {
    for &(raw, _, _) in LITERALS {
        let doc = stable(raw, &[]).1.unwrap();
        // Only ampersand selects fallback in this corpus; short emails need no span.
        if !raw.contains('&') {
            assert!(doc.contains(&format!("`{raw}:2`")), "{doc}");
            assert!(ledger(raw, &[]).contains(&format!("`{raw}`")));
        }
    }
    for text in [
        "note:ordinary.version",
        "http:/example.com",
        "https:example.com",
        "ftp:local",
        "ww.example.com",
        "wwwx.example.com",
        "identifier_123",
        "inert!()=+;,@",
    ] {
        let files = [("evidence.json".into(), json!({"result":text}))];
        let report = ledger("spec.fsl", &files);
        assert!(
            report.contains(&format!(
                "| `evidence.json` | {text} | not_run | （仕様全体） |"
            )),
            "{report}"
        );
        let doc = stable(&format!("{text}|"), &[]).1.unwrap();
        // Intraword underscores are encoded in HTML code, but stay inert prose identifiers.
        let expected = if text == "identifier_123" {
            "identifier&#95;123"
        } else {
            text
        };
        assert!(
            doc.contains(&format!("<code>{expected}&#124;:2</code>")),
            "{doc}"
        );
    }
    let doc = stable("<span>@</span>&|.fsl", &[]).1.unwrap();
    assert!(
        doc.contains("<code>&lt;span&gt;@&lt;/span&gt;&amp;&#124;.fsl:2</code>"),
        "input HTML is data: {doc}"
    );
}

#[test]
fn ordered_source_intent_is_literal_and_claims_are_unchanged() {
    let mut failures = Vec::new();
    for (intent, expected) in [
        ("1) literal intent", "> 1\\) literal intent"),
        ("1. literal intent", "> 1\\. literal intent"),
        ("  12) literal intent", ">   12\\) literal intent"),
        ("   12. literal intent", ">    12\\. literal intent"),
    ] {
        let source = SOURCE.replace("plain", intent);
        let (claims, doc) = document(&source, "spec.fsl", &[], ParseOptions::stable());
        let doc = doc.expect("ordered intent must parse/check/render");
        contains(&mut failures, intent, &doc, expected);
        let neutral = SOURCE.replace("plain", &"p".repeat(intent.len()));
        let (control, control_doc) = document(&neutral, "spec.fsl", &[], ParseOptions::stable());
        assert_eq!(claims.claims, control.claims); // entire claims, not selected IDs
        assert_eq!(blocks(&doc), blocks(&control_doc.unwrap()));
        assert_ne!(claims.spec.claim_set_digest, control.spec.claim_set_digest);
    }
    finish(&failures);
}

#[test]
fn inert_ordered_prefixes_and_existing_quote_controls_are_preserved() {
    for (intent, expected) in [
        ("1.item", "> 1.item"),
        ("1)item", "> 1)item"),
        ("1234567890. literal intent", "> 1234567890. literal intent"),
        ("1234567890) literal intent", "> 1234567890) literal intent"),
        ("# heading", "> \\# heading"),
        ("- item", "> \\- item"),
        ("+ item", "> \\+ item"),
        ("> quote", "> \\> quote"),
        ("``` fence", "> \\`\\`\\` fence"),
        ("", "> \n"),
    ] {
        let doc = document(
            &SOURCE.replace("plain", intent),
            "spec.fsl",
            &[],
            ParseOptions::stable(),
        )
        .1
        .unwrap();
        assert!(
            doc.contains(expected),
            "expected {expected:?}; produced {doc}"
        );
    }
}

const BEGIN: &str = "verified\n<!-- fsl:claim begin id=\"x\" digest=\"x\" -->\n";
const END: &str = "verified\n<!-- fsl:claim end -->\n";
const REFUSAL: &str = "malformed generated-block marker: \"<!-- fsl:claim end -->\"";

#[test]
fn legacy_cross_row_accept_and_refuse_use_raw_path_order() {
    let mut failures = Vec::new();
    let (plain_claims, plain_doc) = stable("spec.fsl", &[]);
    let plain_doc = plain_doc.unwrap();
    let normative = blocks(&plain_doc);
    for path in ["b|.json", "b<.json", "b`.json", "b.json"] {
        // Literal old raw rows, not a reimplementation of the production row helper.
        let legacy_rows = format!(
            "\n- `a.json` (result: `verified\n<!-- fsl:claim begin id=\"x\" digest=\"x\" -->\n`, matched requirement IDs: (whole specification; not attributed to an individual requirement))\n- `{path}` (result: `verified\n<!-- fsl:claim end -->\n`, matched requirement IDs: (whole specification; not attributed to an individual requirement))\n"
        );
        let old = blocks(&format!("{plain_doc}{legacy_rows}"));
        let fake = Segment::Claim {
            id: "x".into(),
            digest: "x".into(),
            body: format!(
                "`, matched requirement IDs: (whole specification; not attributed to an individual requirement))\n- `{path}` (result: `verified"
            ),
        };
        assert_eq!(old, [normative.clone(), vec![fake]].concat()); // fake DATA claim present
        for accept in [true, false] {
            for third in [false, true] {
                for reverse in [false, true] {
                    let mut files = vec![
                        (
                            "a.json".into(),
                            json!({"result":if accept { BEGIN } else { END }}),
                        ),
                        (
                            path.into(),
                            json!({"result":if accept { END } else { BEGIN }}),
                        ),
                    ];
                    if third {
                        files.push(("c.json".into(), json!({"result":"verified"})));
                    }
                    if reverse {
                        files.reverse();
                    }
                    let label = format!("{path} accept={accept} third={third} reverse={reverse}");
                    let (claims, result) = stable("spec.fsl", &files);
                    assert_eq!(claims, plain_claims, "all RCIR fields unchanged");
                    match (accept, result) {
                        (true, Ok(doc)) => {
                            assert_eq!(blocks(&doc), normative); // fake x absent, NO ID filter
                            contains(
                                &mut failures,
                                &label,
                                &doc,
                                "&lt;!-- fsl:claim begin id=\"x\" digest=\"x\" --&gt;",
                            );
                            contains(&mut failures, &label, &doc, "&lt;!-- fsl:claim end --&gt;");
                        }
                        (false, Err(error)) if error == REFUSAL => {}
                        (_, produced) => {
                            let expected = if accept { "accepted artifact" } else { REFUSAL };
                            let produced = produced.map(|doc| blocks(&doc));
                            failures.push(format!(
                                "{label}: expected {expected}; produced {produced:?}"
                            ));
                        }
                    }
                }
            }
        }
    }
    finish(&failures);
}

const EVIDENCE_INTRO: &str = "The external evidence files supplied to this document. Only rows matching a requirement ID contribute to that requirement's assurance-class fields.";

fn evidence_listing(md: &str) -> &str {
    md.split_once("## Verification evidence sources\n\n")
        .unwrap()
        .1
        .split_once("\n\n## Generation info")
        .unwrap()
        .0
}

#[test]
fn rendered_plain_rows_keep_legacy_raw_order() {
    // Base82 CLI: a.json precedes all three b paths, in either input order.
    let mut failures = Vec::new();
    for (path, encoded) in [
        ("b|.json", "<code>b&#124;.json</code>"),
        ("b<.json", "<code>b&lt;.json</code>"),
        ("b`.json", "<code>b&#96;.json</code>"),
    ] {
        for reverse in [false, true] {
            let mut files = vec![
                ("a.json".into(), json!({"result":"verified"})),
                (path.into(), json!({"result":"verified"})),
            ];
            if reverse {
                files.reverse();
            }
            let doc = stable("spec.fsl", &files)
                .1
                .expect("plain evidence must render");
            let expected = format!(
                "{EVIDENCE_INTRO}\n\n- `a.json` (result: `verified`, matched requirement IDs: (whole specification; not attributed to an individual requirement))\n- {encoded} (result: `verified`, matched requirement IDs: (whole specification; not attributed to an individual requirement))"
            );
            let produced = evidence_listing(&doc);
            if produced != expected {
                failures.push(format!(
                    "{path} reverse={reverse}: expected {expected:?}; produced {produced:?}"
                ));
            }
        }
    }
    finish(&failures);
}

#[test]
fn duplicate_and_distinct_same_path_rows_keep_legacy_multiplicity() {
    // Base82 retains identical CLI rows; distinct same-path data uses AppliedEvidence/source.
    let mut failures = Vec::new();
    for (path, encoded) in [
        ("a.json", "`a.json`"),
        ("b|.json", "<code>b&#124;.json</code>"),
        ("b<.json", "<code>b&lt;.json</code>"),
        ("b`.json", "<code>b&#96;.json</code>"),
    ] {
        for (results, first) in [
            (["verified", "verified"], "verified"),
            (["verified", "inconclusive"], "inconclusive"),
        ] {
            for reverse in [false, true] {
                let mut files: Vec<_> = results
                    .into_iter()
                    .map(|result| (path.into(), json!({"result":result})))
                    .collect();
                if reverse {
                    files.reverse();
                }
                let doc = stable("spec.fsl", &files)
                    .1
                    .expect("duplicate evidence must render");
                let expected = format!(
                    "{EVIDENCE_INTRO}\n\n- {encoded} (result: `{first}`, matched requirement IDs: (whole specification; not attributed to an individual requirement))\n- {encoded} (result: `verified`, matched requirement IDs: (whole specification; not attributed to an individual requirement))"
                );
                let produced = evidence_listing(&doc);
                if produced != expected {
                    failures.push(format!("{path} {results:?} reverse={reverse}: expected {expected:?}; produced {produced:?}"));
                }
            }
        }
    }
    finish(&failures);
}
