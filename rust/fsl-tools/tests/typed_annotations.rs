// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_core::{Annotation, FsResolver, action_target, build_model, parse_kernel_source};
use fsl_syntax::{SourcePos, Span};

fn annotation_span(line: u32) -> Span {
    let start = SourcePos {
        offset: line as usize,
        line,
        column: 1,
    };
    Span {
        start,
        end: SourcePos {
            offset: start.offset + 1,
            column: 2,
            ..start
        },
    }
}

fn model_with_two_requirements() -> fsl_core::KernelModel {
    let mut kernel = parse_kernel_source(
        r#"
spec TypedOutputs {
  state { ready: Bool }
  init { ready = false }
  action publish() "REQ-2: second" { ready = true }
}
"#,
        &FsResolver::new("."),
    )
    .expect("parse spec");
    kernel.bind_annotation(
        action_target("publish"),
        Annotation::Requirement {
            id: "REQ-1".to_owned(),
            text: Some("first".to_owned()),
            span: annotation_span(20),
        },
    );
    build_model(kernel).expect("build model")
}

#[test]
fn tsg_keeps_all_typed_requirement_relations_with_a_singular_projection() {
    let tsg = fsl_tools::build_tsg(&model_with_two_requirements());
    let action = tsg["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == "action:publish")
        .expect("publish node");

    assert_eq!(action["meta"]["id"], "REQ-1");
    assert_eq!(
        action["requirements"],
        serde_json::json!([
            {"id":"REQ-1","text":"first"},
            {"id":"REQ-2","text":"second"}
        ])
    );
}

#[test]
fn ledger_registers_every_typed_requirement_relation() {
    let ledger = fsl_tools::render_ledger(
        "typed.fsl",
        &model_with_two_requirements(),
        &serde_json::json!({}),
        &serde_json::json!({}),
        None,
        &[],
    );

    assert!(ledger.contains("REQ-1"));
    assert!(ledger.contains("REQ-2"));
}

#[test]
fn tsg_and_undecided_report_read_declaration_level_annotation_syntax_with_no_legacy_string() {
    let kernel = parse_kernel_source(
        r#"
spec AnnotationSyntaxOnly {
  state { ready: Bool }
  init { ready = false }
  @requirement("REQ-1", "publishing changes readiness")
  @undecided("rollback policy is pending")
  action publish() { ready = true }
}
"#,
        &FsResolver::new("."),
    )
    .expect("parse spec");
    let model = build_model(kernel).expect("build model");

    let tsg = fsl_tools::build_tsg(&model);
    let action = tsg["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == "action:publish")
        .expect("publish node");
    assert_eq!(
        action["requirements"],
        serde_json::json!([{"id":"REQ-1","text":"publishing changes readiness"}])
    );

    let report = fsl_tools::undecided_declarations(&model);
    assert_eq!(report.len(), 1);
    assert_eq!(report[0]["declaration"], "action publish");
    assert_eq!(report[0]["reason"], "rollback policy is pending");
    assert_eq!(report[0]["requirement_ids"], serde_json::json!(["REQ-1"]));
}

#[test]
fn undecided_reports_use_typed_annotations_when_a_requirement_coexists() {
    let mut kernel = parse_kernel_source(
        r#"
spec TypedReport {
  state { ready: Bool }
  init { ready = false }
  action publish() "REQ-1: publishing changes readiness" { ready = true }
  reachable Published "REQ-2: publishing remains possible" { ready == true }
}
"#,
        &FsResolver::new("."),
    )
    .expect("parse spec");
    kernel.bind_annotation(
        action_target("publish"),
        Annotation::Undecided {
            reason: "review owner is pending".to_owned(),
            span: annotation_span(10),
        },
    );
    let model = build_model(kernel).expect("build model");

    let report = fsl_tools::undecided_declarations(&model);
    assert_eq!(report.len(), 1);
    assert_eq!(report[0]["declaration"], "action publish");
    assert_eq!(report[0]["reason"], "review owner is pending");
    assert_eq!(
        report[0]["requirement_ids"],
        serde_json::json!(["REQ-1", "REQ-2"])
    );
}
