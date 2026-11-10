// SPDX-License-Identifier: Apache-2.0

use fsl_syntax::{
    AiDataset, AiMetricRequirement, AiProject, AiRawBlock, AiStatisticalProperty, SourcePos, Span,
    parse_ai_project,
};

fn position(source: &str, offset: usize) -> SourcePos {
    let mut pos = SourcePos {
        offset: 0,
        line: 1,
        column: 1,
    };
    for ch in source[..offset].chars() {
        pos.offset += ch.len_utf8();
        if ch == '\n' {
            pos.line += 1;
            pos.column = 1;
        } else {
            pos.column += 1;
        }
    }
    pos
}

fn marker_span(source: &str, marker: &str) -> Span {
    assert_eq!(source.matches(marker).count(), 1, "unique fixture marker");
    let start = source.find(marker).unwrap();
    Span {
        start: position(source, start),
        end: position(source, start + marker.len()),
    }
}

const DATA: &str = "dataset Real {\n  source \"records//日本😀.jsonl\";\n}\n";
const SIBLING: &str = "evaluator Judge {}\nai_action Route { ignored { nested } }\n";

fn expected_project() -> AiProject {
    AiProject {
        name: "Scanner".into(),
        datasets: vec![AiDataset {
            name: "Real".into(),
            source: Some("records//日本😀.jsonl".into()),
        }],
        evaluators: vec!["Judge".into()],
        raw_blocks: vec![AiRawBlock {
            kind: "ai_action".into(),
            name: "Route".into(),
        }],
        ..AiProject::default()
    }
}

fn assert_project(source: &str) {
    let actual = parse_ai_project(source, "Scanner")
        .unwrap_or_else(|error| panic!("valid // comment grammar must parse: {error:?}"));
    assert_eq!(
        actual,
        expected_project(),
        "entire model; no omitted fields or ghost blocks"
    );
}

#[test]
fn preservation_valid_siblings_and_literal_comment_marker() {
    assert_project(&format!("// 日本😀\r\n{DATA}{SIBLING}"));
}

#[test]
fn detector_commented_headers_do_not_create_ghost_blocks() {
    assert_project(&format!(
        "// dataset Ghost {{}} evaluator Fake {{}} ai_action Phantom {{}}\n{DATA}{SIBLING}"
    ));
}

#[test]
fn detector_leading_comment_braces_do_not_hide_real_siblings() {
    assert_project(&format!("// 日本😀 {{ {{\r\n{DATA}{SIBLING}"));
}

#[test]
fn detector_comment_close_brace_does_not_truncate_dataset_fields() {
    let source = format!(
        "dataset Real {{\n  // }} dataset Ghost {{}}\n  source \"records//日本😀.jsonl\";\n}}\n{SIBLING}"
    );
    assert_project(&source);
}

#[test]
fn detector_comment_open_brace_does_not_make_valid_block_unterminated() {
    let source =
        format!("dataset Real {{\n  // {{\n  source \"records//日本😀.jsonl\";\n}}\n{SIBLING}");
    assert_project(&source);
}

#[test]
fn preservation_quote_in_comment_between_complete_blocks() {
    assert_project(&format!("{DATA}// unmatched quote \"\n{SIBLING}"));
}

#[test]
fn detector_quote_and_keywords_inside_body_comment_are_inert() {
    let source = format!(
        "dataset Real {{\n  // \" }} evaluator Fake {{\n  source \"records//日本😀.jsonl\";\n}}\n{SIBLING}"
    );
    assert_project(&source);
}

#[test]
fn preservation_metadata_escape_spelling_and_string_braces() {
    // The metadata scanner supports backslash escape state; atom preserves
    // the spelling rather than decoding it. Do not demand shared-lexer escapes.
    let source = r#"dataset Real {
  source "records\"//日本😀.jsonl";
}
ai_action Route { payload "escaped\"} dataset Ghost {"; }
evaluator Judge {}
"#;
    let mut expected = expected_project();
    expected.datasets[0].source = Some(r#"records\"//日本😀.jsonl"#.into());
    assert_eq!(parse_ai_project(source, "Scanner").unwrap(), expected);
}

#[test]
fn detector_nested_comment_header_is_not_a_real_slice() {
    let source = "// 日本😀\r\nstatistical_property Quality {\r\n  dataset Real;\r\n  // slice Ghost { require min_samples >= 999; }\r\n  slice Live {\r\n    require min_samples >= 3;\r\n  }\r\n}\r\n";
    let clause = marker_span(source, "require min_samples >= 3").start;
    let expected = AiProject {
        name: "Scanner".into(),
        statistical_properties: vec![AiStatisticalProperty {
            name: "Quality".into(),
            target: None,
            dataset: Some("Real".into()),
            evaluator: None,
            confidence: 0.95,
            requirements: vec![AiMetricRequirement {
                kind: "min_samples".into(),
                metric: None,
                confidence: None,
                comparator: Some(">=".into()),
                threshold: Some(3.0),
                slice: "Live".into(),
                min_samples: Some(3),
                source: "min_samples >= 3".into(),
                line: clause.line,
                column: clause.column,
            }],
        }],
        ..AiProject::default()
    };
    assert_eq!(parse_ai_project(source, "Scanner").unwrap(), expected);
}

fn assert_component_span(prefix: &str) {
    let source = format!(
        "{prefix}// 日本😀\r\n  ai_component Actual {{\r\n    @kind(\"real\") tool Search {{ schema SearchV1; }}\r\n  }}\r\nevaluator Judge {{}}"
    );
    let project = parse_ai_project(&source, "Scanner").unwrap();
    assert_eq!(project.components.len(), 1);
    assert_eq!(project.components[0].name, "Actual");
    assert_eq!(
        project.components[0].tools[0].annotations.source_order()[0].span(),
        marker_span(&source, "@kind(\"real\")")
    );
    let loc = marker_span(&source, "ai_component Actual").start;
    assert_eq!(
        (
            project.components[0].loc.line,
            project.components[0].loc.column
        ),
        (loc.line, loc.column)
    );
    assert_eq!(project.evaluators, ["Judge"]);
}

#[test]
fn preservation_component_annotation_span_after_unicode_comment() {
    assert_component_span("");
}

#[test]
fn detector_commented_component_does_not_replace_real_component_or_spans() {
    assert_component_span("// ai_component Ghost {} \" } {\r\n");
}

#[test]
fn preservation_real_unterminated_block_reports_original_open_brace() {
    let source = "// 日本😀\r\ndataset Good {}\r\n  dataset Broken {\r\n source \"x\";\r\n";
    let error = parse_ai_project(source, "Scanner").unwrap_err();
    let open = position(source, source.find("Broken {").unwrap() + "Broken ".len());
    assert_eq!(error.code(), "FSL-PARSE");
    assert_eq!(error.message, "unterminated dataset 'Broken' block");
    assert_eq!(
        error.span,
        Span {
            start: open,
            end: open
        }
    );
}

#[test]
fn preservation_real_duplicate_is_rejected_at_second_declaration() {
    let source = "// 日本😀\n dataset Real {}\n  dataset Real {\n source \"other\";\n}";
    let error = parse_ai_project(source, "Scanner").unwrap_err();
    let start = marker_span(source, "dataset Real {\n").start;
    assert_eq!(error.code(), "FSL-PARSE");
    assert_eq!(error.message, "duplicate dataset 'Real'");
    assert_eq!(error.span, Span { start, end: start });
}
