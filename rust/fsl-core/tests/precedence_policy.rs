// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_core::{
    FsResolver, KernelExpr, KernelLValue, KernelModel, KernelStatement, TypeRef, build_model,
    parse_kernel_source,
};

fn checked(source: &str) -> KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse business source"))
        .expect("build business model")
}

fn indexed_assignment<'a>(
    statements: &'a [KernelStatement],
    state_name: &str,
) -> Option<&'a KernelExpr> {
    statements.iter().find_map(|statement| match statement {
        KernelStatement::Assign {
            target: KernelLValue::Index(name, _),
            value,
            ..
        } if name == state_name => Some(value),
        KernelStatement::ForAll { statements, .. } => indexed_assignment(statements, state_name),
        _ => None,
    })
}

fn implication_history_name(expression: &KernelExpr) -> &str {
    let KernelExpr::Quantified { body, .. } = expression else {
        panic!("expected quantified precedence invariant")
    };
    let KernelExpr::Binary { op, right, .. } = body.as_ref() else {
        panic!("expected implication")
    };
    assert_eq!(op, "=>");
    let KernelExpr::Index(collection, _) = right.as_ref() else {
        panic!("expected history lookup")
    };
    let KernelExpr::Var(name) = collection.as_ref() else {
        panic!("expected history state variable")
    };
    name
}

fn compared_stage(expression: &KernelExpr) -> &str {
    let KernelExpr::Binary { op, right, .. } = expression else {
        panic!("expected stage comparison")
    };
    assert_eq!(op, "==");
    let KernelExpr::Var(stage) = right.as_ref() else {
        panic!("expected stage member")
    };
    stage
}

#[test]
fn lowering_materializes_history_init_updates_policy_and_stability() {
    let model = checked(
        r#"
business ReturnHandling {
  actor Manager
  entity Return
  process Return {
    stages Requested, Approved, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition refund Approved -> Refunded by Manager
    transition bypass Requested -> Refunded by Manager
  }
  @requirement("REQ-PRECEDENCE", "the authored control remains attached")
  policy CTRL-APPROVAL "no completion without approval"
    every Return reaching Refunded must have passed through Approved
}
verify { instances Return = 1 }
"#,
    );

    let history = "return_stage_via_Approved";
    assert!(model.state.contains(&(
        history.to_owned(),
        TypeRef::Map(
            Box::new(TypeRef::Named("Return".to_owned())),
            Box::new(TypeRef::Bool),
        ),
    )));
    assert_eq!(
        indexed_assignment(&model.init, history),
        Some(&KernelExpr::Bool(false))
    );

    let approve = model
        .actions
        .iter()
        .find(|action| action.name == "approve")
        .expect("approve action");
    assert_eq!(
        indexed_assignment(&approve.statements, history),
        Some(&KernelExpr::Bool(true))
    );
    let bypass = model
        .actions
        .iter()
        .find(|action| action.name == "bypass")
        .expect("bypass action");
    assert_eq!(indexed_assignment(&bypass.statements, history), None);

    let policy = model
        .invariants
        .iter()
        .find(|property| property.name == "CTRL-APPROVAL")
        .expect("precedence invariant");
    assert_eq!(implication_history_name(&policy.expr), history);
    assert_eq!(
        policy.meta.as_ref().map(|meta| meta.id.as_str()),
        Some("CTRL-APPROVAL")
    );
    assert!(
        policy
            .annotations
            .requirements()
            .expect("requirements")
            .iter()
            .any(|requirement| requirement.id == "REQ-PRECEDENCE")
    );

    let stability = model
        .invariants
        .iter()
        .find(|property| property.name == "CTRL-APPROVAL_stability")
        .expect("stability invariant");
    assert_eq!(implication_history_name(&stability.expr), history);
    assert_eq!(
        stability
            .meta
            .as_ref()
            .and_then(|meta| meta.text.as_deref()),
        Some(
            "stability: no completion without approval (auto-synthesized, dominated-set invariant for k-induction)"
        )
    );
}

#[test]
fn histories_deduplicate_by_process_and_stage_ordered_waypoint_set() {
    let model = checked(
        r#"
business Decisions {
  actor Manager
  entity Case
  process Case {
    stages Requested, Approved, Rejected, Closed, Archived
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition closeApproved Approved -> Closed by Manager
    transition closeRejected Rejected -> Closed by Manager
    transition archive Closed -> Archived by Manager
  }
  policy CTRL-CLOSE "decision before close"
    every Case reaching Closed must have passed through Rejected or Approved
  policy CTRL-ARCHIVE "decision before archive"
    every Case reaching Archived must have passed through Approved or Rejected
}
verify { instances Case = 1 }
"#,
    );

    let histories = model
        .state
        .iter()
        .filter(|(name, _)| name.contains("_via_"))
        .collect::<Vec<_>>();
    assert_eq!(histories.len(), 1);
    assert_eq!(histories[0].0, "case_stage_via_Approved_Rejected");

    for policy_name in ["CTRL-CLOSE", "CTRL-ARCHIVE"] {
        let policy = model
            .invariants
            .iter()
            .find(|property| property.name == policy_name)
            .expect("precedence invariant");
        assert_eq!(
            implication_history_name(&policy.expr),
            "case_stage_via_Approved_Rejected"
        );
    }
    assert_eq!(
        model
            .invariants
            .iter()
            .filter(|property| property.name.ends_with("_stability"))
            .count(),
        1
    );
}

#[test]
fn stability_uses_dominated_stages_not_every_downstream_stage() {
    let model = checked(
        r#"
business DownstreamDominator {
  actor Worker
  entity Item
  process Item {
    stages R, A, B, C
    initial R
    transition toA R -> A by Worker
    transition toB R -> B by Worker
    transition aToB A -> B by Worker
    transition aToC A -> C by Worker
  }
  policy CTRL-DOM "C is reached only via A"
    every Item reaching C must have passed through A
}
verify { instances Item = 1 }
"#,
    );

    let stability = model
        .invariants
        .iter()
        .find(|property| property.name == "CTRL-DOM_stability")
        .expect("stability invariant");
    let KernelExpr::Quantified { body, .. } = &stability.expr else {
        panic!("expected quantified stability invariant")
    };
    let KernelExpr::Binary { op, left, .. } = body.as_ref() else {
        panic!("expected implication")
    };
    assert_eq!(op, "=>");
    let KernelExpr::Binary {
        op,
        left: stage_a,
        right: stage_c,
    } = left.as_ref()
    else {
        panic!("expected two dominated-stage alternatives")
    };
    assert_eq!(op, "or");

    assert_eq!(compared_stage(stage_a), "A");
    assert_eq!(compared_stage(stage_c), "C");
}

#[test]
fn initial_waypoint_initializes_history_true() {
    let model = checked(
        r#"
business InitialWaypoint {
  actor Worker
  entity Item
  process Item {
    stages Requested, Done
    initial Requested
    transition finish Requested -> Done by Worker
  }
  policy CTRL-INITIAL "the initial stage is a waypoint"
    every Item reaching Done must have passed through Requested
}
verify { instances Item = 1 }
"#,
    );

    assert_eq!(
        indexed_assignment(&model.init, "item_stage_via_Requested"),
        Some(&KernelExpr::Bool(true))
    );
}
