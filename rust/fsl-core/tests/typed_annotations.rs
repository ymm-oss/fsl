// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_core::{
    Annotation, AnnotationValue, FsResolver, SymbolPath, action_target, build_model,
    parse_kernel_source, requirements_trace_contract,
};
use fsl_syntax::{SourcePos, Span};

fn span(line: u32) -> Span {
    Span {
        start: SourcePos {
            offset: line as usize,
            line,
            column: 1,
        },
        end: SourcePos {
            offset: line as usize + 1,
            line,
            column: 2,
        },
    }
}

fn direct_spec() -> fsl_core::KernelSpec {
    parse_kernel_source(
        r#"
spec TypedAnnotations "design: annotation carrier" {
  state { ready: Bool }
  init "undecided: initial rollout state is pending" { ready = false }
  action publish() "REQ-1: publishing changes readiness" { ready = true }
  invariant Stable "REQ-2: readiness is Boolean" { ready == ready }
}
"#,
        &FsResolver::new("."),
    )
    .expect("parse direct spec")
}

#[test]
fn top_level_annotations_are_passed_to_the_dispatched_declaration() {
    let kernel = parse_kernel_source(
        r#"@requirement("REQ-DOCUMENT", "document contract")
@acme.review.owner(team.platform)
spec AnnotatedDocument {}
"#,
        &FsResolver::new("."),
    )
    .expect("parse top-level annotations");
    let annotations = kernel.annotations().annotations_for("spec");
    assert_eq!(annotations.source_order().len(), 2);
    assert!(matches!(
        &annotations.source_order()[0],
        Annotation::Requirement { id, text, .. }
            if id == "REQ-DOCUMENT" && text.as_deref() == Some("document contract")
    ));
    assert!(matches!(
        &annotations.source_order()[1],
        Annotation::Custom { namespace, .. }
            if namespace.to_string() == "acme.review.owner"
    ));
}

#[test]
fn keeps_multiple_typed_annotations_and_legacy_projection_on_one_declaration() {
    let mut kernel = direct_spec();
    let target = action_target("publish");
    kernel.bind_annotation(
        target.clone(),
        Annotation::Requirement {
            id: "REQ-3".to_owned(),
            text: Some("publication is auditable".to_owned()),
            span: span(20),
        },
    );
    kernel.bind_annotation(
        target.clone(),
        Annotation::Undecided {
            reason: "review owner is pending".to_owned(),
            span: span(21),
        },
    );
    kernel.bind_annotation(
        target.clone(),
        Annotation::Kind {
            id: "safety".to_owned(),
            text: None,
            span: span(22),
        },
    );
    kernel.bind_annotation(
        target.clone(),
        Annotation::Custom {
            namespace: SymbolPath::new(["acme".to_owned(), "review".to_owned()], span(23))
                .expect("valid namespace"),
            arguments: vec![
                AnnotationValue::String("team-a".to_owned()),
                AnnotationValue::Integer(2),
                AnnotationValue::Boolean(true),
            ],
            span: span(23),
        },
    );

    let model = build_model(kernel).expect("build checked model");
    let action = model
        .actions
        .iter()
        .find(|action| action.name == "publish")
        .expect("publish action");
    assert_eq!(action.annotations.source_order().len(), 5);
    assert_eq!(
        model
            .requirements_for(&target)
            .into_iter()
            .map(|requirement| requirement.id)
            .collect::<Vec<_>>(),
        ["REQ-1", "REQ-3"]
    );
    assert_eq!(
        action.annotations.undecided()[0].0,
        "review owner is pending"
    );
    assert_eq!(action.meta.as_ref().expect("legacy projection").id, "REQ-1");
}

#[test]
fn semantic_requirement_order_is_stable_and_identical_relations_are_deduplicated() {
    let target = action_target("publish");
    let annotations = [
        Annotation::Requirement {
            id: "REQ-9".to_owned(),
            text: Some("later".to_owned()),
            span: span(30),
        },
        Annotation::Requirement {
            id: "REQ-0".to_owned(),
            text: Some("earlier".to_owned()),
            span: span(31),
        },
        Annotation::Requirement {
            id: "REQ-9".to_owned(),
            text: Some("later".to_owned()),
            span: span(32),
        },
    ];
    let mut forward = direct_spec();
    let mut reverse = direct_spec();
    for annotation in annotations.clone() {
        forward.bind_annotation(target.clone(), annotation);
    }
    for annotation in annotations.into_iter().rev() {
        reverse.bind_annotation(target.clone(), annotation);
    }

    let forward = build_model(forward).expect("forward model");
    let reverse = build_model(reverse).expect("reverse model");
    let ids = |model: &fsl_core::KernelModel| {
        model
            .requirements_for(&target)
            .into_iter()
            .map(|requirement| requirement.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&forward), ["REQ-0", "REQ-1", "REQ-9"]);
    assert_eq!(ids(&forward), ids(&reverse));
}

#[test]
fn conflicting_text_for_one_requirement_id_is_a_checked_model_error() {
    let mut kernel = direct_spec();
    kernel.bind_annotation(
        action_target("publish"),
        Annotation::Requirement {
            id: "REQ-1".to_owned(),
            text: Some("conflicting text".to_owned()),
            span: span(40),
        },
    );
    let error = build_model(kernel).expect_err("conflicting relation must fail");
    assert!(error.message.contains("REQ-1"));
    assert!(error.message.contains("conflicting text"));
}

#[test]
fn requirements_covers_lowers_through_the_typed_requirement_adapter() {
    let source = r#"
requirements CoversAdapter {
  process Claim {
    stages Draft, Done
    initial Draft
    transition finish Draft -> Done by User covers REQ-C "finish is traceable"
  }
}
verify { instances Claim = 1 }
"#;
    let kernel =
        parse_kernel_source(source, &FsResolver::new(".")).expect("parse requirements process");
    let model = build_model(kernel).expect("build requirements model");
    let links = model.requirements_for(&action_target("finish"));
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].id, "REQ-C");
    assert_eq!(links[0].text.as_deref(), Some("finish is traceable"));
    assert_eq!(links[0].span.start.line, 6);
    let annotation = "covers REQ-C \"finish is traceable\"";
    let offset = source.find(annotation).expect("covers annotation");
    assert_eq!(links[0].span.start.offset, offset);
    assert_eq!(links[0].span.start.column, 45);
    assert_eq!(links[0].span.end.offset, offset + annotation.len());
}

#[test]
fn requirement_blocks_merge_outer_requirement_with_inner_legacy_annotations() {
    let source = r#"
requirements BlockAdapter {
  state { ready: Bool }
  init { ready = false }
  requirement REQ-B "publishing is controlled" {
    action publish() "undecided: review owner is pending" { ready = true }
  }
}
"#;
    let kernel =
        parse_kernel_source(source, &FsResolver::new(".")).expect("parse requirement block");
    let model = build_model(kernel).expect("build requirement block model");
    let action = model
        .actions
        .iter()
        .find(|action| action.name == "publish")
        .expect("publish action");
    let requirement = &model.requirements_for(&action_target("publish"))[0];
    assert_eq!(requirement.id, "REQ-B");
    let annotation = "requirement REQ-B \"publishing is controlled\"";
    let offset = source.find(annotation).expect("requirement annotation");
    assert_eq!(requirement.span.start.offset, offset);
    assert_eq!(requirement.span.end.offset, offset + annotation.len());
    assert_eq!(
        action.annotations.undecided()[0].0,
        "review owner is pending"
    );
}

#[test]
fn explicit_requirement_syntax_rejects_the_reserved_undecided_id() {
    let kernel = parse_kernel_source(
        r#"
requirements ReservedRequirementId {
  process Claim {
    stages Draft, Done
    initial Draft
    transition finish Draft -> Done by User covers undecided "not a requirement"
  }
}
verify { instances Claim = 1 }
"#,
        &FsResolver::new("."),
    )
    .expect("parse reserved ID for checked validation");
    let error = build_model(kernel).expect_err("reserved requirement ID must fail");
    assert!(error.message.contains("reserved"));
    assert!(error.message.contains("requirement ID"));
}

#[test]
fn acceptance_and_forbidden_cases_expose_the_same_typed_requirement_relation() {
    let contract = requirements_trace_contract(
        r#"
requirements TraceCases {
  state { ready: Bool }
  init { ready = false }
  action publish() { ready = true }
  acceptance AC-1 "publication succeeds" {
    publish()
    expect ready == true
  }
  forbidden NEG-1 "publication cannot repeat" {
    publish() publish()
    expect rejected
  }
}

"#,
    )
    .expect("parse trace contract")
    .expect("requirements trace contract");
    assert_eq!(
        contract.acceptance[0].annotations.requirements().unwrap()[0].id,
        "AC-1"
    );
    assert_eq!(
        contract.forbidden[0].annotations.requirements().unwrap()[0].id,
        "NEG-1"
    );
}

#[test]
fn trace_cases_reject_the_reserved_undecided_requirement_id() {
    for keyword in ["acceptance", "forbidden"] {
        let expectation = if keyword == "acceptance" {
            "expect ready == true"
        } else {
            "expect rejected"
        };
        let source = format!(
            r#"
requirements ReservedTraceId {{
  state {{ ready: Bool }}
  init {{ ready = false }}
  action publish() {{ ready = true }}
  {keyword} undecided "not a requirement" {{
    publish()
    {expectation}
  }}
}}
"#
        );
        let error = requirements_trace_contract(&source)
            .expect_err("reserved trace requirement ID must fail");
        assert!(error.message.contains("reserved"));
        assert_eq!(error.line, 6);
    }
}
