// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_syntax::{Annotation, DomainSpec, parse_ai_component, parse_db_system, parse_domain};

fn domain(source: &str) -> DomainSpec {
    parse_domain(source).expect("parse domain")
}

#[test]
fn aggregate_command_decide_evolve_and_projection_accept_leading_annotations() {
    let spec = domain(
        r#"
domain Orders {
  aggregate Order {
    id OrderId
    state { ready: Bool }
    @requirement("REQ-CMD")
    command Place { input qty: Int }
    @requirement("REQ-DECIDE")
    decide Place { emits Placed }
    event Placed { qty: Int }
    @requirement("REQ-EVOLVE")
    evolve Placed { ready = true }
    @requirement("REQ-INVARIANT")
    invariant Ready { ready == ready }
    @requirement("REQ-PROJECTION")
    projection OrderView { from Order fields [ready] }
  }
}
"#,
    );
    let aggregate = &spec.aggregates[0];
    assert_eq!(aggregate.commands[0].annotations.source_order().len(), 1);
    assert_eq!(aggregate.decides[0].annotations.source_order().len(), 1);
    assert_eq!(aggregate.evolves[0].annotations.source_order().len(), 1);
    assert_eq!(aggregate.invariants[0].annotations.source_order().len(), 1);
    assert_eq!(spec.projections[0].annotations.source_order().len(), 1);
}

#[test]
fn effect_and_saga_step_accept_leading_annotations() {
    let spec = domain(
        r#"
domain Orders {
  aggregate Order {
    id OrderId
    state { ready: Bool }
    command Place { input qty: Int }
    decide Place { emits Placed }
    event Placed { qty: Int }
    event Shipped { qty: Int }
    evolve Placed { ready = true }
  }
  @requirement("REQ-EFFECT")
  effect Ship {
    handles Placed
    success_event Shipped
  }
  saga Fulfillment {
    starts_on Placed
    @requirement("REQ-STEP")
    step Notify {
      awaits one_of [Shipped]
    }
  }
}
"#,
    );
    assert_eq!(spec.effects[0].annotations.source_order().len(), 1);
    assert_eq!(spec.sagas[0].steps[0].annotations.source_order().len(), 1);
}

#[test]
fn stray_annotation_before_unsupported_domain_declaration_is_a_target_error() {
    let error =
        parse_domain("domain Bad {\n  @requirement(\"REQ-1\")\n  type SchemaVersion = 0..1\n}\n")
            .unwrap_err();
    assert_eq!(error.code(), "FSL-ANNOTATION-TARGET");
}

#[test]
fn stray_annotation_before_closing_brace_in_aggregate_is_a_target_error() {
    let error = parse_domain(
        "domain Bad {\n  aggregate Order {\n    id OrderId\n    state { ready: Bool }\n    @requirement(\"REQ-1\")\n  }\n}\n",
    )
    .unwrap_err();
    assert_eq!(error.code(), "FSL-ANNOTATION-TARGET");
}

#[test]
fn ai_authority_rules_accept_annotations_and_expose_plain_name_python_ast() {
    let component = parse_ai_component(
        r#"
ai_component Assistant {
  tool Search { schema SearchV1; }
  tool Execute { schema ExecuteV1; }
  authority {
    @requirement("REQ-SUGGEST")
    may_suggest Search;
    @requirement("REQ-EXECUTE")
    may_execute Execute;
  }
}
"#,
    )
    .expect("parse ai_component");
    assert_eq!(component.authority.may_suggest[0].name, "Search");
    assert_eq!(
        component.authority.may_suggest[0]
            .annotations
            .source_order()
            .len(),
        1
    );
    assert_eq!(component.authority.may_execute[0].name, "Execute");
    let ast = component.python_ast();
    assert_eq!(
        ast["authority"]["may_suggest"],
        serde_json::json!(["Search"])
    );
    assert_eq!(
        ast["authority"]["may_execute"],
        serde_json::json!(["Execute"])
    );
}

#[test]
fn ai_scalar_declaration_before_annotation_is_a_target_error() {
    let error =
        parse_ai_component("ai_component Bad {\n  @requirement(\"REQ-1\")\n  model \"gpt\";\n}\n")
            .unwrap_err();
    assert_eq!(error.code(), "FSL-ANNOTATION-TARGET");
}

#[test]
fn db_migration_and_check_rule_accept_annotations() {
    let system = parse_db_system(
        r#"
dbsystem Orders {
  database Db {
    schema 0
    table orders {
      column status: Value present backfilled not_null;
    }
  }
  @requirement("REQ-MIGRATION")
  migration AddStatus from 0 to 1 {
    add orders.status not_null;
  }
  check compatibility {
    @requirement("REQ-RULE")
    rule not_null_after_backfill;
  }
}
"#,
    )
    .expect("parse dbsystem");
    assert_eq!(
        system.migrations[0].decl_annotations.source_order().len(),
        1
    );
    assert!(matches!(
        &system.check.rules[0].annotations.source_order()[0],
        Annotation::Requirement { id, .. } if id == "REQ-RULE"
    ));
}

#[test]
fn db_migration_op_before_annotation_is_a_target_error() {
    let error = parse_db_system(
        "dbsystem Bad {\n  database Db {\n    schema 0\n    table orders {\n      column status: Value present backfilled not_null;\n    }\n  }\n  migration AddStatus from 0 to 1 {\n    @requirement(\"REQ-1\")\n    add orders.status not_null;\n  }\n}\n",
    )
    .unwrap_err();
    assert_eq!(error.code(), "FSL-ANNOTATION-TARGET");
}

#[test]
fn stray_annotation_before_saga_does_not_leak_into_its_first_declaration() {
    let error = parse_domain(
        r#"
domain Bad {
  @undecided("whole saga design pending review")
  saga Fulfillment {
    invariant Stable { true }
  }
}
"#,
    )
    .unwrap_err();
    assert_eq!(error.code(), "FSL-ANNOTATION-TARGET");
}

#[test]
fn stray_annotation_inside_saga_compensation_is_a_target_error() {
    let error = parse_domain(
        r#"
domain Bad {
  aggregate Order {
    id OrderId
    state { ready: Bool }
    event A { }
    event B { }
  }
  saga Fulfillment {
    compensation {
      @requirement("REQ-1")
      when A after B {
        emits A
      }
    }
  }
}
"#,
    )
    .unwrap_err();
    assert_eq!(error.code(), "FSL-ANNOTATION-TARGET");
}

#[test]
fn ai_check_hard_rule_accepts_annotations() {
    let component = parse_ai_component(
        r#"
ai_component Assistant {
  tool Search { schema SearchV1; }
  check hard {
    @requirement("REQ-RULE")
    rule tool_authority;
    rule tool_schema_declared;
  }
}
"#,
    )
    .expect("parse ai_component");
    assert_eq!(component.check.rules[0].name, "tool_authority");
    assert_eq!(component.check.rules[0].annotations.source_order().len(), 1);
    assert!(
        component.check.rules[1]
            .annotations
            .source_order()
            .is_empty()
    );
    let ast = component.python_ast();
    assert_eq!(
        ast["check"]["rules"],
        serde_json::json!(["tool_authority", "tool_schema_declared"])
    );
}

#[test]
fn ai_fallback_conflicting_block_and_item_requirement_text_is_a_checked_error() {
    let error = parse_ai_component(
        r#"
ai_component Assistant {
  tool Search { schema SearchV1; }
  @requirement("REQ-X", "block text")
  fallback {
    @requirement("REQ-X", "item text")
    when low_confidence require human_review;
  }
}
"#,
    )
    .unwrap_err();
    assert_eq!(error.code(), "FSL-ANNOTATION-INVALID");
}
