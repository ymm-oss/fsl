// SPDX-License-Identifier: Apache-2.0

//! Reached evidence is not lexical authority. Each independently selectable cell
//! checks ordinary truth/full caller restoration before its literal diagnostic map.
//! Native projection is one candidate level, not recursive forall expansion.
use fsl_core::{
    FsResolver, FslValue as Value, KernelExpr as Expr, KernelModel, build_model,
    parse_kernel_source,
};
use fsl_runtime::{Bindings, Monitor, State, eval, violating_bindings};
use serde_json::{Value as Json, json};

const DECLARATIONS: &str = "struct Box { flag:Bool } \
 state { o:Option<Int> = some(1), p:Option<Int> = some(2), n:Option<Int> = none, q:Seq<Bool,2> = Seq{true,false}, \
 r:relation Bool -> Bool = Set {} }";

fn model(expression: &str) -> KernelModel {
    let source =
        format!("spec W {{ {DECLARATIONS} action tick() {{}} invariant Q {{ {expression} }} }}\n");
    build_model(parse_kernel_source(&source, &FsResolver::new(".")).expect("parse fixture"))
        .unwrap_or_else(|error| panic!("checked fixture {expression}: {error:?}"))
}

fn literal_maps(expected: Json) -> Option<Vec<Bindings>> {
    fn value(literal: &Json) -> Value {
        match literal {
            Json::Bool(value) => Value::Bool(*value),
            Json::Array(items) => Value::Seq(items.iter().map(value).collect()),
            _ => Value::Int(literal.as_i64().expect("literal integer")),
        }
    }
    match expected {
        Json::Null => None,
        Json::Array(maps) => Some(
            maps.into_iter()
                .map(|map| {
                    map.as_object()
                        .expect("literal object")
                        .iter()
                        .map(|(key, literal)| (key.clone(), value(literal)))
                        .collect()
                })
                .collect(),
        ),
        _ => panic!("literal null or maps"),
    }
}

fn assert_cell(expression: &str, truth: bool, expected: Json) {
    let (model, state, expr) = checked_expr(expression);
    assert_evaluation(&model, &state, &expr, truth, expected);
}

fn assert_evaluation(model: &KernelModel, state: &State, expr: &Expr, truth: bool, expected: Json) {
    let expression = fsl_core::expr_text(expr);
    let original_state = state.clone();
    let before = Bindings::from([
        ("outer".into(), Value::Int(11)),
        ("k".into(), Value::Int(99)),
        ("v".into(), Value::Int(9)),
        ("w".into(), Value::Int(8)),
        ("z".into(), Value::Int(7)),
    ]);
    // Two ordinary calls detect accidental mutation/reuse independently of evidence.
    for _ in 0..2 {
        let mut caller = before.clone();
        assert_eq!(
            eval(expr, state, &mut caller, model, None).unwrap(),
            Value::Bool(truth),
            "ordinary truth: {expression}"
        );
        assert_eq!(caller, before, "ordinary caller: {expression}");
        assert_eq!(*state, original_state, "ordinary state: {expression}");
    }
    let actual = violating_bindings(expr, state, model).unwrap();
    assert_eq!(*state, original_state, "diagnostic state: {expression}");
    let expected = literal_maps(expected);
    eprintln!("CELL {expression}: expected={expected:?} produced={actual:?}");
    assert_eq!(actual, expected, "complete diagnostic frame: {expression}");
}

macro_rules! cell {
    ($name:ident, $expression:literal, $truth:literal, $expected:tt) => {
        #[test]
        fn $name() {
            assert_cell($expression, $truth, json!($expected));
        }
    };
}

macro_rules! candidate {
    ($name:ident, $body:literal, $expected:tt) => {
        #[test]
        fn $name() {
            assert_cell(
                concat!("forall k in 0..0 { ", $body, " }"),
                false,
                json!($expected),
            );
        }
    };
}

// Logical, eager, constructor, access and named-operator child edges. Distinct
// successful matches make loss of an individual child observable, not just loss
// of the common Is hook. No oracle is obtained from the evaluator under test.
candidate!(or_left_r3, "((o is some(v)) and v == 2) or false", [{"k":0,"v":1}]);
candidate!(or_right, "false or ((o is some(v)) and false)", [{"k":0,"v":1}]);
candidate!(implication, "(o is some(v)) => false", [{"k":0,"v":1}]);
candidate!(negation, "not (o is some(v))", [{"k":0,"v":1}]);
candidate!(condition_and_then, "if o is some(v) then (p is some(w) and false) else true", [{"k":0,"v":1,"w":2}]);
candidate!(selected_else, "if false then true else (p is some(w) and false)", [{"k":0,"w":2}]);
candidate!(comparison_operands, "(o is some(v)) == not (p is some(w))", [{"k":0,"v":1,"w":2}]);
candidate!(some_seq, "some(Seq {o is some(v), p is some(w)}) == some(Seq {false,false})", [{"k":0,"v":1,"w":2}]);
candidate!(set_receiver_argument, "Set {o is some(v)}.contains(p is some(w)) == false", [{"k":0,"v":1,"w":2}]);
candidate!(seq_receiver_argument, "Seq {o is some(v)}.contains(p is some(w)) == false", [{"k":0,"v":1,"w":2}]);
candidate!(generic_index_base, "Seq {o is some(v)}[0] == false", [{"k":0,"v":1}]);
// Checked concrete-runtime-only edge: current native verify rejects this
// Seq-of-struct access with "indexing requires a map or sequence" on f8/CED.
// This is not a public CLI packet anchor or a claim of symbolic support.
candidate!(runtime_only_generic_index_and_field, "Seq {Box {flag:o is some(v)}}[if p is some(w) then 0 else 1].flag == false", [{"k":0,"v":1,"w":2}]);
candidate!(struct_field_and_optimized_index, "q[if (Box {flag:o is some(v)}).flag then 0 else 1] == false", [{"k":0,"v":1}]);
cell!(bindings_root_index, "forall local in Seq {q} { local[if o is some(v) then 0 else 1] == false }", false, [{"local":[true,false],"v":1}]);
candidate!(neg_abs, "abs(-(if o is some(v) then 0 else 1)) == 1", [{"k":0,"v":1}]);
candidate!(min_both_operands, "min(if o is some(v) then 0 else 1, if p is some(w) then 0 else 1) == 1", [{"k":0,"v":1,"w":2}]);
candidate!(max_both_operands, "max(if o is some(v) then 0 else 1, if p is some(w) then 0 else 1) == 1", [{"k":0,"v":1,"w":2}]);
candidate!(arithmetic_operands, "(if o is some(v) then 1 else 2) + (if p is some(w) then 1 else 2) == 0", [{"k":0,"v":1,"w":2}]);
candidate!(division_operands, "(if o is some(v) then 4 else 2) / (if p is some(w) then 2 else 1) == 0", [{"k":0,"v":1,"w":2}]);
candidate!(remainder_operands, "(if o is some(v) then 3 else 2) % (if p is some(w) then 2 else 1) == 0", [{"k":0,"v":1,"w":2}]);
candidate!(reachable_all_operands, "reachable(if o is some(v) then r else r, p is some(w), some(3) is some(z))", [{"k":0,"v":1,"w":2,"z":3}]);
candidate!(relation_acyclic, "not acyclic(if o is some(v) then r else r)", [{"k":0,"v":1}]);
candidate!(relation_functional, "not functional(if o is some(v) then r else r)", [{"k":0,"v":1}]);
candidate!(relation_injective, "not injective(if o is some(v) then r else r)", [{"k":0,"v":1}]);
candidate!(relation_domain, "domain(if o is some(v) then r else r).contains(false)", [{"k":0,"v":1}]);
candidate!(relation_range, "range(if o is some(v) then r else r).contains(false)", [{"k":0,"v":1}]);
candidate!(is_scrutinee, "some(o is some(v)) is some(w) and false", [{"k":0,"v":1,"w":true}]);

// Candidate construction, selected-filter/body separation, no stale first match.
cell!(selected_filter_body, "forall k in 0..0 where o is some(w) and w == 1 { not (p is some(v)) }", false, [{"k":0,"w":1,"v":2}]);
cell!(
    rejected_filter,
    "forall k in 0..0 where o is some(w) and w == 2 { not (p is some(v)) }",
    true,
    null
);
cell!(
    true_body,
    "forall k in 0..0 where o is some(w) and w == 1 { p is some(v) }",
    true,
    null
);
cell!(varied_candidates, "forall k in 0..1 { not ((if k == 0 then o else p) is some(v)) }", false, [{"k":0,"v":1},{"k":1,"v":2}]);
cell!(absent_first, "forall k in 0..1 { (if k == 0 then n else p) is some(v) and false }", false, [{"k":0},{"k":1,"v":2}]);
cell!(absent_second, "forall k in 0..1 { (if k == 0 then o else n) is some(v) and false }", false, [{"k":0,"v":1},{"k":1}]);
cell!(filtered_candidate_discarded, "forall k in 0..1 where (if k == 0 then o else p) is some(w) and w == 2 { not (o is some(v)) }", false, [{"k":1,"w":2,"v":1}]);
cell!(nonfailed_candidate_discarded, "forall k in 0..1 { (if k == 0 then o else p) is some(v) and v == 1 }", false, [{"k":1,"v":2}]);
candidate!(absent_option, "n is some(v) and false", [{"k":0}]);

// Skipped children have different names and values; full map equality rejects
// a source prewalk, absent payload fabrication, and resumed-parent frame loss.
candidate!(skipped_and, "((o is some(v)) and false) and (p is some(skipped))", [{"k":0,"v":1}]);
candidate!(skipped_or, "((o is some(v)) or (p is some(skipped))) == false", [{"k":0,"v":1}]);
candidate!(skipped_implication, "(((o is some(v)) and false) => (p is some(skipped))) == false", [{"k":0,"v":1}]);
candidate!(skipped_else, "if o is some(v) then false else (p is some(skipped))", [{"k":0,"v":1}]);
candidate!(skipped_then, "if not (o is some(v)) then (p is some(skipped)) else false", [{"k":0,"v":1}]);
candidate!(nested_forall, "(o is some(v)) and (forall j in 0..0 where p is some(inner_filter) { p is some(inner_body) }) and (p is some(w)) and false", [{"k":0,"v":1,"w":2}]);
candidate!(nested_exists, "(o is some(v)) and (exists j in 0..0 where p is some(inner_filter) { p is some(inner_body) }) and (p is some(w)) and false", [{"k":0,"v":1,"w":2}]);
candidate!(nested_count, "(o is some(v)) and count(j in 0..0 where p is some(inner_filter)) == 1 and (p is some(w)) and false", [{"k":0,"v":1,"w":2}]);
candidate!(nested_sum, "(o is some(v)) and sum(j in 0..0 of if p is some(inner_value) then 1 else 0 where p is some(inner_filter)) == 1 and (p is some(w)) and false", [{"k":0,"v":1,"w":2}]);
candidate!(nested_unique, "(o is some(v)) and unique(j in 0..0 where p is some(inner_filter)) and (p is some(w)) and false", [{"k":0,"v":1,"w":2}]);
candidate!(nested_exactly_one, "(o is some(v)) and exactlyOne(j in 0..0 where p is some(inner_filter)) and (p is some(w)) and false", [{"k":0,"v":1,"w":2}]);
cell!(collection_source_isolation, "forall k in Seq {o is some(source_capture)} { false }", false, [{"k":true}]);
// Concrete-runtime-only dynamic bounds: native verify requires static bounds;
// checked AST evaluation still has range-source child edges to isolate.
cell!(runtime_only_range_source_isolation, "forall k in (if o is some(lo_capture) then 0 else 1)..(if p is some(hi_capture) then 0 else 1) { false }", false, [{"k":0}]);
candidate!(nested_source_isolation, "(o is some(v)) and (forall j in Seq {p is some(source_capture)} { j }) and (p is some(w)) and false", [{"k":0,"v":1,"w":2}]);
candidate!(nested_aggregate_source_isolation, "(o is some(v)) and count(j in Seq {p is some(source_capture)}) == 1 and (p is some(w)) and false", [{"k":0,"v":1,"w":2}]);

cell!(fallback_not, "not (o is some(v))", false, [{"v":1}]);
cell!(fallback_conditional, "if o is some(v) then (p is some(w) and false) else true", false, [{"v":1,"w":2}]);
cell!(fallback_eager, "Seq {o is some(v)} == Seq {false}", false, [{"v":1}]);
cell!(fallback_false_and, "false and true", false, [{}]);
cell!(fallback_true, "true", true, null);
cell!(conjunction_search_resets, "(o is some(v)) and not (p is some(w))", false, [{"w":2}]);
cell!(quantified_left_search, "(forall k in 0..0 { o is some(v) and v == 2 }) and false", false, [{"k":0,"v":1}]);

// Direct checked/rewrite controls below are deliberately not admission claims.
// They exercise old-state and invalid/internal nodes unavailable in invariants.
fn checked_expr(expression: &str) -> (KernelModel, State, Expr) {
    let model = model(expression);
    let state = Monitor::new(model.clone()).unwrap().state;
    let expr = model
        .invariants
        .iter()
        .find(|p| p.name == "Q")
        .unwrap()
        .expr
        .clone();
    (model, state, expr)
}

#[test]
fn old_uses_old_state_and_missing_old_preserves_error() {
    let (model, state, expr) = checked_expr("o is some(v) and v == 2");
    let old = State::from([("o".into(), Value::Some(Box::new(Value::Int(2))))]);
    let mut caller = Bindings::from([("v".into(), Value::Int(9))]);
    let before = caller.clone();
    assert_eq!(
        eval(&expr, &state, &mut caller, &model, None).unwrap(),
        Value::Bool(false)
    );
    let old_expr = Expr::UnaryNamed {
        name: "old".into(),
        expr: Box::new(expr),
        span: model.invariants[0].span,
    };
    assert_eq!(
        eval(&old_expr, &state, &mut caller, &model, Some(&old)).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(caller, before);
    let error = eval(&old_expr, &state, &mut caller, &model, None).unwrap_err();
    assert_eq!(error.message, "old() used without old state");
    assert_eq!(error.span, None);
    assert_eq!(error.partial_operation, None);
    assert!(!error.map_key_outside_domain);
    assert_eq!(caller, before);
    assert_eq!(violating_bindings(&old_expr, &state, &model), Err(error));
}

#[test]
fn lazy_closed_children_do_not_evaluate_missing_names_or_partial_ops() {
    let (model, state, _) = checked_expr("false and true");
    for (op, left, truth) in [
        ("and", false, false),
        ("or", true, true),
        ("=>", false, true),
    ] {
        for right in [
            Expr::Var("missing".into()),
            Expr::Binary {
                op: "/".into(),
                left: Box::new(Expr::Num(1)),
                right: Box::new(Expr::Num(0)),
            },
        ] {
            let expr = Expr::Binary {
                op: op.into(),
                left: Box::new(Expr::Bool(left)),
                right: Box::new(right),
            };
            let mut caller = Bindings::from([("v".into(), Value::Int(9))]);
            let before = caller.clone();
            assert_eq!(
                eval(&expr, &state, &mut caller, &model, None).unwrap(),
                Value::Bool(truth)
            );
            assert_eq!(caller, before);
        }
    }
    // Public ordinary eval, not property totalization, establishes real laziness.
}

#[test]
fn rewritten_filter_observation_is_not_body_lookup_authority() {
    let (model, state, mut expr) = checked_expr("forall k in 0..0 where o is some(w) { false }");
    if let Expr::Quantified { body, .. } = &mut expr {
        **body = Expr::Var("w".into());
    } else {
        panic!("quantifier");
    }
    let mut caller = Bindings::from([("outer".into(), Value::Int(9))]);
    let before = caller.clone();
    let error = eval(&expr, &state, &mut caller, &model, None).unwrap_err();
    assert_eq!(error.message, "unknown identifier 'w'");
    assert_eq!(caller, before);
    assert_eq!(violating_bindings(&expr, &state, &model), Err(error));
}

// Internal-only checked rewrites: the checker intentionally rejects nested Seq
// state, Seq-of-struct state and collection-valued struct fields. Start with
// checked scalar access/indices, then substitute concrete values and access
// chains to exercise eval_place's otherwise unreachable structural combinations.
// These are NOT public source admission or symbolic conformance claims.
fn rewritten_access(shape: &str, generic: bool) {
    let source = if generic {
        "forall k in 0..0 { (if true then q else q)[if o is some(v) then 0 else 1] == false }"
    } else {
        "forall k in 0..0 { q[if o is some(v) then 0 else 1] == false }"
    };
    let (model, mut state, mut expr) = checked_expr(source);
    let (_, _, second) = checked_expr("q[if p is some(w) then 0 else 1] == false");
    let Expr::Binary { left: second, .. } = second else {
        panic!("comparison");
    };
    let Expr::Index(_, second_index) = *second else {
        panic!("index");
    };
    let Expr::Quantified { body, .. } = &mut expr else {
        panic!("candidate");
    };
    let Expr::Binary { left, .. } = body.as_mut() else {
        panic!("comparison");
    };
    let scalar = Value::Bool(true);
    let sequence = Value::Seq(vec![scalar.clone(), Value::Bool(false)]);
    let (value, access) = match shape {
        "two_indices" => (
            Value::Seq(vec![sequence]),
            Expr::Index(left.clone(), second_index),
        ),
        "field_between" => (
            Value::Seq(vec![Value::Struct {
                type_name: "InternalRow".into(),
                fields: std::collections::BTreeMap::from([("cells".into(), sequence)]),
            }]),
            Expr::Index(
                Box::new(Expr::Field(left.clone(), "cells".into())),
                second_index,
            ),
        ),
        "field_after" => (
            Value::Seq(vec![Value::Struct {
                type_name: "Box".into(),
                fields: std::collections::BTreeMap::from([("flag".into(), scalar)]),
            }]),
            Expr::Field(left.clone(), "flag".into()),
        ),
        "field_before" => {
            let Expr::Index(base, index) = left.as_ref() else {
                panic!("index");
            };
            (
                Value::Struct {
                    type_name: "InternalRow".into(),
                    fields: std::collections::BTreeMap::from([("cells".into(), sequence)]),
                },
                Expr::Index(
                    Box::new(Expr::Field(base.clone(), "cells".into())),
                    index.clone(),
                ),
            )
        }
        "map" => (
            Value::Map(std::collections::BTreeMap::from([(Value::Int(0), scalar)])),
            *left.clone(),
        ),
        _ => panic!("finite shape"),
    };
    state.insert("q".into(), value);
    **left = access;
    let expected = if matches!(shape, "two_indices" | "field_between") {
        json!([{"k":0,"v":1,"w":2}])
    } else {
        json!([{"k":0,"v":1}])
    };
    assert_evaluation(&model, &state, &expr, false, expected);
}

macro_rules! access_pair {
    ($optimized:ident,$generic:ident,$shape:literal) => {
        #[test]
        fn $optimized() {
            rewritten_access($shape, false);
        }
        #[test]
        fn $generic() {
            rewritten_access($shape, true);
        }
    };
}
access_pair!(
    internal_optimized_two_indices,
    internal_generic_two_indices,
    "two_indices"
);
access_pair!(
    internal_optimized_field_between,
    internal_generic_field_between,
    "field_between"
);
access_pair!(
    internal_optimized_field_after,
    internal_generic_field_after,
    "field_after"
);
access_pair!(
    internal_optimized_field_before,
    internal_generic_field_before,
    "field_before"
);
access_pair!(internal_optimized_map, internal_generic_map, "map");

#[test]
fn rewritten_pattern_shadow_preserves_original_binder_projection() {
    let (model, state, mut expr) = checked_expr("forall k in 0..0 { o is some(v) and v == 0 }");
    let Expr::Quantified { body, .. } = &mut expr else {
        panic!("candidate");
    };
    let Expr::Binary { left, right, .. } = body.as_mut() else {
        panic!("conjunction");
    };
    let Expr::Is { pattern, .. } = left.as_mut() else {
        panic!("pattern");
    };
    *pattern = fsl_core::Pattern::Some("k".into());
    let Expr::Binary { left, .. } = right.as_mut() else {
        panic!("comparison");
    };
    **left = Expr::Var("k".into());
    // Payload 1 makes the body false; incorrectly reading diagnostic binder 0
    // would make it true. The complete ordinary caller still retains k99.
    assert_evaluation(&model, &state, &expr, false, json!([{"k":0}]));
}

#[test]
fn rewritten_repeated_key_keeps_first_observation_but_lexical_last_match() {
    let (model, state, mut expr) =
        checked_expr("forall k in 0..0 { o is some(v) and p is some(w) and w == 1 }");
    let Expr::Quantified { body, .. } = &mut expr else {
        panic!("candidate");
    };
    let Expr::Binary { left, right, .. } = body.as_mut() else {
        panic!("conjunction");
    };
    let Expr::Binary { right: second, .. } = left.as_mut() else {
        panic!("inner conjunction");
    };
    let Expr::Is { pattern, .. } = second.as_mut() else {
        panic!("second match");
    };
    *pattern = fsl_core::Pattern::Some("v".into());
    let Expr::Binary { left, .. } = right.as_mut() else {
        panic!("comparison");
    };
    **left = Expr::Var("v".into());
    // Rewritten duplicate names remain illegal source. The lexical comparison
    // sees second payload 2; the diagnostic key projection retains first 1.
    assert_evaluation(&model, &state, &expr, false, json!([{"k":0,"v":1}]));
}

fn assert_error(
    model: &KernelModel,
    state: &State,
    expr: &Expr,
    message: &str,
    partial: Option<fsl_core::PartialOperation>,
) {
    let before = Bindings::from([
        ("v".into(), Value::Int(9)),
        ("outer".into(), Value::Int(11)),
    ]);
    let original_state = state.clone();
    let expected = fsl_runtime::RuntimeError {
        message: message.into(),
        span: None,
        partial_operation: partial,
        map_key_outside_domain: false,
    };
    let mut caller = before.clone();
    assert_eq!(
        eval(expr, state, &mut caller, model, None),
        Err(expected.clone())
    );
    assert_eq!(caller, before);
    assert_eq!(violating_bindings(expr, state, model), Err(expected));
    assert_eq!(*state, original_state);
}

#[test]
fn errors_after_reached_match_do_not_publish_partial_frames() {
    let (model, state, mut expr) = checked_expr("forall k in 0..0 { (o is some(v)) and false }");
    let Expr::Quantified { body, .. } = &mut expr else {
        panic!("candidate");
    };
    let Expr::Binary { right, .. } = body.as_mut() else {
        panic!("conjunction");
    };
    **right = Expr::Var("missing".into());
    assert_error(&model, &state, &expr, "unknown identifier 'missing'", None);
}

#[test]
fn optimized_and_generic_access_preserve_root_and_inner_failure_order() {
    for source in [
        "q[if o is some(v) then 0 else 1] == false",
        "(if true then q else q)[if o is some(v) then 0 else 1] == false",
    ] {
        let (model, mut state, mut expr) = checked_expr(source);
        // Missing root wins over an error in the index.
        let Expr::Binary { left, .. } = &mut expr else {
            panic!("comparison");
        };
        let Expr::Index(_, index) = left.as_mut() else {
            panic!("index");
        };
        **index = Expr::Var("index_missing".into());
        state.remove("q");
        assert_error(&model, &state, &expr, "unknown identifier 'q'", None);

        // A failed inner step wins over an unevaluated outer index.
        let (model, mut state, mut expr) = checked_expr(source);
        state.insert("q".into(), Value::Seq(vec![]));
        let Expr::Binary { left, .. } = &mut expr else {
            panic!("comparison");
        };
        **left = Expr::Index(left.clone(), Box::new(Expr::Var("outer_missing".into())));
        assert_error(
            &model,
            &state,
            &expr,
            "sequence index out of range",
            Some(fsl_core::PartialOperation::Index),
        );
    }
}

#[test]
fn unsupported_nodes_preserve_error_priority_without_visiting_children() {
    let (model, state, expr) = checked_expr("false");
    let span = model.invariants[0].span;
    let missing = Box::new(Expr::Var("missing".into()));
    for (expr, message) in [
        (
            Expr::Call {
                name: "internal".into(),
                args: vec![*missing.clone()],
                span,
            },
            "unexpanded predicate call 'internal'",
        ),
        (
            Expr::Stage {
                process: None,
                entity: missing.clone(),
                entity_span: span,
                span,
            },
            "unlowered stage access",
        ),
        (
            Expr::UnaryNamed {
                name: "unsupported".into(),
                expr: missing.clone(),
                span,
            },
            "unsupported unary expression 'unsupported'",
        ),
        (
            Expr::TernaryNamed {
                name: "unsupported".into(),
                first: missing.clone(),
                second: missing.clone(),
                third: Box::new(expr),
            },
            "unsupported ternary function 'unsupported'",
        ),
        (
            Expr::BinaryNamed {
                name: "unsupported".into(),
                left: Box::new(Expr::Num(0)),
                right: Box::new(Expr::Num(1)),
            },
            "unsupported binary function 'unsupported'",
        ),
        (
            Expr::BinaryNamed {
                name: "unsupported".into(),
                left: missing,
                right: Box::new(Expr::Num(1)),
            },
            "unknown identifier 'missing'",
        ),
    ] {
        assert_error(&model, &state, &expr, message, None);
    }
}

#[test]
fn eager_observations_never_supply_a_closed_sibling_lookup() {
    let (model, state, mut expr) = checked_expr("(o is some(v)) == false");
    let Expr::Binary { right, .. } = &mut expr else {
        panic!("comparison");
    };
    **right = Expr::Binary {
        op: "==".into(),
        left: Box::new(Expr::Var("v".into())),
        right: Box::new(Expr::Num(1)),
    };
    let mut caller = Bindings::from([("v".into(), Value::Int(9))]);
    let before = caller.clone();
    assert_eq!(
        eval(&expr, &state, &mut caller, &model, None).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(caller, before);
    let error = violating_bindings(&expr, &state, &model).unwrap_err();
    assert_eq!(
        error,
        fsl_runtime::RuntimeError {
            message: "unknown identifier 'v'".into(),
            span: None,
            partial_operation: None,
            map_key_outside_domain: false
        }
    );
}
