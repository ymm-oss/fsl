// SPDX-License-Identifier: Apache-2.0

use fsl_syntax::{
    AiAuthority, AiComponent, AiHardCheck, AiLoc, Annotation, AnnotationValue, Annotations,
    SourcePos, Span, SymbolPath, parse_ai_component, parse_ai_project,
};

const RICH: &str = r#"ai_component Rich { model "日本語😀";
  prompt prompt_v1;
  retriever retriever_v1;
  temperature 0.25;
  input Request;
  output Response;
  @acme.tool(ops.owner, "tag", 2, true)
  tool Search irreversible {
    schema SearchV1;
    precondition permitted;
    precondition available;
    effect saved;
  }
  @kind("blocked")
  tool Blocked { schema BlockedV1; }
  @requirement("TOOLS")
  tools [SuggestOnly, ApprovalOnly];
  @requirement("AUTH")
  authority {
    @requirement("SUGGEST")
    may_suggest SuggestOnly;
    @undecided("execute policy")
    may_execute Search;
    @kind("APPROVAL")
    requires_human_approval ApprovalOnly;
    @acme.deny(false)
    forbidden Blocked;
  }
  @requirement("FALLBACK")
  fallback {
    @kind("FALLBACK-ITEM")
    when unavailable require human_review;
  }
  @requirement("CHECK")
  check hard {
    @requirement("RULE")
    rule tool_authority;
    @kind("SCHEMA")
    rule tool_schema_declared;
  }
}
"#;

fn rich_source() -> String {
    format!(
        "// 日本語😀\nai_component First {{ tool FirstTool {{ schema FirstV1; }} }}\n\n    {RICH}"
    )
}

fn component(source: &str) -> AiComponent {
    let mut project = parse_ai_project(source, "Locations").expect("valid project syntax");
    project.components.pop().expect("last component")
}

// Fixture-side oracle: unique byte markers, newline counts and scalar widths.
// Neither the lexer nor the production project's translation is used here.
fn position(source: &str, offset: usize) -> SourcePos {
    let prefix = &source[..offset];
    SourcePos {
        offset,
        line: u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count() + 1).unwrap(),
        column: u32::try_from(prefix.rsplit('\n').next().unwrap().chars().count() + 1).unwrap(),
    }
}

fn span(source: &str, marker: &str) -> Span {
    assert_eq!(
        source.matches(marker).count(),
        1,
        "unique marker {marker:?}"
    );
    let offset = source.find(marker).unwrap();
    Span {
        start: position(source, offset),
        end: position(source, offset + marker.len()),
    }
}

fn loc(source: &str, marker: &str) -> AiLoc {
    let start = span(source, marker).start;
    AiLoc {
        line: start.line,
        column: start.column,
    }
}

fn annotation_cases(component: &AiComponent) -> Vec<(&Annotations, Vec<&'static str>)> {
    vec![
        (
            &component.tools[0].annotations,
            vec![r#"@acme.tool(ops.owner, "tag", 2, true)"#],
        ),
        (&component.tools[1].annotations, vec![r#"@kind("blocked")"#]),
        (
            &component.tools[2].annotations,
            vec![r#"@requirement("TOOLS")"#],
        ),
        (
            &component.tools[3].annotations,
            vec![r#"@requirement("TOOLS")"#],
        ),
        (
            &component.authority.annotations,
            vec![r#"@requirement("AUTH")"#],
        ),
        (
            &component.authority.may_suggest[0].annotations,
            vec![r#"@requirement("SUGGEST")"#],
        ),
        (
            &component.authority.may_execute[0].annotations,
            vec![r#"@undecided("execute policy")"#],
        ),
        (
            &component.authority.requires_human_approval[0].annotations,
            vec![r#"@kind("APPROVAL")"#],
        ),
        (
            &component.authority.forbidden[0].annotations,
            vec!["@acme.deny(false)"],
        ),
        (
            &component.fallback[0].annotations,
            vec![r#"@requirement("FALLBACK")"#, r#"@kind("FALLBACK-ITEM")"#],
        ),
        (
            &component.check.annotations,
            vec![r#"@requirement("CHECK")"#],
        ),
        (
            &component.check.rules[0].annotations,
            vec![r#"@requirement("RULE")"#],
        ),
        (
            &component.check.rules[1].annotations,
            vec![r#"@kind("SCHEMA")"#],
        ),
    ]
}

#[test]
fn component_values_and_annotation_payloads_control() {
    let component = component(&rich_source());
    assert_eq!(component.name, "Rich");
    assert_eq!(component.model.as_deref(), Some("日本語😀"));
    assert_eq!(component.prompt.as_deref(), Some("prompt_v1"));
    assert_eq!(component.retriever.as_deref(), Some("retriever_v1"));
    assert_eq!(
        component.temperature.map(f64::to_bits),
        Some(0.25_f64.to_bits())
    );
    assert_eq!(component.input_schema.as_deref(), Some("Request"));
    assert_eq!(component.output_schema.as_deref(), Some("Response"));
    let tools = component
        .tools
        .iter()
        .map(|tool| {
            (
                tool.name.as_str(),
                tool.schema.as_deref(),
                tool.irreversible,
                tool.preconditions
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                tool.effect.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tools,
        vec![
            (
                "Search",
                Some("SearchV1"),
                true,
                vec!["permitted", "available"],
                Some("saved")
            ),
            ("Blocked", Some("BlockedV1"), false, vec![], None),
            ("SuggestOnly", None, false, vec![], None),
            ("ApprovalOnly", None, false, vec![], None),
        ]
    );
    let authority = &component.authority;
    for (rules, expected) in [
        (&authority.may_suggest, "SuggestOnly"),
        (&authority.may_execute, "Search"),
        (&authority.requires_human_approval, "ApprovalOnly"),
        (&authority.forbidden, "Blocked"),
    ] {
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, expected);
    }
    assert_eq!(component.fallback.len(), 1);
    assert_eq!(component.fallback[0].reason, "unavailable");
    assert_eq!(component.fallback[0].target, "human_review");
    assert_eq!(
        component
            .check
            .rules
            .iter()
            .map(|rule| rule.name.as_str())
            .collect::<Vec<_>>(),
        ["tool_authority", "tool_schema_declared"]
    );
    for (annotations, expected) in annotation_cases(&component) {
        assert_eq!(
            annotations
                .source_order()
                .iter()
                .map(Annotation::render_source)
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn every_component_location_points_into_the_whole_project() {
    let source = rich_source();
    let project = parse_ai_project(&source, "Locations").unwrap();
    assert_eq!(project.components.len(), 2);
    let component = &project.components[1];
    let authority = &component.authority;
    let actual = vec![
        Some(component.loc),
        component.tools[0].loc,
        component.tools[1].loc,
        authority.loc,
        authority.may_suggest[0].loc,
        authority.may_execute[0].loc,
        authority.requires_human_approval[0].loc,
        authority.forbidden[0].loc,
        Some(component.fallback[0].loc),
        component.check.loc,
        component.check.rules[0].loc,
        component.check.rules[1].loc,
        Some(project.components[0].loc),
    ];
    let markers = [
        "ai_component Rich",
        "tool Search irreversible",
        "tool Blocked",
        "authority {",
        "may_suggest",
        "may_execute",
        "requires_human_approval",
        "forbidden Blocked",
        "when unavailable",
        "check hard",
        "rule tool_authority",
        "rule tool_schema_declared",
        "ai_component First",
    ];
    let expected = markers
        .iter()
        .map(|marker| Some(loc(&source, marker)))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected, "construct order: {markers:?}");
}

#[test]
fn absent_optional_locations_and_defaults_control() {
    let source = "// prelude\n    ai_component Bare { tools [Search, Save]; }";
    let component = component(source);
    assert_eq!(component.tools.len(), 2);
    assert!(component.tools.iter().all(|tool| tool.loc.is_none()));
    assert_eq!(component.authority, AiAuthority::default());
    assert_eq!(component.check, AiHardCheck::default());
    assert!(component.fallback.is_empty());
    assert_eq!(
        (component.model, component.prompt, component.retriever),
        (None, None, None)
    );
    assert_eq!(
        (
            component.temperature,
            component.input_schema,
            component.output_schema
        ),
        (None, None, None)
    );
}

#[test]
fn all_annotation_spans_point_into_original_utf8_source() {
    let source = rich_source().replace('\n', "\r\n");
    let component = component(&source);
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for (annotations, markers) in annotation_cases(&component) {
        assert_eq!(annotations.source_order().len(), markers.len());
        for (annotation, marker) in annotations.source_order().iter().zip(markers) {
            actual.push(annotation.span());
            expected.push(span(&source, marker));
        }
    }
    assert_eq!(
        actual, expected,
        "all annotation endpoints, including cloned block/list annotations"
    );
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(
            &source[actual.start.offset..actual.end.offset],
            &source[expected.start.offset..expected.end.offset]
        );
    }
}

fn assert_symbol_span(source: &str, path: &SymbolPath, marker: &str) {
    assert_eq!(path.span(), span(source, marker), "symbol path {marker}");
    let start = span(source, marker).start.offset;
    let mut offset = start;
    let expected = marker
        .split('.')
        .map(|segment| {
            let result = Span {
                start: position(source, offset),
                end: position(source, offset + segment.len()),
            };
            offset += segment.len() + 1;
            result
        })
        .collect::<Vec<_>>();
    assert_eq!(path.segment_spans(), expected, "symbol segments {marker}");
}

#[test]
fn custom_annotation_namespace_and_argument_spans_are_file_relative() {
    let source = rich_source();
    let component = component(&source);
    let Annotation::Custom {
        namespace,
        arguments,
        ..
    } = &component.tools[0].annotations.source_order()[0]
    else {
        panic!("custom tool annotation");
    };
    assert_symbol_span(&source, namespace, "acme.tool");
    let AnnotationValue::Symbol(argument) = &arguments[0] else {
        panic!("symbol argument");
    };
    assert_symbol_span(&source, argument, "ops.owner");
}

fn assert_inline_positions(source: &str) {
    let component = component(source);
    let actual = [
        Some(component.loc),
        component.tools[0].loc,
        component.check.loc,
        component.check.rules[0].loc,
    ];
    let expected = [
        "ai_component Inline",
        "tool Search",
        "check hard",
        "rule tool_authority",
    ]
    .map(|marker| Some(loc(source, marker)));
    assert_eq!(
        actual, expected,
        "original scalar columns, not byte offsets or UTF-16 units"
    );
}

const INLINE: &str = "ai_component Inline { model \"日本語😀\"; tool Search { schema S; } check hard { rule tool_authority; } }";

#[test]
fn indented_inline_unicode_positions() {
    assert_inline_positions(&format!("// 日本語😀\n\n      {INLINE}\n"));
}

#[test]
fn non_bmp_text_before_component_on_the_same_line() {
    assert_inline_positions(&format!("dataset D {{ source \"😀😀\" }} {INLINE}\n"));
}

#[test]
fn crlf_and_bom_inline_unicode_positions() {
    assert_inline_positions(&format!("\u{feff}// 日本語😀\r\n\r\n\t  {INLINE}\r\n"));
}

#[test]
fn initial_bom_counts_as_one_scalar_column() {
    let source = format!("\u{feff}{INLINE}\n");
    assert_eq!(
        loc(&source, "ai_component Inline"),
        AiLoc { line: 1, column: 2 }
    );
    assert_inline_positions(&source);
}

#[test]
fn standalone_complete_component_control() {
    // No extraction/offset: both entry points must retain exactly the same IR.
    let standalone = parse_ai_component(RICH).unwrap();
    let project = component(RICH);
    assert_eq!(project, standalone);
}

fn assert_parse_error_control(body: &str, marker: &str, code: &str, message: &str) {
    let source = format!(
        "\u{feff}// 日本語😀\r\n\r\n    ai_component Broken {{ model \"日本語😀\"; {body} }}\r\n"
    );
    let error = parse_ai_project(&source, "Broken").unwrap_err();
    assert_eq!(error.code(), code);
    assert_eq!(error.message, message);
    assert_eq!(
        error.span,
        span(&source, marker),
        "must relocate once, not twice"
    );
}

#[test]
fn embedded_syntax_error_is_not_double_relocated_control() {
    assert_parse_error_control(
        "invalid;",
        "invalid",
        "FSL-PARSE",
        "expected ai_component declaration",
    );
}

#[test]
fn embedded_lexical_error_is_relocated_once_control() {
    assert_parse_error_control("§", "§", "FSL-PARSE", "unexpected character '§'");
}

#[test]
fn embedded_annotation_error_is_not_double_relocated_control() {
    assert_parse_error_control(
        "@kind(7) tool Search { schema S; }",
        "@kind(7)",
        "FSL-ANNOTATION-ARGUMENTS",
        "@kind expects one or two string arguments",
    );
}
