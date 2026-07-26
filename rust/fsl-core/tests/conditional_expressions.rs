// SPDX-License-Identifier: Apache-2.0

use fsl_core::{FsResolver, build_model, parse_kernel_source, parse_refinement};

fn build(source: &str) -> Result<fsl_core::KernelModel, fsl_core::ModelError> {
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse source");
    build_model(kernel)
}

#[test]
fn constant_conditionals_validate_both_branches_but_evaluate_only_the_selected_one() {
    let model = build(
        "spec S { const Limit = if true then 1 else 1 / 0 type N = 0..Limit state { x: N } init { x = 0 } action stay() { x = x } invariant I { true } }",
    )
    .expect("unselected partial operation must not run");
    assert_eq!(model.consts["Limit"], fsl_core::FslValue::Int(1));

    let unknown_source = "spec S {\n  const Limit = if true then 1 else Missing\n  type N = 0..Limit\n  state { x: N }\n  init { x = 0 }\n  action stay() { x = x }\n  invariant I { true }\n}";
    let unknown = build(unknown_source).expect_err("both branches must be name checked");
    assert!(unknown.message.contains("unknown constant 'Missing'"));
    let unknown_span = unknown
        .origin
        .as_ref()
        .and_then(|origin| origin.primary.as_ref())
        .and_then(|site| site.span)
        .expect("constant branch diagnostic span");
    assert_eq!(
        &unknown_source[unknown_span.start.offset..unknown_span.end.offset],
        "Missing"
    );

    let mismatch_source = "spec S {\n  const Limit = if true then 1 else false\n  type N = 0..Limit\n  state { x: N }\n  init { x = 0 }\n  action stay() { x = x }\n  invariant I { true }\n}";
    let mismatch = build(mismatch_source).expect_err("both branches must have one type");
    assert!(mismatch.message.contains("type mismatch"));
    let mismatch_span = mismatch
        .origin
        .as_ref()
        .and_then(|origin| origin.primary.as_ref())
        .and_then(|site| site.span)
        .expect("constant mismatch diagnostic span");
    assert_eq!(
        &mismatch_source[mismatch_span.start.offset..mismatch_span.end.offset],
        "false"
    );

    let bad_condition_source = "spec S {\n  const Limit = if 1 then 1 else 0\n  type N = 0..Limit\n  state { x: N }\n  init { x = 0 }\n  action stay() { x = x }\n  invariant I { true }\n}";
    let bad_condition = build(bad_condition_source).expect_err("constant condition must be Bool");
    let condition_span = bad_condition
        .origin
        .as_ref()
        .and_then(|origin| origin.primary.as_ref())
        .and_then(|site| site.span)
        .expect("constant condition diagnostic span");
    assert_eq!(
        &bad_condition_source[condition_span.start.offset..condition_span.end.offset],
        "1"
    );
}

#[test]
fn conditional_type_rules_apply_in_general_expression_positions() {
    build(
        "spec Ranking { type N = 0..2 state { pending: Bool, age: N, remaining: N } init { pending = false age = 0 remaining = 0 } action stay() { pending = pending age = age remaining = remaining } leadsTo Progress { pending ~> not pending decreases if pending then age else remaining } }",
    )
    .expect("conditional ranking expression");

    let bad_condition_source = "spec S {\n  state { x: Int }\n  init { x = 0 }\n  action stay() {\n    x = if 1 then 1 else 0\n  }\n  invariant I { true }\n}";
    let bad_condition = build(bad_condition_source).expect_err("condition must be Bool");
    assert!(bad_condition.message.contains("not assignable to Bool"));
    let condition_span = bad_condition
        .origin
        .as_ref()
        .and_then(|origin| origin.primary.as_ref())
        .and_then(|site| site.span)
        .expect("condition diagnostic span");
    assert_eq!(
        &bad_condition_source[condition_span.start.offset..condition_span.end.offset],
        "1"
    );

    let bad_branch_source = "spec S {\n  state { x: Int }\n  init { x = 0 }\n  action stay() {\n    x = if true then 1 else false\n  }\n  invariant I { true }\n}";
    let bad_branch = build(bad_branch_source).expect_err("branches must match");
    assert!(bad_branch.message.contains("not assignable to Int"));
    let branch_span = bad_branch
        .origin
        .as_ref()
        .and_then(|origin| origin.primary.as_ref())
        .and_then(|site| site.span)
        .expect("branch diagnostic span");
    assert_eq!(
        &bad_branch_source[branch_span.start.offset..branch_span.end.offset],
        "false"
    );

    let unknown_branch_source = "spec S {\n  state { x: Int }\n  init { x = 0 }\n  action stay() {\n    x = if true then 1 else Missing\n  }\n  invariant I { true }\n}";
    let unknown_branch = build(unknown_branch_source).expect_err("both branches are name checked");
    assert!(
        unknown_branch
            .message
            .contains("cannot type identifier 'Missing'")
    );
    let unknown_span = unknown_branch
        .origin
        .as_ref()
        .and_then(|origin| origin.primary.as_ref())
        .and_then(|site| site.span)
        .expect("unknown branch diagnostic span");
    assert_eq!(
        &unknown_branch_source[unknown_span.start.offset..unknown_span.end.offset],
        "Missing"
    );

    let inline_source = "spec S {\n  state { x: Int = if 1 then 0 else 1 }\n  action stay() { x = x }\n  invariant I { true }\n}";
    let inline = build(inline_source).expect_err("inline condition must be Bool");
    let inline_span = inline
        .origin
        .as_ref()
        .and_then(|origin| origin.primary.as_ref())
        .and_then(|site| site.span)
        .expect("inline condition diagnostic span");
    assert_eq!(
        &inline_source[inline_span.start.offset..inline_span.end.offset],
        "1"
    );
    assert!(!inline.to_string().contains("conflicting assignment"));
}

#[test]
fn refinement_conditionals_use_the_shared_static_type_rules() {
    let implementation = build(
        "spec Impl { type N = 0..1 state { x: N, gate: Bool } init { x = 0 gate = true } action stay() { x = x gate = gate } invariant I { true } }",
    )
    .expect("implementation");
    let abstraction = build(
        "spec Abs { type N = 0..1 state { y: N } init { y = 0 } action stay() { y = y } invariant I { true } }",
    )
    .expect("abstraction");

    parse_refinement(
        "refinement R { impl Impl abs Abs map y = if gate then x else 0 action stay() -> stay() }",
        &implementation,
        &abstraction,
    )
    .expect("valid conditional mapping");

    let unknown = parse_refinement(
        "refinement R { impl Impl abs Abs map y = if true then x else Missing + 1 action stay() -> stay() }",
        &implementation,
        &abstraction,
    )
    .expect_err("unselected branch must be name checked");
    assert!(unknown.message.contains("cannot type identifier 'Missing'"));

    let bad_condition_source = "refinement R {\n  impl Impl\n  abs Abs\n  map y = if x then 1 else 0\n  action stay() -> stay()\n}";
    let bad_condition = parse_refinement(bad_condition_source, &implementation, &abstraction)
        .expect_err("mapping condition must be Bool");
    assert!(bad_condition.message.contains("not assignable to Bool"));
    let condition_span = bad_condition.span.expect("mapping condition span");
    assert_eq!(
        &bad_condition_source[condition_span.start.offset..condition_span.end.offset],
        "x"
    );

    let mismatch_source = "refinement R {\n  impl Impl\n  abs Abs\n  map y = if true then 1 else false\n  action stay() -> stay()\n}";
    let mismatch = parse_refinement(mismatch_source, &implementation, &abstraction)
        .expect_err("mapping branches must match");
    assert!(
        mismatch.message.contains("not assignable"),
        "{}",
        mismatch.message
    );
    let branch_span = mismatch.span.expect("mapping branch span");
    assert_eq!(
        &mismatch_source[branch_span.start.offset..branch_span.end.offset],
        "false"
    );
}

#[test]
fn refinement_rejects_dynamic_aggregate_ranges() {
    let implementation = build(
        "spec Impl { state { lo: Int, hi: Int, total: Int } init { lo = 0 hi = 2 total = 0 } action stay() { lo = lo hi = hi total = total } invariant I { true } }",
    )
    .expect("implementation");
    let abstraction = build(
        "spec Abs { state { total: Int } init { total = 0 } action stay() { total = total } invariant I { true } }",
    )
    .expect("abstraction");

    let error = parse_refinement(
        "refinement R { impl Impl abs Abs map total = count(i in lo..hi) action stay() -> stay() }",
        &implementation,
        &abstraction,
    )
    .expect_err("dynamic refinement aggregate range must fail closed");
    assert!(error.message.contains("'lo' is not an integer const"));
}

#[test]
fn requirements_and_domain_dialects_share_conditional_syntax() {
    build(
        "requirements R { type N = 0..1 state { x: N, gate: Bool } init { x = 0 gate = true } action choose() { x = if gate then 1 else 0 gate = gate } invariant Choice { if gate then x == 0 else x == 1 } }",
    )
    .expect("requirements conditional");

    build(
        "domain D { enum Status { A, B } aggregate Item { id ItemId state { status: Status = A; gate: Bool = true; } command Step {} event Stepped {} decide Step { requires if gate then status == A else status == B emits Stepped } evolve Stepped { status = if gate then B else A } invariant Choice { if gate then status == A else status == B } } }",
    )
    .expect("domain conditional");
}
