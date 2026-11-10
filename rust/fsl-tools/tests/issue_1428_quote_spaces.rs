// SPDX-License-Identifier: Apache-2.0
//! Literal code-span oracles calibrated by the parent-owned actual GFM probe.
//! Public-API coverage only: no renderer/network or production escaping oracle.

use fsl_tools::{AppliedEvidence, Locale, RequirementClaimSet, Segment};
use serde_json::{Value, json};

const SOURCE: &str = "requirements Demo { state { x: Bool } init { x = false }\n@requirement(\"REQ-1\", \"plain\") action act() { x = true }\n}";

fn document(source: &str, files: &[(String, Value)]) -> (RequirementClaimSet, String) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let claims = fsl_tools::project_requirement_claims_from_source(source, Some("spec.fsl"), root)
        .expect("genuine requirements fixture projects, not semantic RED");
    let saved = claims.clone();
    let kernel = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new(root))
        .expect("fixture parses");
    let model = fsl_core::build_model(kernel.clone()).expect("fixture checks");
    let evidence = AppliedEvidence {
        files,
        digest: "sha256:quote-spaces-evidence",
    };
    let doc = fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        source,
        Locale::En,
        None,
        Some(&evidence),
        None,
    )
    .expect("fixture renders")
    .markdown;
    assert_eq!(claims, saved, "complete input RCIR remains immutable");
    (claims, doc)
}

fn blocks(md: &str) -> Vec<Segment> {
    let claims: Vec<_> = fsl_tools::parse_generated_document(md)
        .expect("WHOLE artifact parses")
        .segments
        .into_iter()
        .filter(|segment| matches!(segment, Segment::Claim { .. }))
        .collect();
    assert!(!claims.is_empty(), "claim comparison must not be vacuous");
    claims
}

fn intent_cases(cases: &[(&str, &str)]) {
    let mut failures = Vec::new();
    for &(raw, expected) in cases {
        let source = SOURCE.replace("plain", raw);
        let original = source.clone();
        // Equal byte lengths keep every claim source span comparable, including TABs.
        let neutral = SOURCE.replace("plain", &"p".repeat(raw.len()));
        let files = [(
            "raw-evidence.json".into(),
            json!({"result":"verified", "detail":"raw **evidence** \t "}),
        )];
        let saved = files.clone();
        let (claims, doc) = document(&source, &files);
        let (control, plain) = document(&neutral, &files);
        assert_eq!(source, original, "authored source bytes unchanged");
        assert_eq!(files, saved, "complete raw producer evidence unchanged");
        assert_eq!(
            claims.requirements[0].statements[0].text.as_deref(),
            Some(raw)
        );
        assert_eq!(claims.claims, control.claims, "complete RCIR claim array");
        assert_eq!(
            blocks(&doc),
            blocks(&plain),
            "whole generated claim segments"
        );
        let produced: Vec<_> = doc.lines().filter(|line| line.starts_with("> ")).collect();
        eprintln!("raw {raw:?}; expected {expected:?}; produced {produced:?}");
        if !doc.contains(&format!("\n{expected}\n")) {
            failures.push(format!(
                "raw {raw:?}; expected {expected:?}; produced {produced:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn deep_quote_ascii_space_edges_need_one_guard_space_each() {
    // GFM removes one ASCII space from each edge of non-all-space code content.
    // These independent literals add exactly those two guards around the WHOLE raw line.
    intent_cases(&[
        ("    1. literal intent ", "> `     1. literal intent  `"),
        ("    1. literal intent  ", "> `     1. literal intent   `"),
        (" \t  1. literal intent ", "> `  \t  1. literal intent  `"),
    ]);
}

#[test]
fn benign_all_spaces_tabs_and_no_trailing_ascii_space_stay_bare() {
    // All-ASCII-space content has GFM's exemption: padding would add visible spaces.
    intent_cases(&[
        ("     ", "> `     `"),
        ("\t  text ", "> `\t  text `"),
        ("    text\t", "> `    text\t`"),
        ("    1. literal intent", "> `    1. literal intent`"),
    ]);
}
