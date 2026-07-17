// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_syntax::{Annotation, MetaTag, SourceFile, SpecItem, SurfaceDocument, parse_document};

fn spec_items(source: &str) -> Vec<SpecItem> {
    let SurfaceDocument::Spec(spec) = parse_document(SourceFile::new(source))
        .expect("parse spec")
        .surface
    else {
        panic!("expected a spec document");
    };
    spec.items
}

#[test]
fn stacked_builtin_and_custom_annotations_attach_to_one_nested_declaration() {
    let items = spec_items(
        r#"
spec Stacked {
  state { ready: Bool }
  init { ready = false }
  @requirement("REQ-1", "publishing changes readiness")
  @undecided("rollback policy is pending")
  @kind("safety")
  @acme.review.owner(team, 2, true)
  invariant Stable { ready == ready }
}
"#,
    );
    let invariant = items
        .iter()
        .find_map(|item| match item {
            SpecItem::Invariant {
                name, annotations, ..
            } if name == "Stable" => Some(annotations),
            _ => None,
        })
        .expect("Stable invariant");
    let annotations = invariant.source_order();
    assert_eq!(annotations.len(), 4);
    assert!(matches!(&annotations[0], Annotation::Requirement { id, .. } if id == "REQ-1"));
    assert!(
        matches!(&annotations[1], Annotation::Undecided { reason, .. } if reason == "rollback policy is pending")
    );
    assert!(matches!(&annotations[2], Annotation::Kind { id, .. } if id == "safety"));
    let Annotation::Custom {
        namespace,
        arguments,
        ..
    } = &annotations[3]
    else {
        panic!("expected custom annotation");
    };
    assert_eq!(namespace.to_string(), "acme.review.owner");
    assert_eq!(arguments.len(), 3);
}

#[test]
fn annotations_attach_across_comments_and_blank_lines() {
    let items = spec_items(
        "spec Commented {\n  state { ready: Bool }\n  init { ready = false }\n\n  // first tag\n  @requirement(\"REQ-1\")\n\n  // second tag\n  @kind(\"safety\")\n\n  invariant Stable { ready == ready }\n}\n",
    );
    let invariant = items
        .iter()
        .find_map(|item| match item {
            SpecItem::Invariant {
                name, annotations, ..
            } if name == "Stable" => Some(annotations),
            _ => None,
        })
        .expect("Stable invariant");
    assert_eq!(invariant.source_order().len(), 2);
}

#[test]
fn annotation_before_closing_brace_is_a_target_error() {
    let error = parse_document(SourceFile::new(
        "spec Stray {\n  state { ready: Bool }\n  init { ready = false }\n  @requirement(\"REQ-1\")\n}\n",
    ))
    .unwrap_err();
    assert_eq!(error.code(), "FSL-ANNOTATION-TARGET");
}

#[test]
fn annotation_before_unsupported_declaration_is_a_target_error() {
    let error = parse_document(SourceFile::new(
        "spec Stray {\n  @requirement(\"REQ-1\")\n  const A = 1\n  state { ready: Bool }\n  init { ready = false }\n}\n",
    ))
    .unwrap_err();
    assert_eq!(error.code(), "FSL-ANNOTATION-TARGET");
}

#[test]
fn nested_annotation_argument_and_path_errors_reuse_the_coded_diagnostics() {
    let arity_error = parse_document(SourceFile::new(
        "spec Bad {\n  state { ready: Bool }\n  init { ready = false }\n  @requirement()\n  invariant Stable { ready == ready }\n}\n",
    ))
    .unwrap_err();
    assert_eq!(arity_error.code(), "FSL-ANNOTATION-ARGUMENTS");

    let path_error = parse_document(SourceFile::new(
        "spec Bad {\n  state { ready: Bool }\n  init { ready = false }\n  @.foo(\"x\")\n  invariant Stable { ready == ready }\n}\n",
    ))
    .unwrap_err();
    assert_eq!(path_error.code(), "FSL-ANNOTATION-PATH");

    let syntax_error = parse_document(SourceFile::new(
        "spec Bad {\n  state { ready: Bool }\n  init { ready = false }\n  @requirement(\"REQ-1\"\n  invariant Stable { ready == ready }\n}\n",
    ))
    .unwrap_err();
    assert_eq!(syntax_error.code(), "FSL-ANNOTATION-SYNTAX");
}

#[test]
fn legacy_meta_first_colon_rule_fixture() {
    let cases = [
        ("REQ-3: text", "REQ-3", Some("text")),
        ("REQ-3: a: b", "REQ-3", Some("a: b")),
        ("REQ-3", "REQ-3", None),
        (" REQ-1 : text ", "REQ-1", Some("text")),
    ];
    for (input, id, text) in cases {
        let tag = MetaTag::parse(
            input,
            fsl_syntax::Span {
                start: fsl_syntax::SourcePos {
                    offset: 0,
                    line: 1,
                    column: 1,
                },
                end: fsl_syntax::SourcePos {
                    offset: input.len(),
                    line: 1,
                    column: u32::try_from(input.len()).expect("short fixture input") + 1,
                },
            },
        );
        assert_eq!(tag.id, id, "input {input:?}");
        assert_eq!(tag.text.as_deref(), text, "input {input:?}");
    }
}

#[test]
fn business_dialect_annotation_diagnostics_are_consistent_with_other_dialects() {
    let error = parse_document(SourceFile::new(
        "business Stray {\n  @requirement(\"REQ-1\")\n  actor Reviewer\n}\n",
    ))
    .unwrap_err();
    assert_eq!(error.code(), "FSL-ANNOTATION-TARGET");
}

#[test]
fn business_policy_and_goal_attach_leading_annotations() {
    let SurfaceDocument::Business(business) = parse_document(SourceFile::new(
        "business Annotated {\n  @requirement(\"REQ-1\")\n  policy POL-1 \"text\" invariant { true }\n}\n",
    ))
    .expect("parse business spec")
    .surface
    else {
        panic!("expected a business document");
    };
    let policy = business
        .items
        .iter()
        .find_map(|item| match item {
            fsl_syntax::BusinessItem::Policy { annotations, .. } => Some(annotations),
            _ => None,
        })
        .expect("POL-1 policy");
    assert_eq!(policy.source_order().len(), 1);
}

#[test]
fn top_level_and_nested_annotations_coexist() {
    let items = spec_items(
        r#"
@kind("safety")
spec TopAndNested {
  state { ready: Bool }
  init { ready = false }
  @requirement("REQ-1")
  invariant Stable { ready == ready }
}
"#,
    );
    let invariant = items
        .iter()
        .find_map(|item| match item {
            SpecItem::Invariant {
                name, annotations, ..
            } if name == "Stable" => Some(annotations),
            _ => None,
        })
        .expect("Stable invariant");
    assert_eq!(invariant.source_order().len(), 1);
    assert_eq!(
        fsl_syntax::dialect_keyword(
            "@kind(\"safety\")\nspec TopAndNested { state { ready: Bool } init { ready = false } }"
        )
        .unwrap(),
        "spec"
    );
}
