// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    FsResolver, TERMINAL_TARGET, action_guard_target, action_statement_target, action_target,
    build_model, property_target, state_target,
};

fn model(source: &str) -> fsl_core::KernelModel {
    let kernel = fsl_core::parse_kernel_source(source, &FsResolver::new("."))
        .expect("lower domain")
        .with_source_file("fixtures/orders-多言語.fsl");
    build_model(kernel).expect("build checked model")
}

#[test]
fn carries_domain_origins_across_surface_model_and_expression_expansion() {
    let source = r"
domain Orders {
  type Status = Pending | Approved
  aggregate Order {
    state { status: Status = Pending; }

    // 多言語 comment keeps byte offsets and source columns distinct.
    command Approve {}
    event Approved {}
    decide Approve {
      requires status == Pending
      emits Approved
    }
    evolve Approved { status = Approved }
    invariant allowed { can(Approve) }
    invariant listed { status in [Pending, Approved] }
    invariant legacy { status == Pending || status == Approved }
  }
}
";
    let model = model(source);

    let allowed = model
        .property_origin("invariant", "Order_allowed")
        .expect("allowed origin");
    let allowed_site = allowed.primary.as_ref().expect("user source");
    assert_eq!(
        allowed_site.source_file.as_deref(),
        Some("fixtures/orders-多言語.fsl")
    );
    assert_eq!(allowed_site.span.expect("span").start.line, 15);
    let allowed_span = allowed_site.span.expect("span");
    assert!(allowed_span.start.offset > 0);
    assert!(allowed_span.end.offset > allowed_span.start.offset);
    assert_eq!(
        &source[allowed_span.start.offset..allowed_span.end.offset],
        "can(Approve)"
    );
    assert_eq!(
        allowed_site.declaration_path,
        ["Orders", "aggregate", "Order", "invariant", "allowed"]
    );
    assert!(
        allowed
            .lowering_steps
            .iter()
            .any(|step| step.kind == "expand_can")
    );

    let listed = model
        .property_origin("invariant", "Order_listed")
        .expect("membership origin");
    assert!(listed.lowering_steps.iter().any(|step| {
        step.kind == "expand_membership"
            && step.detail.as_deref() == Some("2 equality predicate(s)")
    }));
    let listed_span = listed
        .primary
        .as_ref()
        .and_then(|site| site.span)
        .expect("membership span");
    assert_eq!(
        &source[listed_span.start.offset..listed_span.end.offset],
        "status in [Pending, Approved]"
    );
    let listed_expression_targets = model
        .origins()
        .targets()
        .filter(|(target, _)| target.starts_with("property:invariant:Order_listed:expr:root"))
        .collect::<Vec<_>>();
    assert!(listed_expression_targets.len() >= 7);
    assert!(
        listed_expression_targets
            .iter()
            .all(|(_, origins)| { origins.first().is_some_and(|origin| origin.id == listed.id) })
    );
    let allowed_expression_targets = model
        .origins()
        .targets()
        .filter(|(target, _)| target.starts_with("property:invariant:Order_allowed:expr:root"))
        .collect::<Vec<_>>();
    assert!(allowed_expression_targets.len() >= 3);
    assert!(allowed_expression_targets.iter().all(|(_, origins)| {
        origins
            .first()
            .is_some_and(|origin| origin.id == allowed.id)
    }));
    let legacy = model
        .property_origin("invariant", "Order_legacy")
        .expect("legacy origin");
    assert!(
        legacy
            .lowering_steps
            .iter()
            .any(|step| step.kind == "normalize_legacy_operator")
    );

    let requirement = model
        .requirement_for(&property_target("invariant", "Order_allowed"))
        .expect("separate requirement relation");
    assert_eq!(requirement.id, "DOMAIN-INVARIANT");
    assert_ne!(allowed.id.0, requirement.id);
}

#[test]
fn represents_one_to_many_many_to_one_and_generated_only_nodes() {
    let source = r"
domain Orders {
  type Status = Pending | Approved
  aggregate Order {
    state { status: Status = Pending; }
    command Approve {}
    event Approved {}
    decide Approve {
      requires status == Pending
      emits Approved
    }
    evolve Approved { status = Approved }
    invariant safe { status == Pending || status == Approved }
  }
}
";
    let model = model(source);
    let action = model
        .origins()
        .primary_for(&action_target("order_approve"))
        .expect("action origin");
    assert!(action.generated);
    let action_span = action
        .primary
        .as_ref()
        .and_then(|site| site.span)
        .expect("decision source span");
    assert!(action_span.start.offset > 0);
    assert!(action_span.end.offset > action_span.start.offset);
    assert!(action.secondary.iter().any(|site| {
        site.declaration_path == ["Orders", "aggregate", "Order", "command", "Approve"]
    }));

    let generated_statement = model
        .origins()
        .primary_for(&action_statement_target("order_approve", 0))
        .expect("generated event assignment");
    assert_eq!(action.id, generated_statement.id);
    assert!(
        model
            .origins()
            .primary_for(&action_guard_target("order_approve", 0))
            .is_some()
    );

    let event_flag = model
        .origins()
        .primary_for(&state_target("event_Approved"))
        .expect("event flag origin");
    assert!(event_flag.generated);
    assert!(event_flag.primary.is_none());
    let terminal = model
        .origins()
        .primary_for(TERMINAL_TARGET)
        .expect("terminal origin");
    assert!(terminal.generated);
    assert!(terminal.primary.is_none());
}

#[test]
fn checked_model_validation_combines_colliding_domain_origins() {
    let kernel = fsl_core::parse_kernel_source_with_file(
        r"
domain Collision {
  aggregate FooBar { state { value: Int = 0; } }
  aggregate foo_bar { state { value: Int = 0; } }
}
",
        &FsResolver::new("."),
        "collision.fsl",
    )
    .expect("lower collision");
    let error = build_model(kernel).expect_err("generated state names collide");
    assert!(error.message.contains("duplicate state variable"));
    let origin = error.origin.expect("validation origin");
    assert_eq!(
        origin
            .primary
            .as_ref()
            .and_then(|site| site.source_file.as_deref()),
        Some("collision.fsl")
    );
    assert!(origin.secondary.iter().any(|site| {
        site.source_file.as_deref() == Some("collision.fsl")
            && site.declaration_path.iter().any(|part| part == "foo_bar")
    }));
}

#[test]
fn checked_type_validation_keeps_all_duplicate_declaration_origins() {
    let kernel = fsl_core::parse_kernel_source_with_file(
        r"
domain DuplicateType {
  type Count = 0..1
  type Count = 0..2
  aggregate Item { state { count: Count = 0; } }
}
",
        &FsResolver::new("."),
        "duplicate-type.fsl",
    )
    .expect("lower duplicate types");
    let error = build_model(kernel).expect_err("duplicate type rejected");
    assert!(error.message.contains("duplicate type"));
    let origin = error.origin.expect("type validation origin");
    let primary = origin.primary.as_ref().expect("first declaration");
    assert_eq!(primary.source_file.as_deref(), Some("duplicate-type.fsl"));
    assert_eq!(primary.span.expect("first type span").start.line, 3);
    assert!(origin.secondary.iter().any(|site| {
        site.source_file.as_deref() == Some("duplicate-type.fsl")
            && site.span.is_some_and(|span| span.start.line == 4)
    }));
}

#[test]
fn nested_lvalue_statement_keeps_the_original_assignment_span() {
    let source = r"
domain Nested {
  type Quantity = 0..2
  value_object Counter { value: Quantity = 0; }
  aggregate Item {
    state { counter: Counter; }
    command Put { value: Quantity; }
    event PutDone { value: Quantity; }
    decide Put { emits PutDone }
    evolve PutDone { counter.value = value }
    invariant nonnegative { counter.value >= 0 }
  }
}
";
    let model = model(source);
    let action = model
        .actions
        .iter()
        .find(|action| action.name == "item_put")
        .expect("put action");
    let statement_index = action
        .statements
        .iter()
        .rposition(|statement| matches!(statement, fsl_core::KernelStatement::Assign { .. }))
        .expect("nested assignment");
    let origin = model
        .origins()
        .primary_for(&action_statement_target("item_put", statement_index))
        .expect("assignment origin");
    let site = origin.primary.as_ref().expect("source assignment");
    assert!(site.declaration_path.join(".").contains("counter.value"));
    assert!(site.span.expect("assignment span").start.line > 0);
}

#[test]
fn parse_type_and_lowering_errors_keep_primary_and_secondary_source_origins() {
    let parse_error = fsl_core::parse_kernel_source_with_file(
        "domain Broken { aggregate Item { invariant bad { true ",
        &FsResolver::new("."),
        "broken.fsl",
    )
    .expect_err("parse error");
    let parse_primary = parse_error
        .origin
        .as_ref()
        .and_then(|origin| origin.primary.as_ref())
        .expect("parse primary origin");
    assert_eq!(parse_primary.source_file.as_deref(), Some("broken.fsl"));
    assert!(parse_primary.span.is_some());

    let type_error = fsl_core::parse_kernel_source_with_file(
        r"
domain Broken {
  aggregate Item {
    state { count: Int = 0; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched {}
    invariant bad { missing == 0 }
  }
}
",
        &FsResolver::new("."),
        "broken.fsl",
    )
    .expect_err("name/type error");
    let type_primary = type_error
        .origin
        .as_ref()
        .and_then(|origin| origin.primary.as_ref())
        .expect("type primary origin");
    assert_eq!(type_primary.source_file.as_deref(), Some("broken.fsl"));
    assert_eq!(type_primary.span.expect("type span").start.line, 9);

    let lowering_error = fsl_core::parse_kernel_source_with_file(
        r"
domain Recursive {
  aggregate Item {
    state { ready: Bool = true; }
    command Touch {}
    event Touched {}
    decide Touch { requires can(Touch) emits Touched }
    evolve Touched {}
    invariant enabled { can(Touch) }
  }
}
",
        &FsResolver::new("."),
        "recursive.fsl",
    )
    .expect_err("recursive lowering error");
    let lowering = lowering_error.origin.expect("lowering origin");
    assert_eq!(
        lowering
            .primary
            .as_ref()
            .and_then(|site| site.source_file.as_deref()),
        Some("recursive.fsl")
    );
    assert!(lowering.secondary.iter().any(|site| {
        site.source_file.as_deref() == Some("recursive.fsl")
            && site
                .declaration_path
                .last()
                .is_some_and(|item| item == "Touch")
    }));
}
