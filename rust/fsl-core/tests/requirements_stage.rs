// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

#![allow(clippy::needless_raw_string_hashes)]

use fsl_core::{
    FsResolver, KernelExpr, PublicKernelVersion, RequirementsTraceExpectation, build_model,
    parse_kernel_source, parse_kernel_source_with_file, public_kernel_contract_for_version,
    requirements_trace_contract, verify_scope_type_names,
};

fn checked(source: &str) -> fsl_core::KernelModel {
    build_model(
        parse_kernel_source(source, &FsResolver::new(".")).expect("parse requirements source"),
    )
    .expect("build requirements model")
}

#[test]
fn requirements_terminal_lowers_stage_access_to_the_generated_state_map() {
    let model = checked(
        r#"
requirements ClaimLifecycle {
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  terminal { forall c: Claim { stage(c) == Approved } }
}
verify { instances Claim = 1 }
"#,
    );

    let KernelExpr::Quantified { body, .. } = model.terminal.expect("terminal expression") else {
        panic!("expected quantified terminal")
    };
    let KernelExpr::Binary { left, .. } = *body else {
        panic!("expected stage comparison")
    };
    assert_eq!(
        *left,
        KernelExpr::Index(
            Box::new(KernelExpr::Var("claim_stage".to_owned())),
            Box::new(KernelExpr::Var("c".to_owned())),
        )
    );
}

#[test]
fn qualified_stage_access_uses_the_shared_symbol_path_parser() {
    let model = checked(
        r#"
requirements claims {
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  invariant Qualified {
    forall c: Claim { claims.Claim.stage(c) == Draft or Claim.stage(c) == Approved }
  }
}
verify { instances Claim = 1 }
"#,
    );

    assert_eq!(model.invariants.len(), 1);
}

const QUALIFIED_PROCESSES: &str = r#"
requirements ClaimRequirements {
  process claims.Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  process legacy.Claim {
    stages Imported, Archived
    initial Imported
    transition archive Imported -> Archived by Manager
  }
  invariant Qualified {
    forall c: Claim { claims.Claim.stage(c) == Draft and legacy.Claim.stage(c) == Imported }
  }
}
verify { instances Claim = 1 }
"#;

#[test]
fn qualified_process_path_disambiguates_multiple_processes_for_one_entity() {
    let model = checked(QUALIFIED_PROCESSES);

    let stage_maps = model
        .state
        .iter()
        .filter(|(name, _)| name.starts_with("0q"))
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    assert_eq!(stage_maps.len(), 2);
    assert_ne!(stage_maps[0], stage_maps[1]);
}

#[test]
fn qualified_process_symbols_are_injective_across_segment_boundaries() {
    let source = QUALIFIED_PROCESSES
        .replace("claims.Claim", "a.b.Claim")
        .replace("legacy.Claim", "a_b.Claim");
    let model = checked(&source);

    assert_eq!(
        model
            .state
            .iter()
            .filter(|(name, _)| name.starts_with("0q"))
            .count(),
        2
    );
}

#[test]
fn unqualified_stage_reports_all_ambiguous_process_candidates() {
    let source = r#"
requirements ClaimRequirements {
  process claims.Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  process legacy.Claim {
    stages Imported, Archived
    initial Imported
    transition archive Imported -> Archived by Manager
  }
  invariant Ambiguous { forall c: Claim { stage(c) == Draft } }
}
verify { instances Claim = 1 }
"#;
    let error = parse_kernel_source(source, &FsResolver::new(".")).expect_err("stage must fail");

    assert!(error.message.contains("ambiguous for type 'Claim'"));
    assert!(error.message.contains("claims.Claim"));
    assert!(error.message.contains("legacy.Claim"));
}

#[test]
fn stage_rejects_an_untyped_argument_at_the_argument_span() {
    let source = r#"
requirements ClaimLifecycle {
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  invariant Invalid { stage(missing) == Draft }
}
verify { instances Claim = 1 }
"#;
    let error = parse_kernel_source(source, &FsResolver::new(".")).expect_err("stage must fail");

    assert!(
        error
            .message
            .contains("'missing' is not a typed parameter or binder")
    );
    assert_eq!(error.line, 8);
    assert_eq!(error.column, 29);
}

#[test]
fn stage_rejects_a_typed_value_whose_type_has_no_process() {
    let source = r#"
requirements ClaimLifecycle {
  type User = 0..1
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  invariant Invalid { forall user: User { stage(user) == Draft } }
}
verify { instances Claim = 1 }
"#;
    let error = parse_kernel_source(source, &FsResolver::new(".")).expect_err("stage must fail");

    assert!(error.message.contains("type 'User', which has no process"));
}

#[test]
fn business_and_requirements_stage_access_lower_to_the_same_expression() {
    let business = checked(
        r#"
business ClaimBusiness {
  actor Manager
  entity Claim
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  policy P-1 "valid stage" invariant {
    forall c: Claim { stage(c) == Draft or stage(c) == Approved }
  }
}
verify { instances Claim = 1 }
"#,
    );
    let requirements = checked(
        r#"
requirements ClaimRequirements {
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  invariant P_1 {
    forall c: Claim { stage(c) == Draft or stage(c) == Approved }
  }
}
verify { instances Claim = 1 }
"#,
    );

    assert_eq!(business.invariants[0].expr, requirements.invariants[0].expr);
}

#[test]
fn acceptance_expectation_uses_the_same_stage_resolver() {
    let contract = requirements_trace_contract(
        r#"
requirements ClaimLifecycle {
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  acceptance AC-1 "all claims start as draft" {
    expect forall c: Claim { stage(c) == Draft }
  }
}
verify { instances Claim = 1 }
"#,
    )
    .expect("trace contract")
    .expect("requirements traces");

    let Some(RequirementsTraceExpectation::Expr(KernelExpr::Quantified { body, .. })) =
        &contract.acceptance[0].expectation
    else {
        panic!("expected quantified acceptance expression")
    };
    let KernelExpr::Binary { left, .. } = body.as_ref() else {
        panic!("expected stage comparison")
    };
    assert!(matches!(
        left.as_ref(),
        KernelExpr::Index(collection, _)
            if collection.as_ref() == &KernelExpr::Var("claim_stage".to_owned())
    ));
}

#[test]
fn public_kernel_keeps_the_lowered_symbol_and_surface_stage_origin() {
    let source = r#"
requirements ClaimLifecycle {
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  invariant StageVisible { forall c: Claim { stage(c) == Draft } }
}
verify { instances Claim = 1 }
"#;
    let kernel = parse_kernel_source_with_file(
        source,
        &FsResolver::new("fixtures"),
        "fixtures/claim_lifecycle.fsl",
    )
    .expect("lower requirements");
    let model = build_model(kernel.clone()).expect("checked requirements model");
    let contract = public_kernel_contract_for_version(
        &kernel,
        &model,
        "fixtures/claim_lifecycle.fsl",
        "requirements",
        PublicKernelVersion::V2,
    )
    .expect("public Kernel v2");

    assert!(
        contract["properties"]["invariants"][0]["expression"]
            .to_string()
            .contains("claim_stage")
    );
    let stage_origin = contract["provenance"]["origins"]
        .as_array()
        .expect("origins")
        .iter()
        .find(|origin| {
            origin["lowering_steps"].as_array().is_some_and(|steps| {
                steps
                    .iter()
                    .any(|step| step["kind"] == "resolve_stage_access")
            })
        })
        .expect("stage origin");
    assert_eq!(stage_origin["primary"]["span"]["line"], 8);
    assert_eq!(
        stage_origin["lowering_steps"][0]["detail"],
        "stage(c) -> claim_stage[entity]"
    );
}

#[test]
fn requirement_action_parameters_use_stage_in_general_expressions() {
    let model = checked(
        r#"
requirements ClaimLifecycle {
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  requirement REQ-AUDIT "draft claims can be audited" {
    action audit(c: Claim) { requires stage(c) == Draft }
  }
}
verify { instances Claim = 1 }
"#,
    );

    let audit = model
        .actions
        .iter()
        .find(|action| action.name == "audit")
        .expect("audit action");
    assert!(matches!(
        &audit.requires[0],
        KernelExpr::Binary { left, .. }
            if matches!(left.as_ref(), KernelExpr::Index(collection, _)
                if collection.as_ref() == &KernelExpr::Var("claim_stage".to_owned()))
    ));
}

#[test]
fn requirements_process_sinks_do_not_create_an_implicit_terminal() {
    let model = checked(
        r#"
requirements ClaimLifecycle {
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
}
verify { instances Claim = 1 }
"#,
    );

    assert!(model.terminal.is_none());
}

#[test]
fn stage_member_is_checked_against_the_resolved_process_enum() {
    let kernel = parse_kernel_source(
        r#"
requirements ClaimLifecycle {
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  invariant Invalid { forall c: Claim { stage(c) == Missing } }
}
verify { instances Claim = 1 }
"#,
        &FsResolver::new("."),
    )
    .expect("stage access lowers before enum checking");
    let error = build_model(kernel).expect_err("unknown stage member must fail");

    assert!(error.message.contains("cannot type identifier 'Missing'"));
}

#[test]
fn stage_arity_is_rejected_by_the_shared_expression_parser() {
    let error = parse_kernel_source(
        r#"
requirements ClaimLifecycle {
  process Claim {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager
  }
  invariant Invalid { stage() == Draft }
}
verify { instances Claim = 1 }
"#,
        &FsResolver::new("."),
    )
    .expect_err("invalid stage arity must fail");

    assert_eq!(error.message, "stage expects exactly one argument");
    assert_eq!(error.line, 8);
}

/// #1229: the types whose domain is a verify scope are every `entity` /
/// `number` and every business / requirements `process`; a declared range
/// type is not one. A document whose entity types are not all in its own
/// source has none to offer (`None`), so a caller cannot read an out-of-scope
/// argument there as outside a declared type.
#[test]
fn verify_scope_type_names_lists_entity_number_and_process_types() {
    let names = |items: &[&str]| {
        Some(
            items
                .iter()
                .map(|item| (*item).to_owned())
                .collect::<std::collections::BTreeSet<_>>(),
        )
    };
    let requirements = r#"requirements R {
  entity Case
  number Qty
  type Amount = 0..3
  process Claim {
    stages Draft, Done
    initial Draft
    transition finish Draft -> Done by User
  }
  forbidden FB-1 "x" {
    finish(5)
    expect rejected
  }
}
verify { instances Case = 2  instances Claim = 2  values Qty = 0..3 }
"#;
    assert_eq!(
        verify_scope_type_names(requirements).expect("requirements"),
        names(&["Case", "Claim", "Qty"])
    );
    assert_eq!(
        requirements_trace_contract(requirements)
            .expect("trace contract")
            .expect("requirements contract")
            .scope_types,
        names(&["Case", "Claim", "Qty"]).expect("names")
    );
    let spec = "spec S {\n  entity Case\n  number Qty\n  type Amount = 0..3\n  state { n: Amount }\n  init { n = 0 }\n}\n";
    assert_eq!(
        verify_scope_type_names(spec).expect("spec"),
        names(&["Case", "Qty"])
    );
    let business = r#"business B {
  actor Agent
  entity Item
  process Ticket {
    stages Open, Done
    initial Open
    transition finish Open -> Done by Agent
  }
}
"#;
    assert_eq!(
        verify_scope_type_names(business).expect("business"),
        names(&["Item", "Ticket"])
    );
    let compose = "compose Sys {\n  use Comp as c from \"comp.fsl\"\n  state { n: 0..3 }\n  init { n = 0 }\n}\n";
    assert_eq!(verify_scope_type_names(compose).expect("compose"), None);
    let domain = "domain D {\n  type Status = Draft | Done\n  aggregate Order {\n    state {\n      status: Status = Draft;\n    }\n    command Finish {}\n  }\n}\n";
    assert_eq!(verify_scope_type_names(domain).expect("domain"), None);
}
