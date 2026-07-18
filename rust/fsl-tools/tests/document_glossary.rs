// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for the glossary sidecar (issue #330):
//! `fsl_tools::document_glossary`'s parsing/validation, and how the
//! renderer (issue #326) applies accepted labels. `fslc`'s `--glossary`
//! flag, diagnostics, and digest-separation contract are exercised
//! end-to-end in `rust/fslc/tests/document_glossary_cli.rs`.

use std::path::{Path, PathBuf};

use fsl_tools::{AppliedGlossary, GlossaryIssue, Locale};

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_path(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

/// The issue's own example glossary, against `examples/pm/cancel_system.fsl`
/// — which has exactly the `submit_cancel` action, `scr` state variable, and
/// `Screen.CancelForm` enum member the issue's example targets name.
const EXAMPLE_GLOSSARY: &str = r#"{
  "schema": "fslc.document-glossary.v1",
  "locale": "ja",
  "labels": {
    "action:submit_cancel": "解約フォームを送信する",
    "state:scr": "契約画面状態",
    "enum:Screen.CancelForm": "解約フォーム"
  }
}"#;

// --- Parsing and duplicate-key detection ------------------------------------

#[test]
fn valid_glossary_parses_to_a_sorted_label_map() {
    let glossary = fsl_tools::parse_glossary(EXAMPLE_GLOSSARY).expect("parse");
    assert_eq!(glossary.locale, Locale::Ja);
    assert_eq!(glossary.labels.len(), 3);
    assert_eq!(
        glossary
            .labels
            .get("action:submit_cancel")
            .map(String::as_str),
        Some("解約フォームを送信する")
    );
}

#[test]
fn duplicate_label_key_is_detected_as_conflict() {
    let text = r#"{"schema":"fslc.document-glossary.v1","locale":"ja","labels":{"action:submit_cancel":"A","action:submit_cancel":"B"}}"#;
    let issues = fsl_tools::parse_glossary(text).expect_err("duplicate key must be rejected");
    assert!(
        issues
            .iter()
            .any(|issue| matches!(issue, GlossaryIssue::DuplicateTarget(target) if target == "action:submit_cancel"))
    );
}

#[test]
fn triple_duplicate_label_key_is_still_one_conflict_per_repeat() {
    let text = r#"{"schema":"fslc.document-glossary.v1","locale":"ja","labels":{"a:x":"1","a:x":"2","a:x":"3"}}"#;
    let issues = fsl_tools::parse_glossary(text).expect_err("duplicate keys must be rejected");
    let conflicts = issues
        .iter()
        .filter(|issue| matches!(issue, GlossaryIssue::DuplicateTarget(target) if target == "a:x"))
        .count();
    // 3 occurrences -> 2 insert-collisions after the first.
    assert_eq!(conflicts, 2);
}

#[test]
fn duplicate_with_identical_values_is_still_a_conflict() {
    let text = r#"{"schema":"fslc.document-glossary.v1","locale":"ja","labels":{"a:x":"same","a:x":"same"}}"#;
    let issues = fsl_tools::parse_glossary(text).expect_err("duplicate key must be rejected");
    assert!(
        issues
            .iter()
            .any(|issue| matches!(issue, GlossaryIssue::DuplicateTarget(_)))
    );
}

#[test]
fn wrong_schema_is_rejected() {
    let text = r#"{"schema":"wrong","locale":"ja","labels":{}}"#;
    let issues = fsl_tools::parse_glossary(text).expect_err("wrong schema must be rejected");
    assert!(issues.iter().any(
        |issue| matches!(issue, GlossaryIssue::UnsupportedSchema(schema) if schema == "wrong")
    ));
}

#[test]
fn unsupported_locale_is_rejected() {
    let text = r#"{"schema":"fslc.document-glossary.v1","locale":"fr","labels":{}}"#;
    let issues = fsl_tools::parse_glossary(text).expect_err("unsupported locale must be rejected");
    assert!(
        issues.iter().any(
            |issue| matches!(issue, GlossaryIssue::UnsupportedLocale(locale) if locale == "fr")
        )
    );
}

#[test]
fn empty_label_is_rejected() {
    let text = r#"{"schema":"fslc.document-glossary.v1","locale":"ja","labels":{"action:x":""}}"#;
    let issues = fsl_tools::parse_glossary(text).expect_err("empty label must be rejected");
    assert!(
        issues.iter().any(
            |issue| matches!(issue, GlossaryIssue::EmptyLabel(target) if target == "action:x")
        )
    );
}

#[test]
fn multiline_and_control_character_labels_are_rejected() {
    let text = r#"{"schema":"fslc.document-glossary.v1","locale":"ja","labels":{"action:x":"safe\n## injected","state:y":"safe\ttext"}}"#;
    let issues = fsl_tools::parse_glossary(text).expect_err("control characters must be rejected");
    assert_eq!(
        issues
            .iter()
            .filter(|issue| matches!(issue, GlossaryIssue::UnsafeLabel(_)))
            .count(),
        2
    );
}

#[test]
fn unknown_top_level_key_is_rejected() {
    let text = r#"{"schema":"fslc.document-glossary.v1","locale":"ja","labels":{},"extra":true}"#;
    assert!(fsl_tools::parse_glossary(text).is_err());
}

#[test]
fn malformed_json_is_a_single_json_issue() {
    let issues = fsl_tools::parse_glossary("{not json").expect_err("malformed JSON is rejected");
    assert_eq!(issues.len(), 1);
    assert!(matches!(issues[0], GlossaryIssue::Json(_)));
}

// --- Target validation -------------------------------------------------------

fn cancel_system_model() -> (String, fsl_core::KernelModel) {
    let source = read("../../examples/pm/cancel_system.fsl");
    let root = manifest_path("../../examples/pm");
    let resolver = fsl_core::FsResolver::new(&root);
    let kernel = fsl_core::parse_kernel_source(&source, &resolver).expect("parse");
    let model = fsl_core::build_model(kernel.clone()).expect("build model");
    (source, model)
}

#[test]
fn known_action_state_and_enum_targets_all_validate() {
    let (_, model) = cancel_system_model();
    let glossary = fsl_tools::parse_glossary(EXAMPLE_GLOSSARY).expect("parse");
    let unknown = fsl_tools::unknown_targets(&glossary, &model);
    assert!(
        unknown.is_empty(),
        "unexpected unknown targets: {unknown:?}"
    );
}

#[test]
fn unknown_action_state_and_enum_targets_are_reported() {
    let (_, model) = cancel_system_model();
    let text = r#"{
        "schema": "fslc.document-glossary.v1",
        "locale": "ja",
        "labels": {
            "action:nonexistent": "x",
            "state:nonexistent": "y",
            "enum:Screen.Nonexistent": "z",
            "enum:NonexistentType.CancelForm": "w"
        }
    }"#;
    let glossary = fsl_tools::parse_glossary(text).expect("parse");
    let unknown = fsl_tools::unknown_targets(&glossary, &model);
    let targets: Vec<&str> = unknown.iter().map(|entry| entry.target.as_str()).collect();
    assert_eq!(targets.len(), 4);
    assert!(targets.contains(&"action:nonexistent"));
    assert!(targets.contains(&"state:nonexistent"));
    assert!(targets.contains(&"enum:Screen.Nonexistent"));
    assert!(targets.contains(&"enum:NonexistentType.CancelForm"));
}

#[test]
fn unrecognized_namespace_and_colonless_target_are_unknown() {
    let (_, model) = cancel_system_model();
    let text = r#"{
        "schema": "fslc.document-glossary.v1",
        "locale": "ja",
        "labels": {
            "property:invariant.Foo": "x",
            "submit_cancel": "y"
        }
    }"#;
    let glossary = fsl_tools::parse_glossary(text).expect("parse");
    let unknown = fsl_tools::unknown_targets(&glossary, &model);
    assert_eq!(unknown.len(), 2);
}

// --- Rendering integration ---------------------------------------------------

fn render_cancel_system(locale: Locale, glossary: Option<&fsl_tools::Glossary>) -> String {
    let source = read("../../examples/pm/cancel_system.fsl");
    let root = manifest_path("../../examples/pm");
    let claims = fsl_tools::project_requirement_claims_from_source(
        &source,
        Some("examples/pm/cancel_system.fsl"),
        &root,
    )
    .expect("project cancel_system.fsl");
    let resolver = fsl_core::FsResolver::new(&root);
    let kernel = fsl_core::parse_kernel_source(&source, &resolver).expect("parse");
    let model = fsl_core::build_model(kernel.clone()).expect("build model");
    let trace = fsl_core::requirements_trace_contract(&source).expect("trace contract");
    let applied = glossary.map(|glossary| AppliedGlossary {
        glossary,
        digest: "sha256:test",
    });
    fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        trace.as_ref(),
        locale,
        applied.as_ref(),
        None,
    )
    .expect("render paired RCIR")
    .markdown
}

#[test]
fn no_glossary_omits_glossary_specific_content() {
    // `None` emits no glossary key, section, or label under the v2 document
    // schema introduced by this issue.
    let with_explicit_none = render_cancel_system(Locale::Ja, None);
    assert!(!with_explicit_none.contains("glossary_digest"));
    assert!(!with_explicit_none.contains("## 用語集"));
}

#[test]
fn glossary_labels_the_action_heading_and_records_the_frontmatter_digest() {
    let glossary = fsl_tools::parse_glossary(EXAMPLE_GLOSSARY).expect("parse");
    let markdown = render_cancel_system(Locale::Ja, Some(&glossary));
    assert!(markdown.contains("glossary_digest: sha256:test"));
    assert!(markdown.contains("#### 操作: 解約フォームを送信する（`submit_cancel`）"));
    // An unlabeled action keeps the canonical-only heading.
    assert!(markdown.contains("#### 操作: `tap_cancel`"));
}

#[test]
fn glossary_section_lists_every_accepted_label_sorted_by_target() {
    let glossary = fsl_tools::parse_glossary(EXAMPLE_GLOSSARY).expect("parse");
    let markdown = render_cancel_system(Locale::Ja, Some(&glossary));
    let start = markdown.find("## 用語集").expect("glossary section exists");
    let section = &markdown[start..];
    let end = section
        .find("## 生成情報")
        .expect("generation-info section follows");
    let section = &section[..end];
    assert!(section.contains("- `action:submit_cancel`: 解約フォームを送信する"));
    assert!(section.contains("- `enum:Screen.CancelForm`: 解約フォーム"));
    assert!(section.contains("- `state:scr`: 契約画面状態"));
    // Sorted by target string: action: < enum: < state:
    let action_pos = section.find("`action:submit_cancel`").expect("action row");
    let enum_pos = section.find("`enum:Screen.CancelForm`").expect("enum row");
    let state_pos = section.find("`state:scr`").expect("state row");
    assert!(action_pos < enum_pos && enum_pos < state_pos);
}

#[test]
fn composite_lvalue_code_span_is_never_rewritten_by_a_state_label() {
    // v1 does not substitute a label inside rendered expression text (see
    // docs/DESIGN-document-glossary.md) — `state:scr` must never touch the
    // `scr[c].st == CancelForm`-shaped code spans inside claim bodies.
    let glossary = fsl_tools::parse_glossary(EXAMPLE_GLOSSARY).expect("parse");
    let markdown = render_cancel_system(Locale::Ja, Some(&glossary));
    assert!(
        markdown.contains("scr[c]"),
        "composite lvalue must stay canonical"
    );
    assert!(!markdown.contains("契約画面状態[c]"));
}

#[test]
fn glossary_en_locale_uses_the_english_heading_format() {
    let text = r#"{"schema":"fslc.document-glossary.v1","locale":"en","labels":{"action:submit_cancel":"Submit the cancellation form"}}"#;
    let glossary = fsl_tools::parse_glossary(text).expect("parse");
    let markdown = render_cancel_system(Locale::En, Some(&glossary));
    assert!(markdown.contains("#### Operation: Submit the cancellation form (`submit_cancel`)"));
}

#[test]
fn glossary_labels_are_escaped_as_markdown_text() {
    let text = r###"{"schema":"fslc.document-glossary.v1","locale":"en","labels":{"action:submit_cancel":"## MUST <script> *now*"}}"###;
    let glossary = fsl_tools::parse_glossary(text).expect("parse");
    let markdown = render_cancel_system(Locale::En, Some(&glossary));
    assert!(markdown.contains(r"\#\# MUST &lt;script&gt; \*now\*"));
    assert!(!markdown.contains("## MUST <script> *now*"));
}
