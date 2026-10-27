// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1335 / DESIGN-def: compose-source predicates expand before composition,
//! including sync arguments, without exporting component definitions. Each
//! use-position has an executable direct-spec predicate control. Before the fix the
//! compose reaches check with an `unlowered predicate call` (or an unexpanded
//! constant expression in a range); unused invalid defs can silently pass.
//! Glue predicates here use only compose-local state, independently of #1334.

#[path = "support/compose_scratch.rs"]
mod compose_scratch;

use compose_scratch::{check, scratch, verify};
use serde_json::json;

const COMPONENT: &str = "spec A {
  type Id = 0..1
  struct Record { n: Int }
  state { on: Bool }
  init { on = false }
  fair action step(i: Id) { requires i >= 0 on = i == 0 }
}
";

const DEFS: &str = "  def truth() = true
  def zero() = 0
  def one() = 1
  def ids() = Seq { 0, 1 }
";

fn compose(defs: &str, body: &str) -> String {
    format!("compose C {{\n  use A as a from \"a.fsl\"\n{defs}{body}\n}}\n")
}

fn use_position(body: &str) {
    let source = compose(DEFS, body);
    // Materialize the same flat component declarations with ordinary names.
    // Keeping the whole preamble on line 2 preserves the call-site lines.
    let direct = format!(
        "spec C {{\n  type Id = 0..1 struct Record {{ n: Int }} state {{ on: Bool }} init {{ on = false }} fair action step(i: Id) {{ requires i >= 0 on = i == 0 }}\n{DEFS}{}\n}}\n",
        body.replace("a.Id", "Id")
            .replace("a__Id", "Id")
            .replace("a__Record", "Record")
            .replace("a__step", "step")
            // Flatten only this fixture's known component action. Keep the
            // predicate call in the direct action, rather than evaluating it
            // in the test authoring code.
            .replace(
                " = a.step(zero()) {",
                " { requires zero() >= 0 on = zero() == 0 "
            )
            .replace(" = a.step(i) {", " { requires i >= 0 on = i == 0 ")
            .replace(" = a.step(0) {", " { requires 0 >= 0 on = true ")
    );
    let dir = scratch(
        "issue-1335-position",
        &[
            ("a.fsl", COMPONENT),
            ("c.fsl", &source),
            ("direct.fsl", &direct),
        ],
    );
    let (control, control_status) = check(&dir, "direct.fsl");
    assert_ne!(
        control["kind"], "parse",
        "invalid fixture syntax: {control}"
    );
    let direct_verdict = (control_status == 0).then(|| verify(&dir, "direct.fsl"));
    eprintln!(
        "{} direct control: check={control_status} result={} message={} loc={}; verify={}",
        std::thread::current().name().expect("test thread"),
        control["result"],
        control["message"],
        control["loc"],
        direct_verdict.as_ref().map_or_else(
            || "not applicable".to_owned(),
            |(value, code)| format!("{code} {} {}", value["result"], value["message"])
        )
    );
    let (checked, status) = check(&dir, "c.fsl");
    if control_status == 2 {
        assert_eq!(
            status, 2,
            "direct-spec rejection must be preserved: {checked}"
        );
        assert_eq!(checked, control);
        return;
    }
    assert_eq!(
        control_status, 0,
        "unexpected direct check result: {control}"
    );
    assert_eq!(
        status, 0,
        "expected check ok after predicate expansion: {checked}"
    );
    let kernel = fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new(&dir))
        .expect("lower compose");
    assert!(
        !kernel
            .syntax()
            .items
            .iter()
            .any(|item| matches!(item, fsl_syntax::SpecItem::Def { .. })),
        "defs must be removed before compose expansion"
    );
    assert!(
        !kernel.kernel_ast_v1().to_string().contains("[\"call\","),
        "all predicate calls, including helpful arguments, must be expanded"
    );
    let (control, control_status) = direct_verdict.expect("accepted direct source was verified");
    let (verified, status) = verify(&dir, "c.fsl");
    assert_eq!(
        status, control_status,
        "direct={control}; compose={verified}"
    );
    if control_status == 2 {
        assert_eq!(verified, control);
    } else {
        assert_eq!(verified["result"], control["result"], "{verified}");
    }
}

macro_rules! position {
    ($name:ident, $body:literal) => {
        #[test]
        fn $name() {
            use_position($body);
        }
    };
}

position!(invariant, "  invariant P { truth() }");
position!(trans, "  trans P { truth() }");
position!(reachable, "  reachable P { truth() }");
position!(leadsto_before, "  leadsTo P { truth() ~> true }");
position!(leadsto_after, "  leadsTo P { true ~> truth() }");
position!(leadsto_within, "  leadsTo P { true ~> within one() true }");
position!(
    leadsto_decreases,
    "  leadsTo P { true ~> true decreases zero() }"
);
position!(
    leadsto_helpful_argument,
    "  leadsTo P { true ~> true helpful a__step(zero()) }"
);
position!(
    leadsto_typed_binder_where,
    "  leadsTo P { forall i: a.Id where truth() { true ~> true } }"
);
position!(
    leadsto_range_binder,
    "  leadsTo P { forall i in zero()..one() where truth() { true ~> true } }"
);
position!(
    leadsto_collection_binder,
    "  leadsTo P { forall i in ids() where truth() { true ~> true } }"
);
position!(until_before, "  until P { truth() until true }");
position!(until_after, "  until P { true until truth() }");
position!(unless_before, "  unless P { truth() unless true }");
position!(unless_after, "  unless P { true unless truth() }");
position!(state_initializer, "  state { flag: Bool = truth() }");
position!(state_type_range, "  state { n: zero()..one() = 0 }");
position!(init_rhs, "  state { n: Int }\n  init { n = zero() }");
position!(
    init_if,
    "  state { n: Int }\n  init { if truth() { n = 0 } else { n = 1 } }"
);
position!(
    init_forall,
    "  state { flags: Map<a__Id, Bool> }\n  init { forall b: a__Id where truth() { flags[b] = false } }"
);
position!(
    init_lvalue_index,
    "  state { flags: Map<Bool, Bool> }\n  init { flags[truth()] = false }"
);
position!(glue_requires, "  action glue() { requires truth() }");
position!(glue_ensures, "  action glue() { ensures truth() }");
position!(
    glue_let,
    "  action glue() { let yes = truth() requires yes }"
);
position!(
    glue_rhs,
    "  state { n: Int }\n  init { n = 0 }\n  action glue() { n = zero() }"
);
position!(
    glue_if,
    "  state { n: Int }\n  init { n = 0 }\n  action glue() { if truth() { n = 0 } else { n = 1 } }"
);
position!(
    glue_forall,
    "  state { flags: Map<a__Id, Bool> }\n  init { forall b: a__Id { flags[b] = false } }\n  action glue() { forall b: a__Id where truth() { flags[b] = false } }"
);
position!(
    glue_lvalue_index,
    "  state { flags: Map<Bool, Bool> }\n  init { forall i in 0..1 { flags[i == 0] = false } }\n  action glue() { flags[truth()] = false }"
);
position!(
    glue_param_range,
    "  action glue(i in zero()..one()) { requires i >= 0 }"
);
position!(
    quantified_expression,
    "  invariant P { forall i in zero()..one() where truth() { i >= 0 } }"
);
position!(
    aggregate_expression,
    "  invariant P { count(i in ids() where truth()) == 2 }"
);
position!(
    aggregate_range_expression,
    "  invariant P { count(i in zero()..one() where truth()) == 2 }"
);
position!(
    nested_expression_children,
    "  invariant P { (if truth() then one() else zero()) == 1 and not (zero() > one()) }"
);
position!(
    option_constructor,
    "  invariant P { some(zero()) == some(0) }"
);
position!(set_literal, "  invariant P { Set { zero() } == Set { 0 } }");
position!(seq_literal, "  invariant P { Seq { zero() } == Seq { 0 } }");
position!(
    struct_literal_field,
    "  invariant P { a__Record { n: zero() } == a__Record { n: 0 } }"
);
position!(index_base_and_index, "  invariant P { ids()[zero()] == 0 }");
position!(
    method_receiver_and_argument,
    "  invariant P { ids().at(zero()) == 0 }"
);
position!(
    pattern_scrutinee,
    "  invariant P { some(zero()) is some(v) }"
);
position!(
    quantifier_body,
    "  invariant P { forall i in 0..1 { truth() } }"
);
position!(
    aggregate_value,
    "  invariant P { sum(i in 0..1 of one()) == 2 }"
);
position!(unary_named_argument, "  trans P { old(zero()) == zero() }");
position!(
    binary_named_arguments,
    "  invariant P { min(zero(), one()) == 0 }"
);
position!(negated_operand, "  invariant P { -zero() == 0 }");
position!(
    field_base,
    "  def make() = a__Record { n: 0 }\n  invariant P { make().n == 0 }"
);
position!(
    predicate_argument,
    "  def identity(v: Int) = v\n  invariant P { identity(zero()) == 0 }"
);
position!(
    ternary_named_arguments,
    "  state { edges: relation a__Id -> a__Id }\n  init { edges = Set {} }\n  def graph() = edges\n  invariant P { not reachable(graph(), zero(), one()) }"
);
position!(
    nested_lvalue_field_index,
    "  state { records: Map<a__Id, a__Record> }\n  init { forall i: a__Id { records[i] = a__Record { n: 0 } } }\n  action glue() { records[zero()].n = 0 }"
);
position!(sync_argument, "  action together() = a.step(zero()) { }");
position!(
    sync_param_range,
    "  action together(i in zero()..one()) = a.step(i) { }"
);
position!(
    sync_requires,
    "  action together() = a.step(0) { requires truth() }"
);
position!(
    sync_ensures,
    "  action together() = a.step(0) { ensures truth() }"
);
position!(
    sync_let,
    "  action together() = a.step(0) { let yes = truth() requires yes }"
);
position!(
    sync_rhs,
    "  state { n: Int }\n  init { n = 0 }\n  action together() = a.step(0) { n = zero() }"
);
position!(
    sync_if,
    "  state { n: Int }\n  init { n = 0 }\n  action together() = a.step(0) { if truth() { n = 0 } else { n = 1 } }"
);
position!(
    sync_forall,
    "  state { flags: Map<a__Id, Bool> }\n  init { forall b: a.Id { flags[b] = false } }\n  action together() = a.step(0) { forall b: a.Id where truth() { flags[b] = false } }"
);
position!(
    sync_lvalue_index,
    "  state { flags: Map<Bool, Bool> }\n  init { forall i in 0..1 { flags[i == 0] = false } }\n  action together() = a.step(0) { flags[truth()] = false }"
);

#[test]
fn false_predicate_is_not_replaced_with_true() {
    let source = "compose C {
  state { n: Int }
  init { n = 0 }
  def zero() = n == 0
  action break_it() { requires zero() n = 1 }
  invariant Zero { zero() }
}
";
    let direct = source.replacen("compose C", "spec C", 1);
    let dir = scratch(
        "issue-1335-false",
        &[("c.fsl", source), ("direct.fsl", &direct)],
    );
    for file in ["direct.fsl", "c.fsl"] {
        let (checked, status) = check(&dir, file);
        assert_eq!(status, 0, "{file}: {checked}");
        let (result, status) = verify(&dir, file);
        assert_eq!(status, 1, "{file}: {result}");
        assert_eq!(result["result"], "violated", "{result}");
        assert_eq!(result["invariant"], "Zero", "{result}");
        assert_eq!(result["last_action"]["name"], "break_it", "{result}");
    }
}

#[test]
fn alias_in_def_body_is_resolved_after_expansion() {
    let source = compose("  def off() = not a.on\n", "  invariant Off { off() }");
    let direct = "spec C {
  type Id = 0..1
  state { on: Bool }
  init { on = false }
  fair action step(i: Id) { requires i >= 0 on = i == 0 }
  def off() = not on
  invariant Off { off() }
}
";
    let dir = scratch(
        "issue-1335-alias",
        &[
            ("a.fsl", COMPONENT),
            ("c.fsl", &source),
            ("direct.fsl", direct),
        ],
    );
    for file in ["direct.fsl", "c.fsl"] {
        let (result, status) = verify(&dir, file);
        assert_eq!(status, 1, "{file}: {result}");
        assert_eq!(result["result"], "violated", "{result}");
        assert_eq!(result["invariant"], "Off", "{result}");
        assert_eq!(
            result["last_action"]["name"],
            if file == "direct.fsl" {
                "step"
            } else {
                "a.step"
            },
            "{result}"
        );
    }
}

fn diagnostic_contract(body: &str, message: &str) {
    let source = format!(
        "compose C {{\n  state {{ support: Bool }} init {{ support = false }}\n{body}\n}}\n"
    );
    let direct = source.replacen("compose C", "spec C", 1);
    let dir = scratch(
        "issue-1335-diagnostic",
        &[("c.fsl", &source), ("direct.fsl", &direct)],
    );
    let (expected, status) = check(&dir, "direct.fsl");
    assert_eq!(status, 2, "invalid diagnostic control: {expected}");
    assert!(
        expected["message"]
            .as_str()
            .expect("message")
            .contains(message),
        "{expected}"
    );
    let (actual, status) = check(&dir, "c.fsl");
    assert_eq!(status, 2, "expected located def error, got {actual}");
    // Error envelopes contain no solver-cost/cache observables. Compare all
    // fields, including kind, message, location, optional code and versions.
    assert_eq!(actual, expected);
}

#[test]
fn duplicate_def_matches_direct_spec_diagnostic_and_location() {
    diagnostic_contract(
        "  def ready() = true\n  def ready() = false",
        "duplicate def 'ready'",
    );
}

#[test]
fn wrong_arity_matches_direct_spec_diagnostic_and_location() {
    diagnostic_contract(
        "  def ready(v: Bool) = v\n  invariant P { ready() }",
        "expects 1 argument(s), got 0",
    );
}

#[test]
fn unknown_def_matches_direct_spec_diagnostic_and_location() {
    diagnostic_contract(
        "  invariant P { missing() }",
        "undefined predicate 'missing'",
    );
}

#[test]
fn recursion_matches_direct_spec_even_when_unused() {
    diagnostic_contract(
        "  def looped() = looped()",
        "recursive predicate definition is not allowed",
    );
}

#[test]
fn capture_rejection_matches_direct_spec() {
    diagnostic_contract(
        "  def all(v: Bool) = forall i in 0..1 { v }\n  invariant P { forall i in 0..1 { all(i == 0) } }",
        "would capture variable 'i'",
    );
}

#[test]
fn state_dependent_parameter_range_matches_direct_spec_rejection() {
    diagnostic_contract(
        "  state { n: Int }\n  init { n = 0 }\n  def current() = n\n  action glue(i in current()..1) { }",
        "unknown constant 'n'",
    );
}

#[test]
fn defs_are_source_local_and_share_no_variable_namespace() {
    let component = "spec A {
  state { on: Bool }
  init { on = false }
  def on() = not on
  invariant Off { on() }
}
";
    let source = "compose C {
  use A as a from \"a.fsl\"
  state { on: Bool }
  init { on = true }
  def on() = on
  def a() = on()
  invariant On { a() }
  action stay() { requires on() on = true }
}
";
    let dir = scratch(
        "issue-1335-lexical",
        &[("a.fsl", component), ("c.fsl", source)],
    );
    let (result, status) = verify(&dir, "c.fsl");
    assert_eq!(status, 0, "{result}");
    assert_eq!(result["result"], "verified", "{result}");
    assert_eq!(
        result["invariants_checked"],
        json!(["a.Off", "On"]),
        "{result}"
    );
}

#[test]
fn sync_argument_def_expands_before_capture_avoiding_substitution() {
    let component = "spec A {
  type Id = 0..1
  state { n: Id }
  init { n = 0 }
  action copy(i: Id) { let value = 0 n = i }
  reachable One { n == 1 }
}
";
    let source = "compose C {
  use A as a from \"a.fsl\"
  state { value: Int }
  init { value = 1 }
  def chosen() = value
  internal a.copy
  action together() = a.copy(chosen()) { }
}
";
    let dir = scratch(
        "issue-1335-sync-order",
        &[("a.fsl", component), ("c.fsl", source)],
    );
    let (result, status) = verify(&dir, "c.fsl");
    assert_eq!(status, 0, "{result}");
    assert_eq!(result["result"], "verified", "{result}");
    assert_eq!(
        result["reachables"]["a.One"]["witnessed_at_step"], 1,
        "{result}"
    );
}
