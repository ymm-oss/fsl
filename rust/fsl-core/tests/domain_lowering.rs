// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    FsResolver, KernelExpr, KernelLValue, KernelStatement, build_model, parse_kernel_source,
};
use fsl_syntax::{ActionItem, SpecItem};

fn lower(source: &str) -> fsl_core::KernelSpec {
    parse_kernel_source(source, &FsResolver::new(".")).expect("lower domain directly")
}

fn lowering_error(source: &str) -> fsl_core::CoreError {
    parse_kernel_source(source, &FsResolver::new(".")).expect_err("domain lowering must fail")
}

#[test]
fn resolves_enum_members_membership_and_can_structurally() {
    let source = r"
domain Orders {
  type Status = Pending | Approved;
  aggregate Order {
    state {
      status: Status = Pending;
      seen: Set<Status> = Set {};
      current: Option<Status> = none;
    }
    command Approve {}
    event Approved {}
    decide Approve {
      requires Pending == status
      emits Approved
    }
    evolve Approved {
      status = Approved
      seen = Set {}
      current = none
    }
    invariant enabled { can(Approve) }
    invariant emptySet { status in [] }
    invariant noCurrent { current == none }
    invariant bound { current is some(x) => x == Pending }
  }
}
";
    let kernel = lower(source);
    let model = build_model(kernel).expect("checked model");
    let enabled = model
        .invariants
        .iter()
        .find(|item| item.name == "Order_enabled")
        .expect("enabled invariant");
    assert_eq!(
        enabled.expr,
        KernelExpr::Binary {
            op: "==".to_owned(),
            left: Box::new(KernelExpr::Var("Status_Pending".to_owned())),
            right: Box::new(KernelExpr::Var("order_status".to_owned())),
        }
    );
    assert_eq!(
        model
            .invariants
            .iter()
            .find(|item| item.name == "Order_emptySet")
            .expect("empty membership")
            .expr,
        KernelExpr::Bool(false)
    );
    assert!(
        model
            .invariants
            .iter()
            .any(|item| item.name == "Order_bound")
    );
}

#[test]
fn domain_and_kernel_expressions_lower_to_the_same_tree() {
    let domain = lower(
        r"
domain Orders {
  type Status = Pending | Approved
  aggregate Order {
    state { status: Status = Pending; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched { status = Pending }
    invariant pending { status == Pending }
  }
}
",
    );
    let kernel = parse_kernel_source(
        r"
spec Orders {
  enum Status { Status_Pending, Status_Approved }
  state { order_status: Status }
  init { order_status = Status_Pending }
  action order_touch() { order_status = Status_Pending }
  invariant Order_pending { order_status == Status_Pending }
  terminal { false }
}
",
        &FsResolver::new("."),
    )
    .expect("direct Kernel");
    let domain_expr = build_model(domain).expect("domain model").invariants[0]
        .expr
        .clone();
    let kernel_expr = build_model(kernel).expect("kernel model").invariants[0]
        .expr
        .clone();
    assert_eq!(domain_expr, kernel_expr);
}

#[test]
fn rejects_unknown_ambiguous_and_wrong_aggregate_symbols_at_source_spans() {
    let ambiguous = parse_kernel_source(
        r"
domain Ambiguous {
  type First = Pending | Done
  type Second = Pending | Closed
  aggregate Item {
    state { ready: Bool = false; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched { ready = true }
    invariant bad { Pending == Pending }
  }
}
",
        &FsResolver::new("."),
    )
    .expect_err("ambiguous member");
    assert!(
        ambiguous
            .message
            .contains("ambiguous enum member 'Pending'")
    );
    assert_eq!((ambiguous.line, ambiguous.column), (11, 21));

    let wrong_aggregate = parse_kernel_source(
        r"
domain Commands {
  aggregate First {
    state { ready: Bool = false; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched { ready = true }
    invariant bad { can(Approve) }
  }
  aggregate Second {
    state { ready: Bool = false; }
    command Approve {}
    event Approved {}
    decide Approve { emits Approved }
    evolve Approved { ready = true }
  }
}
",
        &FsResolver::new("."),
    )
    .expect_err("cross-aggregate can");
    assert!(
        wrong_aggregate
            .message
            .contains("belongs to another aggregate")
    );
    assert_eq!((wrong_aggregate.line, wrong_aggregate.column), (9, 21));
}

#[test]
fn lowers_nested_lvalues_without_rewriting_identifier_substrings() {
    let kernel = lower(
        r"
domain InventoryDomain {
  type Item = 0..1
  type Quantity = 0..2
  value_object Counter { value: Quantity = 0; }
  aggregate Inventory {
    state {
      total: Quantity = 0;
      counts: Map<Item, Quantity>;
      counter: Counter;
    }
    command Adjust { item: Item }
    event Adjusted { item: Item }
    decide Adjust { emits Adjusted }
    evolve Adjusted {
      counts[item] = counts[item] + 1
      counter.value = total
    }
  }
}
",
    );
    let surface = kernel.syntax();
    let action = surface
        .items
        .iter()
        .find_map(|item| match item {
            SpecItem::Action { name, items, .. } if name == "inventory_adjust" => Some(items),
            _ => None,
        })
        .expect("inventory action");
    let assignments = action.iter().filter_map(|item| match item {
        ActionItem::Statement(KernelStatement::Assign { target, value, .. }) => {
            Some((target, value))
        }
        _ => None,
    });
    assert!(assignments.clone().any(|(target, _)| matches!(
        target,
        KernelLValue::Index(name, KernelExpr::Var(index))
            if name == "inventory_counts" && index == "item"
    )));
    assert!(assignments.into_iter().any(|(target, _)| matches!(
        target,
        KernelLValue::Field(base, field)
            if field == "value" && matches!(base.as_ref(), KernelLValue::Var(name) if name == "inventory_counter")
    )));
}

#[test]
fn rejects_type_mismatch_at_the_domain_operator() {
    let error = parse_kernel_source(
        r"
domain Invalid {
  type Status = Pending | Done
  aggregate Item {
    state { status: Status = Pending; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched { status = Done }
    invariant bad { status == 1 }
  }
}
",
        &FsResolver::new("."),
    )
    .expect_err("type mismatch");
    assert!(error.message.contains("comparison operand type mismatch"));
    assert_eq!((error.line, error.column), (10, 28));
}

#[test]
fn validates_non_executable_domain_expression_positions() {
    let value_object = lowering_error(
        r"
domain InvalidValueObject {
  value_object Counter {
    value: Int = 0;
    invariant bad { missing >= 0 }
  }
  aggregate Item {
    state { counter: Counter; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched { counter.value = 0 }
  }
}

",
    );
    assert!(
        value_object
            .message
            .contains("unknown domain symbol 'missing'")
    );

    let unused_default = lowering_error(
        r"
domain InvalidUnusedDefault {
  value_object Unused { x: Int = missing; }
  aggregate Item {
    state { ready: Bool = false; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched { ready = true }
  }
}
",
    );
    assert!(
        unused_default
            .message
            .contains("unknown domain symbol 'missing'")
    );

    let stale = lowering_error(
        r"
domain InvalidStale {
  type Status = Pending | Done
  aggregate Item {
    state { status: Status = Pending; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched { status = Done }
    on_stale Touched when missing == Done { emits Touched }
  }
}
",
    );
    assert!(stale.message.contains("unknown domain symbol 'missing'"));

    let idempotency = lowering_error(
        r"
domain InvalidEffectPath {
  type RequestId = 0..1
  aggregate Item {
    state { ready: Bool = false; }
    command Request { id: RequestId }
    event Requested { id: RequestId }
    event Done { id: RequestId }
    decide Request { emits Requested }
    evolve Requested { ready = true }
    evolve Done { ready = false }
  }
  effect Work {
    async
    idempotency_key Missing.id
    correlation_id Requested.id
    handles Requested
    emits one_of [Done]
  }
}
",
    );
    assert!(
        idempotency
            .message
            .contains("unknown domain path root 'Missing'")
    );
}

#[test]
fn rejects_invalid_kernel_shaped_domain_expressions_during_check() {
    let wrong_method_type = lowering_error(
        r"
domain InvalidMethodType {
  type Status = Pending | Done
  aggregate Item {
    state { seen: Set<Status>; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched {}
    invariant bad { not seen.contains(1) }
  }
}
",
    );
    assert!(
        wrong_method_type
            .message
            .contains("collection method argument type mismatch")
    );

    let wrong_arity = lowering_error(
        r"
domain InvalidMethodArity {
  type Status = Pending | Done
  aggregate Item {
    state { seen: Set<Status>; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched {}
    invariant bad { not seen.contains() }
  }
}
",
    );
    assert!(
        wrong_arity
            .message
            .contains("invalid Set method 'contains'/0")
    );

    let non_option = lowering_error(
        r"
domain InvalidPattern {
  type Status = Pending | Done
  aggregate Item {
    state { status: Status = Pending; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched { status = Done }
    invariant bad { status is none }
  }
}
",
    );
    assert!(non_option.message.contains("requires an Option expression"));

    let missing_field = lowering_error(
        r"
domain InvalidStruct {
  value_object Pair { x: Int = 0; y: Int = 0; }
  aggregate Item {
    state { pair: Pair; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched {}
    invariant bad { pair == Pair { x: 0 } }
  }
}
",
    );
    assert!(
        missing_field
            .message
            .contains("missing struct field 'Pair.y'")
    );
}

#[test]
fn supports_structural_option_equality_without_binding() {
    let kernel = lower(
        r"
domain OptionEquality {
  type Status = Pending | Done
  aggregate Item {
    state {
      current: Option<Status> = none;
      previous: Option<Status> = some(Pending);
    }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched { current = some(Pending) }
    invariant constructor { current == some(Pending) or current == none }
    invariant values { current != previous or current == previous }
  }
}
",
    );
    let model = build_model(kernel).expect("Option equality should build");

    assert_eq!(model.invariants.len(), 2);
}

#[test]
fn rejects_option_equality_with_mismatched_inner_types() {
    let mismatch = lowering_error(
        r"
domain InvalidOptionComparison {
  type Status = Pending | Done
  type Outcome = Accepted | Rejected
  aggregate Item {
    state {
      current: Option<Status> = none;
      outcome: Option<Outcome> = none;
    }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched {}
    invariant bad { current == outcome }
  }
}
",
    );
    assert!(
        mismatch.message.contains("domain expression type mismatch"),
        "{}",
        mismatch.message
    );
}

#[test]
fn option_equality_does_not_introduce_a_binding() {
    let error = lowering_error(
        r"
domain EqualityDoesNotBind {
  type Status = Pending | Done
  aggregate Item {
    state { current: Option<Status> = some(Pending); }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched {}
    invariant bad { current == some(Pending) and value == Pending }
  }
}
",
    );
    assert!(error.message.contains("unknown domain symbol 'value'"));
}

#[test]
fn rejects_option_ordering() {
    let option_ordering = lowering_error(
        r"
domain InvalidOptionOrdering {
  type Status = Pending | Done
  aggregate Item {
    state { current: Option<Status> = none; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched {}
    invariant bad { current < none }
  }
}
",
    );
    assert!(
        option_ordering
            .message
            .contains("ordering operator requires numeric operands")
    );
}

#[test]
fn canonical_and_legacy_domain_enums_lower_to_the_same_checked_model() {
    let canonical = lower(
        r"
domain Orders {
  enum Status { Pending, Approved }
  aggregate Order {
    state { status: Status = Pending; }
    command Approve {}
    event ApprovedEvent {}
    decide Approve { requires status == Pending emits ApprovedEvent }
    evolve ApprovedEvent { status = Approved }
  }
}
",
    );
    let legacy = lower(
        r"
domain Orders {
  type Status = Pending | Approved;
  aggregate Order {
    state { status: Status = Pending; }
    command Approve {}
    event ApprovedEvent {}
    decide Approve { requires status == Pending emits ApprovedEvent }
    evolve ApprovedEvent { status = Approved }
  }
}
",
    );
    assert_eq!(
        build_model(canonical).unwrap(),
        build_model(legacy).unwrap()
    );
}

#[test]
fn canonical_domain_enum_rejects_empty_and_duplicate_members_at_source_spans() {
    let empty = parse_kernel_source("domain Empty { enum Status {} }", &FsResolver::new("."))
        .expect_err("empty enum");
    assert!(empty.message.contains("enum 'Status' has no members"));
    assert_eq!((empty.line, empty.column), (1, 16));

    let duplicate = parse_kernel_source(
        "domain Duplicate { enum Status { Pending, Pending } }",
        &FsResolver::new("."),
    )
    .expect_err("duplicate enum member");
    assert!(
        duplicate
            .message
            .contains("duplicate enum member 'Pending'")
    );
    assert_eq!((duplicate.line, duplicate.column), (1, 43));
}
